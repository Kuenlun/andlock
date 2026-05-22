// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use crate::mask::{self, Mask};

/// Progress event emitted by [`count_patterns_dp`].
pub enum DpEvent {
    /// One outer-loop mask has been processed.
    Mask,
    /// `counts[length]` has received its last contribution and is now final.
    LengthDone { length: usize, count: u128 },
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

/// Peak `u128`-slot footprint of a single popcount layer,
/// `max_{1<=p<max_length} C(n,p) * p`.
fn peak_layer_entries(n: usize, max_length: usize) -> u128 {
    (1..max_length)
        .map(|p| binomial(n, p).saturating_mul(p as u128))
        .max()
        .unwrap_or(0)
}

/// Bytes [`count_patterns_dp`] allocates for `(n, max_length)`: the
/// `counts` vector plus two ping-pong layer buffers sized to the peak
/// popcount-layer footprint. Saturates to `u64::MAX` on overflow.
#[must_use]
pub fn dp_table_bytes(n: usize, max_length: usize) -> u64 {
    let max_length = max_length.min(n);
    let counts_bytes = (max_length as u128).saturating_add(1).saturating_mul(16);
    let dp_bytes = peak_layer_entries(n, max_length).saturating_mul(32);
    u64::try_from(dp_bytes.saturating_add(counts_bytes)).unwrap_or(u64::MAX)
}

/// Largest `max_length <= requested` whose [`dp_table_bytes`] fits within
/// `budget_bytes`. Returns `0` when even length 1 does not fit; the
/// resulting run still emits the trivial `counts[0] = 1`.
#[must_use]
pub fn effective_max_length(n: usize, requested: usize, budget_bytes: u64) -> usize {
    let cap = requested.min(n);
    for l in (1..=cap).rev() {
        if dp_table_bytes(n, l) <= budget_bytes {
            return l;
        }
    }
    0
}

fn zeroed_buffer(len: usize) -> Result<Vec<u128>, std::collections::TryReserveError> {
    let mut buf: Vec<u128> = Vec::new();
    buf.try_reserve_exact(len)?;
    buf.resize(len, 0);
    Ok(buf)
}

/// Working set [`count_patterns_dp`] needs to run. Allocation failure is
/// hoisted into [`DpScratch::allocate`] so the DP body itself is infallible.
pub struct DpScratch {
    buf: Vec<u128>,
    half: usize,
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
        let half = if max_length < 2 || blocks.iter().all(|&b| b == M::ZERO) {
            0
        } else {
            dp_layer_capacity(n, max_length)
        };
        zeroed_buffer(half.saturating_mul(2)).map(|buf| Self { buf, half })
    }

    fn split_mut(&mut self) -> (&mut [u128], &mut [u128]) {
        self.buf.split_at_mut(self.half)
    }
}

