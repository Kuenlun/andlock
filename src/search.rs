// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Exact counting by prefix partitions when a complete layer exceeds the budget.

use std::collections::TryReserveError;
use std::ops::ControlFlow;

use num_bigint::BigUint;

use crate::counter::{
    DpEvent, DpScratch, count_patterns_dp, count_patterns_filtered, count_patterns_seeded,
    dp_table_bytes, effective_max_length, future_visits_unrestricted, is_unconstrained,
    validate_visits,
};
use crate::grid::GridDefinition;
use crate::mask::{MAX_POINTS, Mask};
use crate::numeric::{CountEvent, GlobalCount, count_unconstrained, local_counts_fit};
use crate::symmetry::{starting_orbits, starting_orbits_filtered};

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
    on_event: F,
) -> Result<(), TryReserveError> {
    count_with_filters(grid, blocks, max_length, budget, None, on_event)
}

/// Count patterns with a permitted node set for each successive visit.
///
/// Each finalized length uses only its corresponding prefix of `allowed`.
/// Deterministic prefixes and impossible suffixes are resolved before allocating
/// counting tables. The budget and event semantics match [`count_patterns_bounded`].
///
/// # Errors
/// Returns an allocation error if the selected counting layers cannot be allocated.
///
/// # Panics
/// Panics for an invalid grid, block matrix, visit mask, or requested length.
pub fn count_patterns_bounded_filtered<
    M: Mask,
    C: GlobalCount,
    F: FnMut(CountEvent<C>) -> ControlFlow<()>,
>(
    grid: &GridDefinition,
    blocks: &[M],
    max_length: usize,
    budget: u64,
    allowed: &[M],
    on_event: F,
) -> Result<(), TryReserveError> {
    validate_visits(grid.node_count(), max_length, allowed);
    count_with_filters(grid, blocks, max_length, budget, Some(allowed), on_event)
}

fn count_with_filters<M: Mask, C: GlobalCount, F: FnMut(CountEvent<C>) -> ControlFlow<()>>(
    grid: &GridDefinition,
    blocks: &[M],
    max_length: usize,
    budget: u64,
    allowed: Option<&[M]>,
    mut on_event: F,
) -> Result<(), TryReserveError> {
    let n = grid.node_count();
    assert!(n <= M::MAX_POINTS && max_length <= n);
    assert_eq!(blocks.len(), n * n);
    let active = allowed.map_or(max_length, |allowed| {
        fixed_prefix(n, blocks, max_length, allowed).active_length
    });
    count_active(grid, blocks, active, budget, allowed, &mut |event| {
        if let CountEvent::LengthDone { length, count } = event {
            on_event(CountEvent::LengthDone { length, count })?;
            if length == active {
                for length in active + 1..=max_length {
                    on_event(CountEvent::LengthDone {
                        length,
                        count: C::default(),
                    })?;
                }
            }
            ControlFlow::Continue(())
        } else {
            on_event(event)
        }
    })
}

struct FixedPrefix<M> {
    visited: M,
    last: Option<usize>,
    length: usize,
    active_length: usize,
}

fn fixed_prefix<M: Mask>(
    n: usize,
    blocks: &[M],
    max_length: usize,
    allowed: &[M],
) -> FixedPrefix<M> {
    let mut prefix = FixedPrefix {
        visited: M::ZERO,
        last: None,
        length: 0,
        active_length: allowed
            .iter()
            .take(max_length)
            .position(|&mask| mask == M::ZERO)
            .unwrap_or(max_length),
    };
    for &allowed in allowed.iter().take(prefix.active_length) {
        let mut mask = allowed & !prefix.visited;
        if let Some(last) = prefix.last {
            let mut candidates = mask;
            while candidates != M::ZERO {
                let bit = candidates & candidates.wrapping_neg();
                candidates ^= bit;
                if blocks[last * n + bit.trailing_zeros() as usize] & !prefix.visited != M::ZERO {
                    mask ^= bit;
                }
            }
        }
        if mask == M::ZERO {
            prefix.active_length = prefix.length;
            break;
        }
        if mask.count_ones() != 1 {
            break;
        }
        prefix.visited |= mask;
        prefix.last = Some(mask.trailing_zeros() as usize);
        prefix.length += 1;
    }
    prefix
}

