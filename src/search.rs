// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Exact counting by prefix partitions when a complete layer exceeds the budget.

use std::collections::TryReserveError;
use std::ops::ControlFlow;

use num_bigint::BigUint;

use crate::counter::{
    DpEvent, DpScratch, count_patterns_dp, count_patterns_seeded, dp_table_bytes,
    effective_max_length, is_unconstrained,
};
use crate::grid::GridDefinition;
use crate::mask::{MAX_POINTS, Mask};
use crate::numeric::{CountEvent, GlobalCount, count_unconstrained, local_counts_fit};
use crate::symmetry::starting_orbits;

/// Counting-table allocation and the prefix length used to stay within it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CountPlan {
    /// Zero uses the ordinary layered counter. Positive values partition by prefix.
    pub prefix_length: usize,
    /// Peak allocation for the reusable counting layers, excluding the input matrix.
    pub table_bytes: u64,
}

/// Choose the shortest prefix whose remaining layered count fits the budget.
/// A zero-byte budget eventually selects traversal without counting tables.
///
/// # Panics
/// Panics when `max_length > n` or `n > MAX_POINTS`.
#[must_use]
pub fn count_plan(n: usize, max_length: usize, budget: u64) -> CountPlan {
    count_plan_with::<u128>(n, max_length, budget)
}

/// Choose a prefix satisfying table storage and the count representation's limits.
///
/// Arbitrary-precision accumulation uses continuations whose unconstrained upper
/// bound fits `u128`, keeping every seeded DP count exact before weighting it.
///
/// # Panics
/// Panics when `max_length > n` or `n > MAX_POINTS`.
#[must_use]
pub fn count_plan_with<C: GlobalCount>(n: usize, max_length: usize, budget: u64) -> CountPlan {
    assert!(max_length <= n && n <= MAX_POINTS);
    for prefix_length in 0..=max_length {
        let table_bytes = dp_table_bytes(n - prefix_length, max_length - prefix_length);
        if table_bytes <= budget
            && (!C::ARBITRARY_PRECISION
                || local_counts_fit(n - prefix_length, max_length - prefix_length))
        {
            return CountPlan {
                prefix_length,
                table_bytes,
            };
        }
    }
    unreachable!("a count with no remaining visits allocates no layers")
}

/// Count all requested lengths exactly without exceeding the counting-table budget.
///
/// When the dense layers do not fit, this partitions valid prefixes and counts each
/// continuation independently. Verified node symmetries reduce starting points.
/// The budget bounds table storage, not runtime: deep partitions can require
/// exponentially more work. Only globally finalized lengths are emitted.
///
/// # Errors
/// Returns an allocation error if the selected counting layers cannot be allocated.
///
/// # Panics
/// Panics for an invalid grid, a mismatched block matrix or `max_length > n`.
pub fn count_patterns_bounded<M: Mask, F: FnMut(DpEvent) -> ControlFlow<()>>(
    grid: &GridDefinition,
    blocks: &[M],
    max_length: usize,
    budget: u64,
    mut on_event: F,
) -> Result<(), TryReserveError> {
    count_patterns_bounded_with::<M, u128, _>(grid, blocks, max_length, budget, |event| {
        on_event(event.into())
    })
}

/// Count with arbitrary-precision global accumulation and compact `u128` continuations.
///
/// The table budget and finalized-length event semantics match
/// [`count_patterns_bounded`]. Exact accumulator payloads need at most 710 bits
/// for the supported 127 points, separately from the table budget.
///
/// # Errors
/// Returns an allocation error if the selected counting layers cannot be allocated.
///
/// # Panics
/// Panics for an invalid grid, a mismatched block matrix or `max_length > n`.
pub fn count_patterns_big<M: Mask, F: FnMut(CountEvent<BigUint>) -> ControlFlow<()>>(
    grid: &GridDefinition,
    blocks: &[M],
    max_length: usize,
    budget: u64,
    on_event: F,
) -> Result<(), TryReserveError> {
    count_patterns_bounded_with(grid, blocks, max_length, budget, on_event)
}

/// Shared bounded traversal for fixed-width and arbitrary-precision global counts.
///
/// # Errors
/// Returns an allocation error if the selected counting layers cannot be allocated.
///
/// # Panics
/// Panics for an invalid grid, a mismatched block matrix or `max_length > n`.
pub fn count_patterns_bounded_with<
    M: Mask,
    C: GlobalCount,
    F: FnMut(CountEvent<C>) -> ControlFlow<()>,
>(
    grid: &GridDefinition,
    blocks: &[M],
    max_length: usize,
    budget: u64,
    mut on_event: F,
) -> Result<(), TryReserveError> {
    let n = grid.node_count();
    assert!(n <= M::MAX_POINTS && max_length <= n);
    assert_eq!(blocks.len(), n * n);
    let Some(direct_length) = count_direct(n, blocks, max_length, budget, &mut on_event)? else {
        return Ok(());
    };
    let orbits = starting_orbits(grid, blocks);
    // Finish each requested length before starting the next. Lower layers are
    // repeated, but interruption preserves every previously completed count.
    for length in direct_length + 1..=max_length {
        let plan = count_plan_with::<C>(n, length, budget);
        let remaining = n - plan.prefix_length;
        let tail_length = length - plan.prefix_length;
        let scratch = DpScratch::allocate_constrained(remaining, tail_length)?;
        let mut counter = PrefixCounter {
            n,
            blocks,
            full_mask: M::low_bits(n),
            prefix_length: plan.prefix_length,
            tail_length,
            count: C::default(),
            scratch,
            reduced: if tail_length > 1 {
                vec![M::ZERO; remaining * remaining]
            } else {
                Vec::new()
            },
            on_event: &mut on_event,
            cancelled: false,
            overflow: false,
        };
        for &(start, weight) in &orbits {
            counter.visit(M::bit(start), start, 1, weight);
            if counter.cancelled {
                return Ok(());
            }
            if counter.overflow {
                break;
            }
        }
        if counter.overflow {
            let _ = (counter.on_event)(CountEvent::Overflow);
            return Ok(());
        }
        if (counter.on_event)(CountEvent::LengthDone {
            length,
            count: counter.count,
        })
        .is_break()
        {
            return Ok(());
        }
    }
    Ok(())
}

