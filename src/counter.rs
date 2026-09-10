// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use std::ops::ControlFlow;

use crate::mask::{self, Mask};
use crate::numeric::count_unconstrained;

/// Progress event emitted by [`count_patterns_dp`].
pub enum DpEvent {
    /// One outer-loop mask has been processed.
    Mask,
    /// `counts[length]` has received its last contribution and is now final.
    LengthDone { length: usize, count: u128 },
    /// The next count would not fit in `u128`. The run stops at the last
    /// [`DpEvent::LengthDone`] so no inexact value is ever emitted.
    Overflow,
}

/// `true` when no move is ever blocked — every entry of `blocks` is zero —
/// so the count has a closed form and the DP allocates nothing.
#[must_use]
pub fn is_unconstrained<M: Mask>(blocks: &[M]) -> bool {
    blocks.iter().all(|&b| b == M::ZERO)
}

/// Exact `C(n, k)` in `u128`, saturating to `u128::MAX` on overflow.
fn binomial(n: usize, k: usize) -> u128 {
    if k > n {
        return 0;
    }
    let k = if k * 2 > n { n - k } else { k };
    let mut result: u128 = 1;
    for i in 0..k {
        let Some(m) = result.checked_mul((n - i) as u128) else {
            return u128::MAX;
        };
        result = m / (i + 1) as u128;
    }
    result
}

/// A state fixes the visited set and endpoint, leaving at most `(p - 1)!`
/// orderings. Only layers below `max_length` are stored. Above 34! the bound
/// no longer fits u128, but checked per-length totals still bound every cell.
const fn cell_bytes(p: usize) -> usize {
    match p {
        0..=6 => 1,
        7..=9 => 2,
        10..=13 => 4,
        14..=21 => 8,
        _ => 16,
    }
}

/// Each parity buffer holds only its own layers, with each layer using the
/// narrowest cells that fit its per-state bound.
fn layer_capacities(n: usize, max_length: usize) -> [u128; 2] {
    let mut capacities = [0; 2];
    for p in 1..max_length.min(n) {
        let bytes = binomial(n, p)
            .saturating_mul(p as u128)
            .saturating_mul(cell_bytes(p) as u128);
        let parity = (p - 1) % 2;
        capacities[parity] = capacities[parity].max(bytes);
    }
    capacities
}

/// Bytes reserved for constrained DP layers.
///
/// Each parity buffer is sized to
/// the largest byte footprint of the layers it stores. Unconstrained runs
/// allocate nothing regardless of this estimate.
/// Saturates to `u64::MAX` on overflow.
#[must_use]
pub fn dp_table_bytes(n: usize, max_length: usize) -> u64 {
    let [odd, even] = layer_capacities(n, max_length);
    u64::try_from(odd.saturating_add(even)).unwrap_or(u64::MAX)
}