fn count_active<M: Mask, C: GlobalCount, F: FnMut(CountEvent<C>) -> ControlFlow<()>>(
    grid: &GridDefinition,
    blocks: &[M],
    max_length: usize,
    budget: u64,
    allowed: Option<&[M]>,
    on_event: &mut F,
) -> Result<(), TryReserveError> {
    let n = grid.node_count();
    assert!(n <= M::MAX_POINTS && max_length <= n);
    assert_eq!(blocks.len(), n * n);
    let prefix = allowed.map(|allowed| fixed_prefix(n, blocks, max_length, allowed));
    let forced = prefix.as_ref().map_or(0, |prefix| prefix.length);
    let direct_length = if let Some((allowed, prefix)) = allowed
        .zip(prefix.as_ref())
        .filter(|(_, prefix)| prefix.length > 0)
    {
        for length in 0..=forced {
            if on_event(CountEvent::LengthDone {
                length,
                count: C::from(1),
            })
            .is_break()
            {
                return Ok(());
            }
        }
        if forced == max_length {
            return Ok(());
        }
        if let Some(starts) = free_tail_starts(n, blocks, max_length, allowed, prefix) {
            count_unconstrained(
                n - forced,
                max_length - forced,
                starts,
                &mut |event| match event {
                    CountEvent::LengthDone { length: 0, .. } => ControlFlow::Continue(()),
                    CountEvent::LengthDone { length, count } => on_event(CountEvent::LengthDone {
                        length: length + forced,
                        count,
                    }),
                    other => on_event(other),
                },
            );
            return Ok(());
        }
        forced
    } else {
        let Some(length) = count_direct(n, blocks, max_length, budget, allowed, on_event)? else {
            return Ok(());
        };
        length
    };
    let orbits = allowed.map_or_else(
        || starting_orbits(grid, blocks),
        |allowed| starting_orbits_filtered(grid, blocks, &allowed[..max_length]),
    );
    // Finish each requested length before starting the next. Lower layers are
    // repeated, but interruption preserves every previously completed count.
    for length in direct_length + 1..=max_length {
        let mut plan = count_plan_with::<C>(n, length, budget);
        plan.prefix_length = plan.prefix_length.max(forced);
        let remaining = n - plan.prefix_length;
        let tail_length = length - plan.prefix_length;
        let scratch = DpScratch::allocate_constrained(remaining, tail_length)?;
        let mut counter = PrefixCounter {
            n,
            blocks,
            allowed,
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
            on_event: &mut *on_event,
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

/// Return the number of legal first moves when every later move is unrestricted.
fn free_tail_starts<M: Mask>(
    n: usize,
    blocks: &[M],
    max_length: usize,
    allowed: &[M],
    prefix: &FixedPrefix<M>,
) -> Option<u128> {
    let free = M::low_bits(n) & !prefix.visited;
    if allowed[prefix.length + 1..max_length]
        .iter()
        .any(|&mask| mask & free != free)
    {
        return None;
    }
    let mut origins = free;
    let mut starts = 0u128;
    while origins != M::ZERO {
        let bit = origins & origins.wrapping_neg();
        origins ^= bit;
        let origin = bit.trailing_zeros() as usize;
        let mut targets = free;
        while targets != M::ZERO {
            let target = targets & targets.wrapping_neg();
            targets ^= target;
            if blocks[origin * n + target.trailing_zeros() as usize] & !prefix.visited != M::ZERO {
                return None;
            }
        }
        if bit & allowed[prefix.length] != M::ZERO
            && prefix
                .last
                .is_none_or(|last| blocks[last * n + origin] & !prefix.visited == M::ZERO)
        {
            starts += 1;
        }
    }
    Some(starts)
}

/// Peak counting-table storage after resolving deterministic and impossible visits.
///
/// # Panics
/// Panics for an invalid block matrix, visit mask, or requested length.
#[must_use]
pub fn filtered_table_bytes<M: Mask, C: GlobalCount>(
    n: usize,
    blocks: &[M],
    max_length: usize,
    budget: u64,
    allowed: &[M],
) -> u64 {
    validate_visits(n, max_length, allowed);
    assert_eq!(blocks.len(), n * n);
    let prefix = fixed_prefix(n, blocks, max_length, allowed);
    let active = prefix.active_length;
    if prefix.length == active || free_tail_starts(n, blocks, active, allowed, &prefix).is_some() {
        return 0;
    }
    (prefix.length + 1..=active)
        .map(|length| {
            let plan = count_plan_with::<C>(n, length, budget);
            let fixed = plan.prefix_length.max(prefix.length);
            dp_table_bytes(n - fixed, length - fixed)
        })
        .max()
        .unwrap_or(0)
}

/// Return the last direct length when partitioned continuations are still needed.
fn count_direct<M: Mask, C: GlobalCount, F: FnMut(CountEvent<C>) -> ControlFlow<()>>(
    n: usize,
    blocks: &[M],
    max_length: usize,
    budget: u64,
    allowed: Option<&[M]>,
    on_event: &mut F,
) -> Result<Option<usize>, TryReserveError> {
    if is_unconstrained(blocks)
        && allowed.is_none_or(|allowed| future_visits_unrestricted(n, max_length, allowed))
    {
        let starts = allowed.map_or(n as u128, |allowed| {
            allowed
                .first()
                .map_or(0, |mask| u128::from(mask.count_ones()))
        });
        count_unconstrained(n, max_length, starts, on_event);
        return Ok(None);
    }
    let mut direct_length = effective_max_length(n, max_length, budget);
    if C::ARBITRARY_PRECISION {
        while !local_counts_fit(n, direct_length) {
            direct_length -= 1;
        }
    }
    let mut scratch = allowed.map_or_else(
        || DpScratch::allocate(n, blocks, direct_length),
        |allowed| DpScratch::allocate_filtered(n, blocks, direct_length, allowed),
    )?;
    if direct_length == max_length {
        let forward = |event: DpEvent| on_event(event.into());
        if let Some(allowed) = allowed {
            count_patterns_filtered(&mut scratch, n, blocks, max_length, allowed, forward);
        } else {
            count_patterns_dp(&mut scratch, n, blocks, max_length, forward);
        }
        return Ok(None);
    }
    let mut stopped = false;
    let forward = |event| {
        stopped |= matches!(event, DpEvent::Overflow);
        let flow = on_event(event.into());
        stopped |= flow.is_break();
        flow
    };
    if let Some(allowed) = allowed {
        count_patterns_filtered(&mut scratch, n, blocks, direct_length, allowed, forward);
    } else {
        count_patterns_dp(&mut scratch, n, blocks, direct_length, forward);
    }
    Ok((!stopped && direct_length < max_length).then_some(direct_length))
}

struct PrefixCounter<'a, M, C, F> {
    n: usize,
    blocks: &'a [M],
    allowed: Option<&'a [M]>,
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
        let mut free = self
            .allowed
            .map_or(self.full_mask, |allowed| allowed[length])
            & !visited;
        while free != M::ZERO && !self.cancelled && !self.overflow {
            let bit = free & free.wrapping_neg();
            free ^= bit;
            let next = bit.trailing_zeros() as usize;
            if self.blocks[last * self.n + next] & !visited == M::ZERO {
                self.visit(visited | bit, next, length + 1, weight);
            }
        }
    }

    fn legal_degree(&self, visited: M, last: usize) -> u128 {
        let mut free = self
            .allowed
            .map_or(self.full_mask, |allowed| allowed[self.prefix_length])
            & !visited;
        let mut degree = 0;
        while free != M::ZERO {
            let bit = free & free.wrapping_neg();
            free ^= bit;
            let next = bit.trailing_zeros() as usize;
            if self.blocks[last * self.n + next] & !visited == M::ZERO {
                degree += 1;
            }
        }
        degree
    }

    fn continue_prefix(&mut self, visited: M, last: usize, weight: u128) {
        if self.tail_length == 1 {
            self.overflow = !self
                .count
                .add_scaled(self.legal_degree(visited, last), weight);
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
        let mut reduced_allowed = [M::ZERO; MAX_POINTS];
        if let Some(allowed) = self.allowed {
            for (index, &mask) in allowed[self.prefix_length..self.prefix_length + self.tail_length]
                .iter()
                .enumerate()
            {
                for (mapped, &node) in nodes[..remaining].iter().enumerate() {
                    if mask & M::bit(node) != M::ZERO {
                        reduced_allowed[index] |= M::bit(mapped);
                    }
                }
            }
            reduced_allowed[0] = reduced_allowed[0] & starts;
            starts = reduced_allowed[0];
        }
        if starts == M::ZERO {
            return;
        }
        let tail_length = self.tail_length;
        let total = &mut self.count;
        let overflow = &mut self.overflow;
        let cancelled = &mut self.cancelled;
        let on_event = &mut self.on_event;
        let forward = |event| {
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
        };
        if self.allowed.is_some() {
            count_patterns_filtered(
                &mut self.scratch,
                remaining,
                &self.reduced,
                tail_length,
                &reduced_allowed[..tail_length],
                forward,
            );
        } else {
            count_patterns_seeded(
                &mut self.scratch,
                remaining,
                &self.reduced,
                tail_length,
                starts,
                forward,
            );
        }
    }
}

#[cfg(test)]
mod tests;