/// Per-buffer entry count; saturates to `usize::MAX` so an overflowing layer
/// surfaces as an alloc failure rather than a silently-truncated buffer.
fn dp_layer_capacity(n: usize, l: usize) -> usize {
    usize::try_from(peak_layer_entries(n, l)).unwrap_or(usize::MAX)
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

/// Closed-form counts when every move is legal: the falling factorial
/// `P(n, k) = n * (n-1) * ... * (n-k+1)`.
fn count_unconstrained(n: usize, max_length: usize) -> Vec<u128> {
    let mut counts = vec![0u128; max_length + 1];
    counts[0] = 1;
    let mut perm: u128 = 1;
    for (k, slot) in counts.iter_mut().enumerate().skip(1) {
        perm *= (n - k + 1) as u128;
        *slot = perm;
    }
    counts
}

/// Counts every valid pattern via layered bitmask dynamic programming.
///
/// `blocks[i * n + j]` is the bitmask of nodes that must already be visited
/// before the move `i -> j` is legal (see [`crate::grid::compute_blocks`]).
/// Returns `counts[k] = patterns of length k` for `k in 0..=max_length`;
/// `counts[0] = 1` is the empty pattern.
///
/// Two popcount layers are alive at any time (source `p`, destination
/// `p + 1`), carved out of `scratch` and ping-ponged in place. Each mask of
/// popcount `p` packs `p` `u128` slots, one per valid endpoint; layer-local
/// indices are reconstructed via colex-rank prefix/suffix sums instead of a
/// `2^n` lookup table.
///
/// # Complexity
/// `O(N^2 * sum_{k < max_length} C(N, k))` extension work.
///
/// # Panics
/// `n > M::MAX_POINTS`, `blocks.len() != n * n`, `max_length > n`, or
/// `scratch` sized for a different `(n, max_length)` than requested.
#[allow(clippy::too_many_lines)]
pub fn count_patterns_dp<M: Mask, F: FnMut(DpEvent)>(
    scratch: &mut DpScratch,
    n: usize,
    blocks: &[M],
    max_length: usize,
    mut on_event: F,
) -> Vec<u128> {
    assert!(
        n <= M::MAX_POINTS,
        "N={n} exceeds the maximum of {}",
        M::MAX_POINTS
    );
    assert_eq!(blocks.len(), n * n, "blocks matrix must be n × n");
    assert!(
        max_length <= n,
        "max_length={max_length} must not exceed n={n}"
    );

    if blocks.iter().all(|&b| b == M::ZERO) {
        let counts = count_unconstrained(n, max_length);
        for (k, &c) in counts.iter().enumerate() {
            on_event(DpEvent::LengthDone {
                length: k,
                count: c,
            });
        }
        return counts;
    }

    let mut counts = vec![0u128; max_length + 1];
    counts[0] = 1;
    on_event(DpEvent::LengthDone {
        length: 0,
        count: 1,
    });
    if max_length == 0 {
        return counts;
    }

    counts[1] = n as u128;
    on_event(DpEvent::LengthDone {
        length: 1,
        count: counts[1],
    });
    if max_length < 2 {
        return counts;
    }

    assert_eq!(
        scratch.half,
        dp_layer_capacity(n, max_length),
        "scratch sized for a different (n, max_length) run"
    );
    let (mut dp_curr, mut dp_next) = scratch.split_mut();

    let full_mask: M = M::low_bits(n);

    // Popcount-1 layer: each of the n masks has exactly one endpoint, one way.
    for slot in &mut dp_curr[..n] {
        *slot = 1;
    }

    let mut prefix_sum = [0usize; SLOTS];
    let mut suffix_sum = [0usize; SLOTS];
    let mut bit_pos = [0u32; SLOTS];
    // (next, dst_idx) per free bit; reused across masks.
    let mut free_meta = [(0usize, 0usize); SLOTS];

    // Ascend through popcount classes so every subset is final before it is
    // read. Stops at max_length-1, the last layer that contributes to
    // counts[max_length].
    for p in 1..max_length {
        let next_p = p + 1;
        // At p == max_length-1 we still accumulate counts[max_length] but
        // skip dp_next writes; nothing would ever read them.
        let need_dp_next = next_p < max_length;
        let next_len = if need_dp_next {
            usize::try_from(binomial(n, next_p).saturating_mul(next_p as u128))
                .unwrap_or(usize::MAX)
        } else {
            0
        };

        if need_dp_next {
            for slot in &mut dp_next[..next_len] {
                *slot = 0;
            }
        }

        let mut idx_curr: usize = 0;
        let mut mask: M = M::low_bits(p);
        let last: M = M::low_bits(p) << (n - p);
        loop {
            on_event(DpEvent::Mask);
            let base_curr = idx_curr * p;

            if need_dp_next {
                // Cache mask bit positions and the colex-rank decomposition.
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
                suffix_sum[p] = 0;
                for j in (0..p).rev() {
                    suffix_sum[j] = suffix_sum[j + 1] + BINOM[bit_pos[j] as usize][j + 2];
                }
            }

            // Hoist per-`next` colex arithmetic out of the (end, next) loop.
            let mut nfree = 0usize;
            let mut free = !mask & full_mask;
            while free != M::ZERO {
                let next_bit = free & free.wrapping_neg();
                free ^= next_bit;
                let next = next_bit.trailing_zeros() as usize;
                let dst_idx = if need_dp_next {
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
                let ways = dp_curr[base_curr + end_off];
                end_off += 1;
                if ways == 0 {
                    continue;
                }
                let row_start = end * n;
                for &(next, dst_idx) in free_slice {
                    let blockers = blocks[row_start + next];
                    if mask & blockers == blockers {
                        counts[next_p] += ways;
                        if need_dp_next {
                            dp_next[dst_idx] += ways;
                        }
                    }
                }
            }
            idx_curr = idx_curr.wrapping_add(1);
            if mask == last {
                break;
            }
            mask = mask.gosper_next();
        }

        // counts[next_p] only takes contributions from popcount-p masks, so
        // it is final once the layer is done.
        on_event(DpEvent::LengthDone {
            length: next_p,
            count: counts[next_p],
        });
        std::mem::swap(&mut dp_curr, &mut dp_next);
    }

    counts
}