/// Largest `max_length <= requested` whose [`dp_table_bytes`] fits within
/// `budget_bytes`.
///
/// Returns `0` when even length 1 does not fit, in which case the resulting
/// run still emits the trivial `counts[0] = 1`.
///
/// `dp_table_bytes` is non-decreasing in `max_length`, so the fit predicate
/// is monotone and the threshold is located via binary search.
#[must_use]
pub fn effective_max_length(n: usize, requested: usize, budget_bytes: u64) -> usize {
    let cap = requested.min(n);
    let (mut lo, mut hi) = (1, cap);
    let mut best = 0;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        if dp_table_bytes(n, mid) <= budget_bytes {
            best = mid;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    best
}

/// Working set [`count_patterns_dp`] needs to run. Allocation failure is
/// hoisted into [`DpScratch::allocate`] so the DP body itself is infallible.
pub struct DpScratch {
    buf: Vec<u8>,
    split: usize,
}

impl DpScratch {
    /// Allocates nothing for the closed-form fast path (`max_length < 2` or
    /// all-zero `blocks`).
    ///
    /// # Errors
    /// Returns [`std::collections::TryReserveError`] when the allocator
    /// cannot satisfy the request, so the user can lower `--max-length` or
    /// set `--memory-limit`.
    pub fn allocate<M: Mask>(
        n: usize,
        blocks: &[M],
        max_length: usize,
    ) -> Result<Self, std::collections::TryReserveError> {
        if max_length < 2 || is_unconstrained(blocks) {
            return Ok(Self {
                buf: Vec::new(),
                split: 0,
            });
        }
        Self::allocate_constrained(n, max_length)
    }

    /// Allocates scratch for [`count_patterns_filtered`]. Restrictions after
    /// the first visit can require layers even when every geometric move is legal.
    ///
    /// # Errors
    /// Returns an allocation error when the required layers cannot be reserved.
    ///
    /// # Panics
    /// Panics for invalid visit masks, too many visits, or an unsupported mask width.
    pub fn allocate_filtered<M: Mask>(
        n: usize,
        blocks: &[M],
        max_length: usize,
        allowed: &[M],
    ) -> Result<Self, std::collections::TryReserveError> {
        validate_visits(n, max_length, allowed);
        if max_length == 0 || allowed[0] == M::ZERO {
            return Self::allocate(n, blocks, 0);
        }
        if future_visits_unrestricted(n, max_length, allowed) {
            Self::allocate(n, blocks, max_length)
        } else {
            Self::allocate_constrained(n, max_length)
        }
    }

    pub(crate) fn allocate_constrained(
        n: usize,
        max_length: usize,
    ) -> Result<Self, std::collections::TryReserveError> {
        let [odd, even] = layer_capacities(n, max_length);
        let len = usize::try_from(odd.saturating_add(even)).unwrap_or(usize::MAX);
        let split = usize::try_from(odd).unwrap_or(usize::MAX);
        let mut buf = Vec::new();
        buf.try_reserve_exact(len)?;
        buf.resize(len, 0);
        Ok(Self { buf, split })
    }

    fn split_mut(&mut self) -> (&mut [u8], &mut [u8]) {
        self.buf.split_at_mut(self.split)
    }
}

fn read_cell<const BYTES: usize>(buf: &[u8], index: usize) -> u128 {
    let offset = index * BYTES;
    let mut bytes = [0; 16];
    bytes[..BYTES].copy_from_slice(&buf[offset..offset + BYTES]);
    u128::from_le_bytes(bytes)
}

fn add_cell<const BYTES: usize>(buf: &mut [u8], index: usize, ways: u128) {
    let value = read_cell::<BYTES>(buf, index) + ways;
    let offset = index * BYTES;
    // The layer's factorial bound fits BYTES. For 16-byte cells the checked
    // length total bounds the sum. Discarded high bytes are therefore zero.
    buf[offset..offset + BYTES].copy_from_slice(&value.to_le_bytes()[..BYTES]);
}

type ReadCell = fn(&[u8], usize) -> u128;

const fn cell_reader(bytes: usize) -> ReadCell {
    match bytes {
        1 => read_cell::<1>,
        2 => read_cell::<2>,
        4 => read_cell::<4>,
        8 => read_cell::<8>,
        16 => read_cell::<16>,
        _ => unreachable!(),
    }
}

/// Number of [`DpEvent::Mask`] events the constrained DP will fire,
/// `sum_{p=1..max_length-1} C(n, p)`. Zero on the closed-form fast path.
#[must_use]
pub fn dp_mask_ticks(n: usize, max_length: usize) -> u64 {
    let max_length = max_length.min(n);
    if n == 0 || max_length < 2 {
        return 0;
    }
    let mut total: u128 = 0;
    for p in 1..max_length {
        total = total.saturating_add(binomial(n, p));
    }
    u64::try_from(total).unwrap_or(u64::MAX)
}

/// Pascal's triangle `[i][j] = C(i, j)`, saturating-add. Sized for every `n`
/// up to `mask::MAX_POINTS` plus the margin the inner loop needs for
/// `BINOM[next][next_off + 1]` and `BINOM[bit_pos[j]][j + 2]`.
const SLOTS: usize = mask::MAX_POINTS + 3;

static BINOM: [[usize; SLOTS]; SLOTS] = {
    let mut t = [[0usize; SLOTS]; SLOTS];
    let mut i = 0;
    while i < SLOTS {
        t[i][0] = 1;
        let mut j = 1;
        while j <= i {
            t[i][j] = t[i - 1][j - 1].saturating_add(t[i - 1][j]);
            j += 1;
        }
        i += 1;
    }
    t
};

/// Counts every valid pattern via layered bitmask dynamic programming.
///
/// `blocks[i * n + j]` is the bitmask of nodes that must already be visited
/// before the move `i -> j` is legal (see [`crate::grid::compute_blocks`]).
/// Each finalised length is delivered through [`DpEvent::LengthDone`]. The
/// caller collects those events to assemble the per-length table.
///
/// `on_event` may return [`ControlFlow::Break`] to abort the run. Lengths
/// already emitted stay valid, and no further events fire.
///
/// Two popcount layers are alive at any time (source `p`, destination
/// `p + 1`), carved out of `scratch` and ping-ponged in place. Each mask of
/// popcount `p` packs `p` cells, one per valid endpoint. Cell widths grow
/// from 1 to 16 bytes with the per-state bound `(p - 1)!`. Layer-local
/// indices are reconstructed via colex-rank prefix/suffix sums instead of a
/// `2^n` lookup table.
///
/// # Complexity
/// `O(N^2 * sum_{k < max_length} C(N, k))` extension work.
///
/// # Panics
/// `n > M::MAX_POINTS`, `blocks.len() != n * n`, `max_length > n`, or
/// `scratch` sized for a different `(n, max_length)` than requested.
pub fn count_patterns_dp<M: Mask, F: FnMut(DpEvent) -> ControlFlow<()>>(
    scratch: &mut DpScratch,
    n: usize,
    blocks: &[M],
    max_length: usize,
    on_event: F,
) {
    assert!(
        n <= M::MAX_POINTS,
        "N={n} exceeds the maximum of {}",
        M::MAX_POINTS
    );
    count_patterns_seeded(scratch, n, blocks, max_length, M::low_bits(n), on_event);
}

/// Counts paths that visit a node in `allowed[p - 1]` at position `p`.
///
/// The empty pattern counts once. Each finalized length applies only its own
/// prefix of the filters. Use [`DpScratch::allocate_filtered`] for allocation.
/// Events and cancellation follow [`count_patterns_dp`].
///
/// # Panics
/// Panics for an invalid block matrix or scratch, out-of-grid visit masks,
/// more visits than nodes, or `max_length > allowed.len()`.
/// The chosen mask width must represent all nodes.
pub fn count_patterns_filtered<M: Mask, F: FnMut(DpEvent) -> ControlFlow<()>>(
    scratch: &mut DpScratch,
    n: usize,
    blocks: &[M],
    max_length: usize,
    allowed: &[M],
    on_event: F,
) {
    validate_visits(n, max_length, allowed);
    let starts = allowed.first().copied().unwrap_or(M::ZERO);
    count_patterns_with_visits(
        scratch,
        n,
        blocks,
        max_length,
        starts,
        Some(allowed),
        on_event,
    );
}

pub(crate) fn validate_visits<M: Mask>(n: usize, max_length: usize, allowed: &[M]) {
    assert!(n <= M::MAX_POINTS, "too many nodes for mask width");
    assert!(allowed.len() <= n, "more visits than nodes");
    assert!(
        max_length <= allowed.len(),
        "maximum length exceeds visit filters"
    );
    let full = M::low_bits(n);
    assert!(
        allowed.iter().all(|&nodes| nodes & full == nodes),
        "visit node outside grid"
    );
}

pub(crate) fn future_visits_unrestricted<M: Mask>(
    n: usize,
    max_length: usize,
    allowed: &[M],
) -> bool {
    let full = M::low_bits(n);
    allowed
        .iter()
        .take(max_length)
        .skip(1)
        .all(|&nodes| nodes == full)
}

/// Counts paths whose first node belongs to `starts`. Singleton weights are
/// either zero or one, preserving the same factorial bound as the full DP.
pub(crate) fn count_patterns_seeded<M: Mask, F: FnMut(DpEvent) -> ControlFlow<()>>(
    scratch: &mut DpScratch,
    n: usize,
    blocks: &[M],
    max_length: usize,
    starts: M,
    on_event: F,
) {
    count_patterns_with_visits(scratch, n, blocks, max_length, starts, None, on_event);
}

fn count_patterns_with_visits<M: Mask, F: FnMut(DpEvent) -> ControlFlow<()>>(
    scratch: &mut DpScratch,
    n: usize,
    blocks: &[M],
    max_length: usize,
    starts: M,
    allowed: Option<&[M]>,
    mut on_event: F,
) {
    assert!(n <= M::MAX_POINTS, "too many nodes for mask width");
    assert!(starts & M::low_bits(n) == starts, "start node outside grid");
    assert_eq!(blocks.len(), n * n, "blocks matrix must be n × n");
    assert!(
        max_length <= n,
        "max_length={max_length} must not exceed n={n}"
    );

    // With every move legal, counts[k] = |starts| * P(n - 1, k - 1).
    // Stream each exact length, stopping before the first product overflow.
    if starts == M::ZERO
        || (is_unconstrained(blocks)
            && allowed.is_none_or(|visits| future_visits_unrestricted(n, max_length, visits)))
    {
        count_unconstrained::<u128, _>(n, max_length, u128::from(starts.count_ones()), |event| {
            on_event(event.into())
        });
        return;
    }

    if on_event(DpEvent::LengthDone {
        length: 0,
        count: 1,
    })
    .is_break()
    {
        return;
    }
    if max_length == 0 {
        return;
    }

    if on_event(DpEvent::LengthDone {
        length: 1,
        count: u128::from(starts.count_ones()),
    })
    .is_break()
    {
        return;
    }
    if max_length < 2 {
        return;
    }

    assert_eq!(
        [
            scratch.split as u128,
            (scratch.buf.len() - scratch.split) as u128
        ],
        layer_capacities(n, max_length),
        "scratch sized for a different (n, max_length) run"
    );
    let (mut dp_curr, mut dp_next) = scratch.split_mut();

    // Every allowed singleton has one ordering. Reset all slots for reuse.
    for (index, cell) in dp_curr[..n].iter_mut().enumerate() {
        *cell = u8::from(starts & M::bit(index) != M::ZERO);
    }

    for p in 1..max_length {
        let next_p = p + 1;
        let next_bytes = if next_p < max_length {
            cell_bytes(next_p)
        } else {
            0
        };
        let layer = Layer {
            n,
            p,
            blocks,
            allowed: allowed.map_or_else(|| M::low_bits(n), |visits| visits[p]),
            current: dp_curr,
            next: dp_next,
        };
        let read_current = cell_reader(cell_bytes(p));
        let result = match next_bytes {
            0 => layer.count::<0, _>(read_current, &mut on_event),
            1 => layer.count::<1, _>(read_current, &mut on_event),
            2 => layer.count::<2, _>(read_current, &mut on_event),
            4 => layer.count::<4, _>(read_current, &mut on_event),
            8 => layer.count::<8, _>(read_current, &mut on_event),
            16 => layer.count::<16, _>(read_current, &mut on_event),
            _ => unreachable!(),
        };
        match result {
            Ok(count) => {
                if on_event(DpEvent::LengthDone {
                    length: next_p,
                    count,
                })
                .is_break()
                {
                    return;
                }
            }
            Err(LayerStop::Cancelled) => return,
            Err(LayerStop::Overflow) => {
                let _ = on_event(DpEvent::Overflow);
                return;
            }
        }
        std::mem::swap(&mut dp_curr, &mut dp_next);
    }
}

enum LayerStop {
    Cancelled,
    Overflow,
}

struct Layer<'a, M> {
    n: usize,
    p: usize,
    blocks: &'a [M],
    allowed: M,
    current: &'a [u8],
    next: &'a mut [u8],
}

