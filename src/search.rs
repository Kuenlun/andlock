// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Exact counting by prefix partitions when a complete layer exceeds the budget.

use std::collections::TryReserveError;
use std::ops::ControlFlow;

use crate::counter::{
    DpEvent, DpScratch, count_patterns_dp, count_patterns_seeded, dp_table_bytes,
    effective_max_length, is_unconstrained,
};
use crate::grid::GridDefinition;
use crate::mask::{MAX_POINTS, Mask};
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
    assert!(max_length <= n && n <= MAX_POINTS);
    for prefix_length in 0..=max_length {
        let table_bytes = dp_table_bytes(n - prefix_length, max_length - prefix_length);
        if table_bytes <= budget {
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
    let n = grid.node_count();
    assert!(n <= M::MAX_POINTS && max_length <= n);
    assert_eq!(blocks.len(), n * n);
    let direct_length = if is_unconstrained(blocks) {
        max_length
    } else {
        effective_max_length(n, max_length, budget)
    };
    let mut stopped = false;
    {
        let mut scratch = DpScratch::allocate(n, blocks, direct_length)?;
        if direct_length == max_length {
            count_patterns_dp(&mut scratch, n, blocks, max_length, on_event);
            return Ok(());
        }
        count_patterns_dp(&mut scratch, n, blocks, direct_length, |event| {
            stopped |= matches!(event, DpEvent::Overflow);
            let flow = on_event(event);
            stopped |= flow.is_break();
            flow
        });
    }
    if stopped {
        return Ok(());
    }
    let orbits = starting_orbits(grid, blocks);
    // Finish each requested length before starting the next. Lower layers are
    // repeated, but interruption preserves every previously completed count.
    for length in direct_length + 1..=max_length {
        let plan = count_plan(n, length, budget);
        let remaining = n - plan.prefix_length;
        let tail_length = length - plan.prefix_length;
        let scratch = DpScratch::allocate_constrained(remaining, tail_length)?;
        let mut counter = PrefixCounter {
            n,
            blocks,
            full_mask: M::low_bits(n),
            prefix_length: plan.prefix_length,
            tail_length,
            cap: length,
            counts: [0; MAX_POINTS + 1],
            scratch,
            reduced: vec![M::ZERO; remaining * remaining],
            on_event: &mut on_event,
            cancelled: false,
        };
        for &(start, weight) in &orbits {
            counter.visit(M::bit(start), start, 1, weight);
            if counter.cancelled {
                return Ok(());
            }
        }
        if counter.cancelled {
            return Ok(());
        }
        if counter.cap < length {
            let _ = (counter.on_event)(DpEvent::Overflow);
            return Ok(());
        }
        if (counter.on_event)(DpEvent::LengthDone {
            length,
            count: counter.counts[length],
        })
        .is_break()
        {
            return Ok(());
        }
    }
    Ok(())
}

struct PrefixCounter<'a, M, F> {
    n: usize,
    blocks: &'a [M],
    full_mask: M,
    prefix_length: usize,
    tail_length: usize,
    cap: usize,
    counts: [u128; MAX_POINTS + 1],
    scratch: DpScratch,
    reduced: Vec<M>,
    on_event: F,
    cancelled: bool,
}

impl<M: Mask, F: FnMut(DpEvent) -> ControlFlow<()>> PrefixCounter<'_, M, F> {
    fn visit(&mut self, visited: M, last: usize, length: usize, weight: u128) {
        if self.cancelled || length >= self.cap {
            return;
        }
        if (self.on_event)(DpEvent::Mask).is_break() {
            self.cancelled = true;
            return;
        }
        if length == self.prefix_length {
            self.continue_prefix(visited, last, weight);
            return;
        }
        let mut free = self.full_mask & !visited;
        while free != M::ZERO && !self.cancelled && length < self.cap {
            let bit = free & free.wrapping_neg();
            free ^= bit;
            let next = bit.trailing_zeros() as usize;
            if self.blocks[last * self.n + next] & !visited == M::ZERO {
                let next_length = length + 1;
                if let Some(count) = self.counts[next_length].checked_add(weight) {
                    self.counts[next_length] = count;
                    self.visit(visited | bit, next, next_length, weight);
                } else {
                    self.cap = length;
                }
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
            let length = self.prefix_length + 1;
            if let Some(count) = degree
                .checked_mul(weight)
                .and_then(|value| self.counts[length].checked_add(value))
            {
                self.counts[length] = count;
            } else {
                self.cap = length - 1;
            }
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
        let mut last_local = 0;
        let prefix = self.prefix_length;
        let counts = &mut self.counts;
        let cap = &mut self.cap;
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
                        last_local = length;
                        if length > 0 {
                            let total_length = length + prefix;
                            if let Some(total) = count
                                .checked_mul(weight)
                                .and_then(|value| counts[total_length].checked_add(value))
                            {
                                counts[total_length] = total;
                            } else {
                                *cap = total_length - 1;
                            }
                        }
                    }
                    DpEvent::Overflow => *cap = (*cap).min(prefix + last_local),
                    DpEvent::Mask => {
                        if on_event(DpEvent::Mask).is_break() {
                            *cancelled = true;
                        }
                    }
                }
                if *cancelled || prefix + last_local >= *cap {
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