/// Return the last direct length when partitioned continuations are still needed.
fn count_direct<M: Mask, C: GlobalCount, F: FnMut(CountEvent<C>) -> ControlFlow<()>>(
    n: usize,
    blocks: &[M],
    max_length: usize,
    budget: u64,
    on_event: &mut F,
) -> Result<Option<usize>, TryReserveError> {
    if is_unconstrained(blocks) {
        count_unconstrained(n, max_length, n as u128, on_event);
        return Ok(None);
    }
    let mut direct_length = effective_max_length(n, max_length, budget);
    if C::ARBITRARY_PRECISION {
        while !local_counts_fit(n, direct_length) {
            direct_length -= 1;
        }
    }
    let mut scratch = DpScratch::allocate(n, blocks, direct_length)?;
    if direct_length == max_length {
        count_patterns_dp(&mut scratch, n, blocks, max_length, |event| {
            on_event(event.into())
        });
        return Ok(None);
    }
    let mut stopped = false;
    count_patterns_dp(&mut scratch, n, blocks, direct_length, |event| {
        stopped |= matches!(event, DpEvent::Overflow);
        let flow = on_event(event.into());
        stopped |= flow.is_break();
        flow
    });
    Ok((!stopped).then_some(direct_length))
}

struct PrefixCounter<'a, M, C, F> {
    n: usize,
    blocks: &'a [M],
    full_mask: M,
    prefix_length: usize,
    tail_length: usize,
    count: C,
    scratch: DpScratch,
    reduced: Vec<M>,
    on_event: F,
    cancelled: bool,
    overflow: bool,
}

impl<M: Mask, C: GlobalCount, F: FnMut(CountEvent<C>) -> ControlFlow<()>>
    PrefixCounter<'_, M, C, F>
{
    fn visit(&mut self, visited: M, last: usize, length: usize, weight: u128) {
        if self.cancelled || self.overflow {
            return;
        }
        if (self.on_event)(CountEvent::Mask).is_break() {
            self.cancelled = true;
            return;
        }
        if length == self.prefix_length {
            self.continue_prefix(visited, last, weight);
            return;
        }
        let mut free = self.full_mask & !visited;
        while free != M::ZERO && !self.cancelled && !self.overflow {
            let bit = free & free.wrapping_neg();
            free ^= bit;
            let next = bit.trailing_zeros() as usize;
            if self.blocks[last * self.n + next] & !visited == M::ZERO {
                self.visit(visited | bit, next, length + 1, weight);
            }
        }
    }

    fn continue_prefix(&mut self, visited: M, last: usize, weight: u128) {
        if self.tail_length == 1 {
            let mut free = self.full_mask & !visited;
            let mut degree = 0u128;
            while free != M::ZERO {
                let bit = free & free.wrapping_neg();
                free ^= bit;
                let next = bit.trailing_zeros() as usize;
                if self.blocks[last * self.n + next] & !visited == M::ZERO {
                    degree += 1;
                }
            }
            self.overflow = !self.count.add_scaled(degree, weight);
            return;
        }
        let remaining = self.n - self.prefix_length;
        let mut nodes = [0usize; MAX_POINTS];
        let mut inverse = [0usize; MAX_POINTS];
        let mut free = self.full_mask & !visited;
        let mut position = 0;
        let mut starts = M::ZERO;
        while free != M::ZERO {
            let bit = free & free.wrapping_neg();
            free ^= bit;
            let node = bit.trailing_zeros() as usize;
            nodes[position] = node;
            inverse[node] = position;
            if self.blocks[last * self.n + node] & !visited == M::ZERO {
                starts |= M::bit(position);
            }
            position += 1;
        }
        for (a, &origin) in nodes[..remaining].iter().enumerate() {
            for (b, &target) in nodes[..remaining].iter().enumerate() {
                let mut blockers = self.blocks[origin * self.n + target] & !visited;
                let mut mapped = M::ZERO;
                while blockers != M::ZERO {
                    let bit = blockers & blockers.wrapping_neg();
                    blockers ^= bit;
                    mapped |= M::bit(inverse[bit.trailing_zeros() as usize]);
                }
                self.reduced[a * remaining + b] = mapped;
            }
        }
        let tail_length = self.tail_length;
        let total = &mut self.count;
        let overflow = &mut self.overflow;
        let cancelled = &mut self.cancelled;
        let on_event = &mut self.on_event;
        count_patterns_seeded(
            &mut self.scratch,
            remaining,
            &self.reduced,
            self.tail_length,
            starts,
            |event| {
                match event {
                    DpEvent::LengthDone { length, count } => {
                        if length == tail_length {
                            *overflow = !total.add_scaled(count, weight);
                        }
                    }
                    DpEvent::Overflow => *overflow = true,
                    DpEvent::Mask => {
                        if on_event(CountEvent::Mask).is_break() {
                            *cancelled = true;
                        }
                    }
                }
                if *cancelled || *overflow {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        );
    }
}

#[cfg(test)]
mod tests;