/// Cache bit positions and the prefix/suffix sums used to rank a successor mask.
#[inline]
fn colex_sums<M: Mask>(
    mask: M,
    bit_pos: &mut [u32; SLOTS],
    prefix_sum: &mut [usize; SLOTS],
    suffix_sum: &mut [usize; SLOTS],
) {
    let mut tmp = mask;
    let mut i = 0usize;
    while tmp != M::ZERO {
        let bit = tmp & tmp.wrapping_neg();
        let pos = bit.trailing_zeros();
        bit_pos[i] = pos;
        prefix_sum[i + 1] = prefix_sum[i] + BINOM[pos as usize][i + 1];
        tmp ^= bit;
        i += 1;
    }
    suffix_sum[i] = 0;
    for j in (0..i).rev() {
        suffix_sum[j] = suffix_sum[j + 1] + BINOM[bit_pos[j] as usize][j + 2];
    }
}

impl<M: Mask> Layer<'_, M> {
    fn count<const NEXT_BYTES: usize, F: FnMut(DpEvent) -> ControlFlow<()>>(
        self,
        read_current: ReadCell,
        on_event: &mut F,
    ) -> Result<u128, LayerStop> {
        let Self {
            n,
            p,
            blocks,
            allowed,
            current,
            next: dp_next,
        } = self;
        let next_p = p + 1;
        let mut prefix_sum = [0usize; SLOTS];
        let mut suffix_sum = [0usize; SLOTS];
        let mut bit_pos = [0u32; SLOTS];
        let mut free_meta = [(0usize, 0usize); SLOTS];
        if NEXT_BYTES != 0 {
            let next_len = usize::try_from(
                binomial(n, next_p)
                    .saturating_mul(next_p as u128)
                    .saturating_mul(NEXT_BYTES as u128),
            )
            .unwrap_or(usize::MAX);
            dp_next[..next_len].fill(0);
        }

        let mut count_next: u128 = 0;
        let mut idx_curr: usize = 0;
        let mut mask: M = M::low_bits(p);
        let last: M = M::low_bits(p) << (n - p);
        loop {
            if on_event(DpEvent::Mask).is_break() {
                return Err(LayerStop::Cancelled);
            }
            let base_curr = idx_curr * p;

            if NEXT_BYTES != 0 {
                colex_sums(mask, &mut bit_pos, &mut prefix_sum, &mut suffix_sum);
            }

            // Hoist per-next colex arithmetic out of the endpoint loop.
            let mut nfree = 0usize;
            let mut free = !mask & allowed;
            while free != M::ZERO {
                let next_bit = free & free.wrapping_neg();
                free ^= next_bit;
                let next = next_bit.trailing_zeros() as usize;
                let dst_idx = if NEXT_BYTES != 0 {
                    let next_off = (mask & next_bit.wrapping_sub_one()).count_ones() as usize;
                    let idx_new =
                        prefix_sum[next_off] + BINOM[next][next_off + 1] + suffix_sum[next_off];
                    idx_new * next_p + next_off
                } else {
                    0
                };
                free_meta[nfree] = (next, dst_idx);
                nfree += 1;
            }
            let free_slice = &free_meta[..nfree];

            let mut end_off: usize = 0;
            let mut visited = mask;
            while visited != M::ZERO {
                let end_bit = visited & visited.wrapping_neg();
                visited ^= end_bit;
                let end = end_bit.trailing_zeros() as usize;
                let ways = read_current(current, base_curr + end_off);
                end_off += 1;
                if ways == 0 {
                    continue;
                }
                let row_start = end * n;
                if NEXT_BYTES == 0 {
                    // The final layer only needs the number of legal successors.
                    let degree = free_slice
                        .iter()
                        .filter(|&&(next, _)| {
                            let blockers = blocks[row_start + next];
                            mask & blockers == blockers
                        })
                        .count();
                    let contribution = ways
                        .checked_mul(degree as u128)
                        .ok_or(LayerStop::Overflow)?;
                    count_next = count_next
                        .checked_add(contribution)
                        .ok_or(LayerStop::Overflow)?;
                } else {
                    for &(next, dst_idx) in free_slice {
                        let blockers = blocks[row_start + next];
                        if mask & blockers == blockers {
                            count_next = count_next.checked_add(ways).ok_or(LayerStop::Overflow)?;
                            // The checked total bounds each stored cell. Narrow
                            // cells additionally fit the factorial layer bound.
                            add_cell::<NEXT_BYTES>(dp_next, dst_idx, ways);
                        }
                    }
                }
            }
            idx_curr += 1;
            if mask == last {
                break;
            }
            mask = mask.gosper_next();
        }
        Ok(count_next)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests;
