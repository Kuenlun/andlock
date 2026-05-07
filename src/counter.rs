/*!
andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
Copyright (C) 2026  Juan Luis Leal Contreras (Kuenlun)

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use crate::mask::{self, Mask};

/// Progress event emitted by [`count_patterns_dp`] during execution.
///
/// A streaming caller uses `Mask` to advance a per-mask progress bar and
/// `LengthDone` to print the count of patterns of a given length as soon as it
/// is fully known — before the whole DP finishes.
pub enum DpEvent {
    /// One outer-loop mask has been processed. Fired exactly
    /// [`dp_mask_ticks(n, max_length)`](dp_mask_ticks) times during the
    /// constrained DP; not fired at all on the unconstrained fast path.
    Mask,
    /// `counts[length]` has received its last contribution and is now final.
    LengthDone { length: usize, count: u128 },
}

/// Exact binomial coefficient `C(n, k)`, returned as `u128`.
///
/// The intermediate `result * (n-i)` step can exceed `u128::MAX` for `n`
/// past ~127 even when the final value would fit; in that case the
/// routine saturates to `u128::MAX` rather than wrapping. Callers
/// (`dp_table_bytes`, `dp_mask_ticks`, `dp_layer_capacity`) only feed
/// the result through `saturating_mul` / `saturating_add` chains and
/// `u64::try_from(...).unwrap_or(u64::MAX)`, so a saturated binomial
/// flows through to a saturated byte count — exactly the "this run does
/// not fit" signal the memory clamp wants.
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

/// Returns the exact number of bytes [`count_patterns_dp`] allocates up
/// front for `n` nodes when called with `max_length`.
///
/// The layered DP keeps two ping-pong `Vec<u128>` buffers, each sized to the
/// largest single popcount layer it will ever hold. Layer `p` packs
/// `C(n, p)·p` `u128` slots — one per valid endpoint — with layer 1 always
/// equal to `n = C(n, 1)·1`. The peak entry count is therefore
/// `M = max_{p ∈ 1..max_length} C(n, p)·p` and the DP allocation is `2·M`
/// `u128` entries (= `32·M` bytes) on top of the `(max_length+1)·16`-byte
/// `counts` vector. The buffers are allocated once and reused via
/// [`std::mem::swap`]; there is no per-iteration allocation.
///
/// When `max_length < 2` the DP body exits before allocating any layer
/// buffer, so the result is just the `counts` vector (16 bytes per slot).
///
/// `max_length` is clamped to `n`; values above that are equivalent to `n`
/// (passing `max_length > n` to [`count_patterns_dp`] itself panics, so the
/// estimator simply normalises the input rather than mirroring the panic).
/// Uses saturating arithmetic so callers can pass any `n` and any
/// `max_length` without overflowing intermediate products.
#[must_use]
pub fn dp_table_bytes(n: usize, max_length: usize) -> u64 {
    let max_length = max_length.min(n);

    // counts vector: max_length+1 u128 slots, always allocated.
    let counts_bytes: u128 = (max_length as u128).saturating_add(1).saturating_mul(16);

    // No DP buffers when the algorithm exits before iterating popcount
    // layers (n == 0, max_length == 0, or max_length == 1 — the latter
    // hard-codes counts[1] = n without touching any DP layer).
    if n == 0 || max_length < 2 {
        return u64::try_from(counts_bytes).unwrap_or(u64::MAX);
    }

    // Peak single popcount layer, in u128 entries. Layer p has C(n, p)·p
    // entries; the loop bound `1..max_length` covers every layer the DP
    // visits as either source or destination. `binomial` already saturates
    // at u128::MAX when the intermediate product overflows, so passing any
    // `n` up to `mask::MAX_POINTS` is well-defined here.
    let mut peak_layer_entries: u128 = 0;
    for p in 1..max_length {
        let entries = binomial(n, p).saturating_mul(p as u128);
        if entries > peak_layer_entries {
            peak_layer_entries = entries;
        }
    }

    let dp_bytes = peak_layer_entries.saturating_mul(2).saturating_mul(16);
    let total = dp_bytes.saturating_add(counts_bytes);
    u64::try_from(total).unwrap_or(u64::MAX)
}

/// Returns the largest `max_length ≤ requested` whose peak allocation
/// (per [`dp_table_bytes`]) fits within `budget_bytes`.
///
/// `dp_table_bytes` is monotone non-decreasing in `max_length`, so the
/// search walks `requested..=0` and returns the first value that fits.
/// Always returns at most `requested.min(n)`. Returns `0` when even
/// `max_length = 1` does not fit; the resulting run still produces the
/// trivial `counts[0] = 1` and the caller can present that as a partial
/// result rather than aborting.
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

/// Allocates a `Vec<u128>` of exactly `len` zeroed entries without any
/// over-allocation, surfacing allocator failure as `Err` instead of aborting.
///
/// The DP scratch buffer goes through this helper so the request size
/// matches [`dp_layer_capacity`] exactly — the layered DP relies on knowing
/// the peak working set up front and has no use for a larger backing buffer.
fn zeroed_buffer(len: usize) -> Result<Vec<u128>, std::collections::TryReserveError> {
    let mut buf: Vec<u128> = Vec::new();
    buf.try_reserve_exact(len)?;
    buf.resize(len, 0);
    Ok(buf)
}

/// Pre-allocated working set [`count_patterns_dp`] needs to run.
///
/// The DP itself is infallible — every possible memory failure is hoisted
/// into [`DpScratch::allocate`], the single fallible step, so callers can
/// react to OOM up front and the algorithm never has to thread an error
/// through its inner loop. A scratch buffer can be reused across
/// consecutive runs that share the same `(n, blocks, max_length)` shape.
pub struct DpScratch {
    buf: Vec<u128>,
    half: usize,
}

impl DpScratch {
    /// Reserves the working set [`count_patterns_dp`] needs for a run of
    /// `(n, blocks, max_length)`. Allocates nothing when the DP body
    /// short-circuits (`max_length < 2`) or takes the unconstrained fast
    /// path (every block mask is zero).
    ///
    /// Generic over the [`Mask`] type so the same allocator works for
    /// every supported width; the choice of width doesn't change the
    /// scratch size, only the type of `blocks` accepted by the
    /// unconstrained-check shortcut.
    ///
    /// # Errors
    /// Returns the underlying [`std::collections::TryReserveError`] when
    /// the request cannot be satisfied. Surface the error to the user
    /// and let them lower `--max-length` or set `--memory-limit`.
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
        Self::with_layer_capacity(half)
    }

    /// Internal entry point that sizes the buffer directly from a
    /// per-layer capacity. Public callers go through [`Self::allocate`],
    /// which derives `half` from the run parameters.
    fn with_layer_capacity(half: usize) -> Result<Self, std::collections::TryReserveError> {
        zeroed_buffer(half.saturating_mul(2)).map(|buf| Self { buf, half })
    }

    fn split_mut(&mut self) -> (&mut [u128], &mut [u128]) {
        self.buf.split_at_mut(self.half)
    }
}

/// Computes the per-buffer entry count `M` for the ping-pong DP buffers at
/// `max_length = l`, namely `max_{p ∈ 1..l} C(n, p)·p`. Returns 0 when
/// `l < 2` (no DP buffer is needed in that case).
///
/// Arithmetic is performed in `u128` and saturated to `usize::MAX` at the
/// boundary: at the wider mask widths `binomial(n, p)` can exceed
/// `usize::MAX` while still fitting in `u128`, and a naive `as usize`
/// would truncate the high bits — yielding a small bogus capacity, an
/// undersized allocation, and out-of-bounds writes inside
/// [`process_layer`]. Saturating instead lets [`DpScratch::allocate`]
/// surface the request as a real allocator failure.
fn dp_layer_capacity(n: usize, l: usize) -> usize {
    if l < 2 {
        return 0;
    }
    let mut m: u128 = n as u128;
    for p in 2..l {
        let entries = binomial(n, p).saturating_mul(p as u128);
        if entries > m {
            m = entries;
        }
    }
    usize::try_from(m).unwrap_or(usize::MAX)
}

/// Returns the exact number of [`DpEvent::Mask`] events
/// [`count_patterns_dp`] will fire for `(n, max_length)` on a constrained
/// grid. The unconstrained fast path emits zero `Mask` events regardless.
///
/// Equal to `Σ_{p=1}^{max_length−1} C(n, p)` — one event per popcount-`p`
/// mask processed, summed across the popcount layers the DP actually visits
/// (which stops at `max_length − 1`, the last layer that can contribute to
/// `counts[max_length]`). Callers use this value to size a per-mask
/// progress bar that reaches exactly 100% when the DP returns.
///
/// `max_length` is clamped to `n`, mirroring [`dp_table_bytes`]. Returns
/// `0` for `n == 0` or `max_length < 2` (the DP body either exits early or
/// finalises every length without entering the popcount loop).
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

/// Pascal's triangle, indexed `[i][j] = C(i, j)`. Sized to cover every
/// `n ≤ mask::MAX_POINTS` (= 127) plus a margin for the highest index
/// reached in [`process_layer`] (which reads `BINOM[bit_pos][j + 2]`
/// with `bit_pos < n` and `j + 2 <= p + 1 <= max_length`).
///
/// Entries are stored as `usize` so the colex-rank prefix/suffix sums
/// inside the DP need no further conversion before indexing the
/// destination layer's `Vec`. Saturating addition guards the construction
/// against entries past the `usize` ceiling — those rows correspond to
/// popcounts whose DP layer would dwarf any physical memory and so are
/// unreachable in practice: any `(n, max_length)` whose
/// [`dp_layer_capacity`] exceeds `usize::MAX` is rejected by
/// [`DpScratch::allocate`] before [`process_layer`] runs, so a saturated
/// cell can never be read. The
/// `binom_reads_never_saturate_for_clampable_runs` test pins this
/// invariant by sweeping every `(n, max_length)` the dispatcher accepts.
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

/// Computes pattern counts when there are no visibility constraints.
///
/// When every `blocks[i * n + j] == 0` every move is unconditionally legal, so
/// the number of valid patterns of length k is the falling factorial
/// `P(n, k) = n! / (n-k)! = n × (n-1) × … × (n-k+1)`.
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
/// `blocks[i * n + j]` must hold the bitmask of nodes that must already be
/// visited before the move `i → j` is legal (see [`crate::grid::compute_blocks`]).
///
/// `max_length` bounds the pattern lengths considered: any prefix whose length
/// reaches `max_length` is not extended, so the exponential inner work for
/// longer prefixes is never performed.
///
/// Returns a `Vec<u128>` of length `max_length + 1` where `counts[k]` is the
/// number of valid patterns of exactly `k` nodes. `counts[0] = 1` (the empty
/// pattern).
///
/// `u128` is used because for n ≥ 21 the total count can exceed `u64::MAX`
/// (e.g. an unrestricted 21-node graph can produce 21! ≈ 5.1 × 10¹⁹ patterns,
/// while `u64::MAX` ≈ 1.8 × 10¹⁹).
///
/// # Mask width
/// Generic over [`Mask`] — the DP body, the unconstrained fast path,
/// and `process_layer` all monomorphise per impl, so `M = u32` retains
/// the original (byte-identical) hot path while `M = u64` and `M = u128`
/// extend the same logic to wider grids. The CLI's pipeline picks the
/// smallest sufficient width once per run; library callers who already
/// know `n` instantiate the function directly.
///
/// # Memory
/// Two adjacent popcount layers are alive at any time: the source layer
/// (popcount `p`, read) and the destination (popcount `p+1`, written). Both
/// live in the ping-pong slices carved out of `scratch` and sized to the
/// largest popcount layer this run will ever hold, swapped in place between
/// iterations — no per-iteration allocation occurs. Each mask of popcount
/// `p` packs only `p` `u128` slots (one per valid endpoint). Layer-local
/// indices are computed via a colex-rank formula instead of stored in a
/// `2ⁿ × u32` lookup table, which keeps the working set proportional to
/// the actual popcount layers rather than `2ⁿ`. The source layer is read
/// with an incrementing counter that mirrors Gosper order (= colex order
/// for fixed popcount); writes into the destination layer use precomputed
/// prefix/suffix sums to reconstruct the rank in O(1). See
/// [`dp_table_bytes`] for the exact byte count and
/// [`effective_max_length`] for clamping a requested cap to a memory
/// budget. Allocate `scratch` with [`DpScratch::allocate`] using the same
/// `(n, blocks, max_length)` triple — that hoists every possible memory
/// failure out of the algorithm itself.
///
/// # Complexity
/// With `L = max_length`, extension work is bounded by the prefixes of length
/// `< L`, so the runtime shrinks from the full `O(N² · 2ᴺ)` to
/// `O(N² · Σ_{k<L} C(N, k))` when `L < N` — identical to the flat-table
/// version; layering only changes storage.
///
/// # Panics
/// Panics if `n > M::MAX_POINTS`, `blocks.len() != n * n`, `max_length > n`,
/// or `scratch` was sized for a different `(n, max_length)` shape than
/// the one requested.
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

    // The two buffers were sized to the largest popcount layer this run
    // will ever hold (see [`dp_layer_capacity`]). They are pre-allocated
    // and reused via [`std::mem::swap`]; no per-iteration allocation
    // occurs.
    //
    // Per-state values can reach (len-1)! at the full mask, which exceeds
    // u64::MAX starting around n=22 — hence u128 (same rationale as `counts`).
    //
    // Layout: each popcount-`p` mask packs only its `p` valid endpoints in
    // mask-major order. The endpoint offset within a mask is the popcount of
    // (mask & (bit−1)) — a single hardware instruction. Source-layer
    // indices are an incrementing counter (Gosper order == colex order for
    // fixed popcount); destination-layer indices are computed via
    // prefix/suffix sums, so no `2ⁿ` lookup table is allocated.

    // Initialise the popcount-1 layer in dp_curr: each of the n masks has
    // exactly one valid endpoint and one way to reach it.
    for slot in &mut dp_curr[..n] {
        *slot = 1;
    }

    // Enumerate popcount classes ascending so every proper subset of a
    // popcount-`p` mask is already final by the time we read it. Streaming
    // `LengthDone` events fire as soon as a class completes. The loop
    // stops at `max_length - 1`: the popcount-`(max_length-1)` layer is
    // the last one whose extensions contribute to `counts[max_length]`,
    // and no caller-visible state is produced at higher popcounts.
    for p in 1..max_length {
        let next_p = p + 1;
        // We still accumulate `counts[max_length]` from the source layer
        // at p == max_length - 1, but skip the dp_next writes — they
        // would never be read.
        let need_dp_next = next_p < max_length;
        let next_len = if need_dp_next {
            // Match `dp_layer_capacity`: keep the product in u128 and
            // saturate at the `usize` boundary so the wider mask widths
            // do not silently truncate a > usize::MAX binomial down to
            // a small bogus length.
            let raw = binomial(n, next_p).saturating_mul(next_p as u128);
            usize::try_from(raw).unwrap_or(usize::MAX)
        } else {
            0
        };

        // Zero only the destination prefix that will be written. The rest
        // of the buffer carries stale data from earlier iterations but is
        // never indexed.
        if need_dp_next {
            for slot in &mut dp_next[..next_len] {
                *slot = 0;
            }
        }

        process_layer::<M, _>(LayerCtx {
            n,
            full_mask,
            blocks,
            p,
            need_dp_next,
            dp_curr: &*dp_curr,
            dp_next: &mut dp_next[..next_len],
            counts: &mut counts,
            on_event: &mut on_event,
        });

        // All contributions to counts[p+1] came from popcount-p masks, so
        // the value is now final.
        on_event(DpEvent::LengthDone {
            length: next_p,
            count: counts[next_p],
        });

        // Swap roles for the next iteration: today's destination becomes
        // tomorrow's source. The buffers themselves are reused, no
        // reallocation occurs.
        std::mem::swap(&mut dp_curr, &mut dp_next);
    }

    counts
}

/// Bundle of state passed into [`process_layer`].
///
/// Pulled into its own struct so the helper avoids `clippy::too_many_arguments`
/// while still threading the streaming `on_event` callback through.
struct LayerCtx<'a, M: Mask, F: FnMut(DpEvent)> {
    n: usize,
    full_mask: M,
    blocks: &'a [M],
    p: usize,
    need_dp_next: bool,
    dp_curr: &'a [u128],
    dp_next: &'a mut [u128],
    counts: &'a mut [u128],
    on_event: &'a mut F,
}

/// Processes every popcount-`p` mask exactly once, contributing its
/// extensions to `counts[p+1]` and (when `need_dp_next`) to `dp_next`.
///
/// Fires one [`DpEvent::Mask`] per mask. The caller is responsible for
/// only invoking `process_layer` for popcounts that contribute work
/// (`p < max_length`); the helper does no further bounds check.
fn process_layer<M: Mask, F: FnMut(DpEvent)>(ctx: LayerCtx<'_, M, F>) {
    let LayerCtx {
        n,
        full_mask,
        blocks,
        p,
        need_dp_next,
        dp_curr,
        dp_next,
        counts,
        on_event,
    } = ctx;

    let next_p = p + 1;
    // prefix/suffix sums reconstruct colex_rank(mask | next_bit) in O(1) per extension
    let mut prefix_sum: [usize; SLOTS] = [0; SLOTS];
    let mut suffix_sum: [usize; SLOTS] = [0; SLOTS];
    let mut bit_pos: [u32; SLOTS] = [0; SLOTS];

    let mut idx_curr: usize = 0;
    let mut mask: M = M::low_bits(p);
    let last: M = M::low_bits(p) << (n - p);
    loop {
        on_event(DpEvent::Mask);
        let base_curr = idx_curr * p;

        if need_dp_next {
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
            let mut free = !mask & full_mask;
            while free != M::ZERO {
                let next_bit = free & free.wrapping_neg();
                free ^= next_bit;
                let next = next_bit.trailing_zeros() as usize;

                let blockers = blocks[row_start + next];
                if mask & blockers == blockers {
                    counts[next_p] += ways;
                    if need_dp_next {
                        // `next_bit` is not in `mask`, so the set bits of
                        // (mask | next_bit) below `next_bit` are exactly
                        // the bits of `mask` below it.
                        let next_off = (mask & next_bit.wrapping_sub_one()).count_ones() as usize;
                        let idx_new =
                            prefix_sum[next_off] + BINOM[next][next_off + 1] + suffix_sum[next_off];
                        dp_next[idx_new * next_p + next_off] += ways;
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
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::grid::{GridDefinition, build_grid_definition, compute_blocks};

    /// Pascal's-triangle table sized for the legacy `u32` path. Used by
    /// the test-only `colex_rank` oracle below, which mirrors the
    /// production rank arithmetic at `u32` width so a future regression
    /// in `BINOM`'s widened layout is caught against a hand-rolled
    /// reference.
    const BINOM_U32: [[u32; 33]; 33] = {
        let mut t = [[0u32; 33]; 33];
        let mut i = 0;
        while i < 33 {
            t[i][0] = 1;
            let mut j = 1;
            while j <= i {
                t[i][j] = t[i - 1][j - 1] + t[i - 1][j];
                j += 1;
            }
            i += 1;
        }
        t
    };

    /// Colex rank of a bitmask of popcount `k`: a perfect hash into
    /// `[0, C(n, k))` where bit positions `a_1 < … < a_k` map to
    /// `Σ C(a_i, i)`.
    const fn colex_rank(mut mask: u32) -> u32 {
        let mut rank: u32 = 0;
        let mut k: usize = 1;
        while mask != 0 {
            let bit = mask & mask.wrapping_neg();
            let pos = bit.trailing_zeros() as usize;
            rank += BINOM_U32[pos][k];
            mask ^= bit;
            k += 1;
        }
        rank
    }

    /// Standalone Gosper's-hack helper for the perfect-hash test below.
    /// The production code calls it through the [`Mask`] trait; pinning a
    /// `u32`-only copy here keeps the test free of a trait round-trip.
    const fn gosper_next_u32(x: u32) -> u32 {
        let c = x & x.wrapping_neg();
        let r = x.wrapping_add(c);
        (((r ^ x) >> 2) / c) | r
    }

    // IDDFS oracle: cross-checks count_patterns_dp. Doesn't scale past n ≈ 25.
    fn count_patterns_dfs<M: Mask>(n: usize, blocks: &[M], max_length: usize) -> Vec<u128> {
        assert!(
            n <= M::MAX_POINTS,
            "N={n} exceeds the supported maximum of {}",
            M::MAX_POINTS
        );
        assert_eq!(blocks.len(), n * n, "blocks matrix must be n × n");
        assert!(
            max_length <= n,
            "max_length={max_length} must not exceed n={n}"
        );

        if blocks.iter().all(|&b| b == M::ZERO) {
            return count_unconstrained(n, max_length);
        }

        let mut counts = vec![0u128; max_length + 1];
        counts[0] = 1;
        if n == 0 || max_length == 0 {
            return counts;
        }
        counts[1] = n as u128;
        if max_length < 2 {
            return counts;
        }

        let full_mask: M = M::low_bits(n);

        for (i, count_slot) in counts[2..].iter_mut().enumerate() {
            let target = i + 2;
            let mut count_target = 0u128;
            for start in 0..n {
                let start_bit = M::bit(start);
                let row = start * n;
                for second in 0..n {
                    if second == start {
                        continue;
                    }
                    let second_bit = M::bit(second);
                    let blockers = blocks[row + second];
                    if start_bit & blockers == blockers {
                        if target == 2 {
                            count_target += 1;
                        } else {
                            count_target += iddfs_count::<M>(
                                start_bit | second_bit,
                                second,
                                2,
                                target,
                                blocks,
                                n,
                                full_mask,
                            );
                        }
                    }
                }
            }
            *count_slot = count_target;
        }

        counts
    }

    fn iddfs_count<M: Mask>(
        mask: M,
        end: usize,
        depth: usize,
        target: usize,
        blocks: &[M],
        n: usize,
        full_mask: M,
    ) -> u128 {
        let mut total = 0u128;
        let row = end * n;
        let mut free = !mask & full_mask;
        while free != M::ZERO {
            let next_bit = free & free.wrapping_neg();
            free ^= next_bit;
            let next = next_bit.trailing_zeros() as usize;
            let blockers = blocks[row + next];
            if mask & blockers == blockers {
                if depth + 1 == target {
                    total += 1;
                } else {
                    total += iddfs_count::<M>(
                        mask | next_bit,
                        next,
                        depth + 1,
                        target,
                        blocks,
                        n,
                        full_mask,
                    );
                }
            }
        }
        total
    }

    fn grid(dimensions: usize, points: Vec<Vec<i32>>) -> GridDefinition {
        GridDefinition { dimensions, points }
    }

    // Runs both counters at u32 width, asserts they agree, and returns the
    // result. Every test that checks output values goes through this helper
    // so that both algorithms are verified in a single pass on the legacy
    // hot path.
    fn count(n: usize, blocks: &[u32], max_length: usize) -> Vec<u128> {
        let mut scratch = DpScratch::allocate::<u32>(n, blocks, max_length).unwrap();
        let dp = count_patterns_dp(&mut scratch, n, blocks, max_length, |_| {});
        let dfs = count_patterns_dfs::<u32>(n, blocks, max_length);
        assert_eq!(
            dp, dfs,
            "DP and DFS counts diverge for n={n}, max_length={max_length}"
        );
        dp
    }

    #[test]
    fn android_3x3_matches_known_pattern_counts() {
        #[rustfmt::skip]
        let g = grid(
            2,
            vec![
                vec![0, 0], vec![1, 0], vec![2, 0],
                vec![0, 1], vec![1, 1], vec![2, 1],
                vec![0, 2], vec![1, 2], vec![2, 2],
            ],
        );
        g.validate().unwrap();
        let blocks = compute_blocks::<u32>(&g);
        let n = g.points.len();
        let counts = count(n, &blocks, n);

        assert_eq!(counts[0], 1);
        assert_eq!(counts[1], 9);
        assert_eq!(counts[2], 56);
        assert_eq!(counts[3], 320);
        assert_eq!(counts[4], 1_624);
        assert_eq!(counts[5], 7_152);
        assert_eq!(counts[6], 26_016);
        assert_eq!(counts[7], 72_912);
        assert_eq!(counts[8], 140_704);
        assert_eq!(counts[9], 140_704);
        assert_eq!(counts[4..=9].iter().sum::<u128>(), 389_112);
    }

    #[test]
    fn no_three_collinear_collapses_to_permutations() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![1, 1], vec![0, 1]]);
        let blocks = compute_blocks::<u32>(&g);
        assert!(blocks.iter().all(|&b| b == 0));
        let n = g.points.len();
        assert_eq!(count(n, &blocks, n), vec![1, 4, 12, 24, 24]);
    }

    #[test]
    fn blocker_becomes_transparent_once_visited() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![2, 0]]);
        let blocks = compute_blocks::<u32>(&g);
        let n = g.points.len();
        let counts = count(n, &blocks, n);

        assert_eq!(counts[1], 3);
        assert_eq!(counts[2], 4);
        // A→B→C, B→A→C, B→C→A, C→B→A survive.
        assert_eq!(counts[3], 4);
    }

    #[test]
    fn edge_cases_zero_and_one_point() {
        let empty = grid(2, vec![]);
        empty.validate().unwrap();
        let blocks = compute_blocks::<u32>(&empty);
        assert!(blocks.is_empty());
        assert_eq!(count(0, &blocks, 0), vec![1]);

        let single = grid(2, vec![vec![7, 7]]);
        single.validate().unwrap();
        let blocks = compute_blocks::<u32>(&single);
        assert_eq!(blocks, vec![0]);
        assert_eq!(count(1, &blocks, 1), vec![1, 1]);
    }

    #[test]
    fn generated_3x3_matches_known_pattern_counts() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks::<u32>(&g);
        let n = g.points.len();
        let counts = count(n, &blocks, n);
        assert_eq!(counts[4..=9].iter().sum::<u128>(), 389_112);
    }

    #[test]
    fn max_length_truncates_counts_to_prefix_of_full_run() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks::<u32>(&g);
        let n = g.points.len();
        let full = count(n, &blocks, n);
        for cap in 0..=n {
            let capped = count(n, &blocks, cap);
            assert_eq!(capped.len(), cap + 1, "unexpected length for cap={cap}");
            assert_eq!(
                capped.as_slice(),
                &full[..=cap],
                "truncated counts disagree with full run at cap={cap}"
            );
        }
    }

    // Regression test: at n=21 with 5 free points, per-endpoint `ways` values at
    // intermediate mask sizes reach the u64 ceiling and would wrap silently if
    // the DP table were not u128, producing a non-monotonic `counts` sequence.
    // Final counts[21] is ≈7.29×10¹⁸ — below u64::MAX thanks to the 4×4
    // collinearity blockers, so we cannot detect the bug from counts[n] alone;
    // we lock in monotonicity across every length instead.
    //
    // Cheaper sibling of `count_4x4_plus_8_free_is_monotonic_in_length`
    // (~125 MB vs ~1 GB after the layered DP) so the invariant has a runnable
    // check on smaller hardware.
    #[test]
    #[ignore = "allocates ~125 MB — run manually with: cargo test -- --ignored"]
    fn count_4x4_plus_5_free_is_monotonic_in_length() {
        let g = build_grid_definition(&[4, 4], 5);
        let blocks = compute_blocks::<u32>(&g);
        let n = g.points.len();
        assert_eq!(n, 21);
        let mut scratch = DpScratch::allocate::<u32>(n, &blocks, n).unwrap();
        let counts = count_patterns_dp(&mut scratch, n, &blocks, n, |_| {});
        for k in 1..=n {
            assert!(
                counts[k] >= counts[k - 1],
                "counts[{k}]={} must be >= counts[{}]={}; DP likely overflowed",
                counts[k],
                k - 1,
                counts[k - 1]
            );
        }
    }

    // Regression test for silent overflow in the DP table itself. At n=24, the
    // per-endpoint `ways` stored at mask length 23 reaches ≈21! ≈ 5.1×10¹⁹,
    // which wraps in u64 and produces counts[24] < counts[23] — a monotonicity
    // violation that is provably impossible (every full pattern of length 24
    // extends some length-23 prefix). We assert on every suffix length to lock
    // the invariant in place.
    //
    // The layered DP brought the peak from ~6.4 GB down to ~1 GB, but the test
    // remains gated by default to keep `cargo test` fast.
    #[test]
    #[ignore = "allocates ~1 GB — run manually with: cargo test -- --ignored"]
    fn count_4x4_plus_8_free_is_monotonic_in_length() {
        let g = build_grid_definition(&[4, 4], 8);
        let blocks = compute_blocks::<u32>(&g);
        let n = g.points.len();
        assert_eq!(n, 24);
        let mut scratch = DpScratch::allocate::<u32>(n, &blocks, n).unwrap();
        let counts = count_patterns_dp(&mut scratch, n, &blocks, n, |_| {});
        for k in 1..=n {
            assert!(
                counts[k] >= counts[k - 1],
                "counts[{k}]={} must be >= counts[{}]={}; DP likely overflowed",
                counts[k],
                k - 1,
                counts[k - 1]
            );
        }
        // With 8 free orthogonal axes the base-grid collinearities are
        // preserved, so counts[n] is strictly less than n! but still well above
        // u64::MAX.
        assert!(counts[n] > u128::from(u64::MAX));
    }

    #[test]
    fn max_length_zero_on_nonempty_grid_returns_only_empty_pattern() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks::<u32>(&g);
        assert_eq!(count(g.points.len(), &blocks, 0), vec![1]);
    }

    #[test]
    fn max_length_one_reports_only_singletons() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks::<u32>(&g);
        assert_eq!(count(g.points.len(), &blocks, 1), vec![1, 9]);
    }

    #[test]
    fn max_length_four_matches_android_minimum_run() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks::<u32>(&g);
        assert_eq!(count(g.points.len(), &blocks, 4)[4], 1_624);
    }

    // Verify the closed-form formula P(n,k) = n!/(n-k)! is mathematically
    // correct for n = 0..=7 by checking against the falling-factorial definition.
    #[test]
    fn unconstrained_formula_is_falling_factorial() {
        for n in 0..=7usize {
            let zero_blocks = vec![0u32; n * n];
            let counts = count(n, &zero_blocks, n);
            assert_eq!(counts.len(), n + 1);

            let mut expected = vec![0u128; n + 1];
            expected[0] = 1;
            let mut perm = 1u128;
            for (k, slot) in expected.iter_mut().enumerate().skip(1) {
                perm *= (n - k + 1) as u128;
                *slot = perm;
            }
            assert_eq!(counts, expected, "n={n}");
        }
    }

    // For a grid with no collinear triplets the fast path must fire and must
    // produce the same counts that the DP previously returned (regression guard).
    #[test]
    fn fast_path_matches_known_dp_result_for_unconstrained_grid() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![1, 1], vec![0, 1]]);
        let blocks = compute_blocks::<u32>(&g);
        assert!(
            blocks.iter().all(|&b| b == 0),
            "expected zero block matrix for a square grid"
        );
        let n = g.points.len();
        // P(4, k) for k=0..4: [1, 4, 12, 24, 24]
        assert_eq!(count(n, &blocks, n), vec![1, 4, 12, 24, 24]);
    }

    // For a grid with collinear points the fast path must NOT fire; the DP
    // result must differ from the unconstrained formula.
    #[test]
    fn constrained_grid_does_not_use_fast_path() {
        // Three collinear points: node 1 blocks 0→2 and 2→0.
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![2, 0]]);
        let blocks = compute_blocks::<u32>(&g);
        assert!(
            blocks.iter().any(|&b| b != 0),
            "expected non-zero block matrix for collinear points"
        );
        let n = g.points.len();
        let counts = count(n, &blocks, n);
        // Constrained: only 4 valid length-3 patterns, not 3! = 6.
        assert_eq!(counts[3], 4);
        // Unconstrained formula would give P(3,3) = 6.
        assert_ne!(counts[3], 6);
    }

    #[test]
    fn dp_table_bytes_zero_n_or_zero_max_length_returns_counts_only() {
        // n == 0: only counts[0..=0] is allocated, so 16 bytes.
        assert_eq!(dp_table_bytes(0, 0), 16);
        // n > 0 but max_length == 0: function returns before the index/DP
        // allocations, so still only counts[0..=0] = 16 bytes.
        assert_eq!(dp_table_bytes(31, 0), 16);
    }

    #[test]
    fn dp_table_bytes_is_monotone_in_max_length() {
        for n in 0..=10 {
            let mut prev: u64 = 0;
            for l in 0..=n {
                let b = dp_table_bytes(n, l);
                assert!(
                    b >= prev,
                    "dp_table_bytes(n={n}, max_length={l}) regressed: {b} < {prev}"
                );
                prev = b;
            }
        }
    }

    #[test]
    fn dp_table_bytes_max_length_above_n_matches_full_run() {
        // The function clamps `max_length` to `n` internally, so passing a
        // value greater than `n` must yield the same byte count as `n`.
        // We compare against `n` itself rather than recomputing the formula
        // so the test exercises the public API.
        for n in 1..=8 {
            let full = dp_table_bytes(n, n);
            assert_eq!(dp_table_bytes(n, n + 1), full, "n={n}, max_length=n+1");
            assert_eq!(dp_table_bytes(n, 64), full, "n={n}, max_length=64");
        }
    }

    #[test]
    fn dp_table_bytes_handles_max_supported_n_without_overflow() {
        // n = 31, full run: peak DP layer pair dominates at this scale.
        // Must not saturate to u64::MAX — we only need a finite, sensible bound.
        let bytes = dp_table_bytes(31, 31);
        assert!(bytes < u64::MAX);
        assert!(bytes > (1u64 << 30), "expected at least 1 GiB, got {bytes}");
    }

    /// `dp_table_bytes` must saturate cleanly at the wider [`Mask`]
    /// ceilings rather than wrap. At `n = mask::MAX_POINTS` the peak
    /// popcount layer's byte count vastly exceeds any physical
    /// `u64::MAX`-byte bound, so the saturating-arithmetic chain is
    /// expected to flatten to `u64::MAX`. Pinning that here proves the
    /// `binomial → saturating_mul → saturating_add → try_from` pipeline
    /// is wired correctly even when intermediate `u128` products
    /// overflow during construction.
    #[test]
    fn dp_table_bytes_saturates_at_widest_mask_ceiling() {
        assert_eq!(
            dp_table_bytes(crate::mask::MAX_POINTS, crate::mask::MAX_POINTS),
            u64::MAX
        );
    }

    // Hand-verified expected sizes for n = 3 across every max_length.
    //   max_length < 2: no DP buffer is allocated; only counts (16 B per slot).
    //   max_length >= 2: two ping-pong buffers of size M = max_{p in 1..L} C(n,p)·p
    //     u128 entries, plus counts of (L+1)·16 B.
    //     M(L=2) = C(3,1)·1 = 3
    //     M(L=3) = max(3, C(3,2)·2) = 6
    #[test]
    fn dp_table_bytes_n3_known_values() {
        // max_length = 0 → early exit, counts only (1·16 = 16 B).
        assert_eq!(dp_table_bytes(3, 0), 16);
        // max_length = 1 → early exit (no DP layer iterated), counts = 2·16 = 32 B.
        assert_eq!(dp_table_bytes(3, 1), 32);
        // max_length = 2 → 2·M·16 + counts = 2·3·16 + 3·16 = 96 + 48 = 144 B.
        assert_eq!(dp_table_bytes(3, 2), 96 + 48);
        // max_length = 3 → 2·M·16 + counts = 2·6·16 + 4·16 = 192 + 64 = 256 B.
        assert_eq!(dp_table_bytes(3, 3), 192 + 64);
    }

    // effective_max_length must be monotone in budget and never exceed
    // requested.min(n). Cross-check by evaluating dp_table_bytes at the
    // returned cap and the next length up.
    #[test]
    fn effective_max_length_respects_budget_monotonically() {
        for n in 1..=8 {
            let mut prev: usize = 0;
            // Sweep budgets in 1 KiB steps from 0 up to the full-run cost.
            let full_bytes = dp_table_bytes(n, n);
            let step: u64 = (full_bytes / 16).max(1024);
            let mut budget: u64 = 0;
            loop {
                let eff = effective_max_length(n, n, budget);
                assert!(eff <= n, "n={n}, budget={budget}: eff={eff} exceeds n");
                assert!(
                    eff >= prev,
                    "n={n}, budget={budget}: eff regressed {prev} -> {eff}"
                );
                if eff < n {
                    let next = eff + 1;
                    assert!(
                        dp_table_bytes(n, next) > budget,
                        "n={n}, budget={budget}: cap {eff} could have been {next} (still fits)",
                    );
                }
                if eff > 0 {
                    assert!(
                        dp_table_bytes(n, eff) <= budget,
                        "n={n}, budget={budget}: returned cap {eff} does not fit"
                    );
                }
                prev = eff;
                if budget >= full_bytes {
                    break;
                }
                budget = budget.saturating_add(step);
            }
        }
    }

    #[test]
    fn effective_max_length_clamps_to_requested_and_n() {
        // With u64::MAX budget the cap is the smaller of requested and n.
        for n in 0..=8 {
            for req in 0..=12 {
                let eff = effective_max_length(n, req, u64::MAX);
                assert_eq!(eff, req.min(n), "n={n}, req={req}");
            }
        }
    }

    #[test]
    fn effective_max_length_zero_budget_falls_back_to_zero() {
        // Even with zero budget the helper returns 0 — the caller can still
        // emit the trivial counts[0] = 1 result rather than aborting.
        for n in 0..=8 {
            assert_eq!(effective_max_length(n, n, 0), 0);
        }
    }

    // Verifies `colex_rank` is a perfect hash on each popcount class — the
    // contract `process_layer` relies on for write-side indexing into
    // `dp_next`. Sweeps every popcount-`k` mask of an n=12 universe and
    // checks that the ranks are exactly `0..C(12, k)`.
    #[test]
    fn colex_rank_is_a_perfect_hash_per_popcount() {
        let n: u32 = 12;
        for k in 1..=n {
            let mut mask: u32 = (1u32 << k) - 1;
            let last: u32 = mask << (n - k);
            let mut expected: u32 = 0;
            loop {
                assert_eq!(colex_rank(mask), expected, "n={n}, k={k}, mask={mask:#b}");
                if mask == last {
                    break;
                }
                mask = gosper_next_u32(mask);
                expected += 1;
            }
        }
    }

    // The constrained DP must emit exactly `dp_mask_ticks(n, max_length)`
    // `Mask` events — the contract the CLI relies on to size its progress
    // bar so it reaches 100% precisely when the run finishes. Sweep across
    // every `max_length` cap to cover the early-exit branches as well.
    #[test]
    fn dp_mask_event_count_matches_dp_mask_ticks() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks::<u32>(&g);
        assert!(
            blocks.iter().any(|&b| b != 0),
            "3×3 grid must be constrained for this test to exercise the popcount loop"
        );
        let n = g.points.len();
        for cap in 0..=n {
            let mask_count = std::cell::Cell::new(0u64);
            let mut scratch = DpScratch::allocate::<u32>(n, &blocks, cap).unwrap();
            count_patterns_dp(&mut scratch, n, &blocks, cap, |event| {
                if matches!(event, DpEvent::Mask) {
                    mask_count.set(mask_count.get() + 1);
                }
            });
            assert_eq!(
                mask_count.get(),
                dp_mask_ticks(n, cap),
                "Mask event count diverges from dp_mask_ticks at cap={cap}"
            );
        }
    }

    #[test]
    fn dp_mask_ticks_known_values() {
        // n == 0 or max_length < 2: no popcount loop, no Mask events.
        assert_eq!(dp_mask_ticks(0, 0), 0);
        assert_eq!(dp_mask_ticks(5, 0), 0);
        assert_eq!(dp_mask_ticks(5, 1), 0);
        // max_length = 2 with n = 5: just popcount-1 masks → C(5,1) = 5.
        assert_eq!(dp_mask_ticks(5, 2), 5);
        // max_length = 3 with n = 5: C(5,1) + C(5,2) = 5 + 10 = 15.
        assert_eq!(dp_mask_ticks(5, 3), 15);
        // Full run: Σ_{p=1}^{n-1} C(n, p) = 2ⁿ − 2.
        for n in 2..=10 {
            let expected: u64 = (1u64 << n) - 2;
            assert_eq!(dp_mask_ticks(n, n), expected, "n={n}");
        }
    }

    // Covers `C(n, k) = 0` when `k > n`, including the degenerate `n = 0`.
    #[test]
    fn binomial_returns_zero_when_k_exceeds_n() {
        assert_eq!(binomial(3, 5), 0);
        assert_eq!(binomial(0, 1), 0);
    }

    /// `binomial` must saturate to `u128::MAX` on intermediate-product
    /// overflow rather than wrap silently. At `n = 200, k = 100` the
    /// product `C(n, k_partial) * (n - k_partial)` blows past `u128::MAX`
    /// well before the final value would fit, so the saturating
    /// short-circuit is the only path that can return without
    /// triggering Rust's release-mode `u128 *` UB. Pinning the saturated
    /// return here keeps `dp_table_bytes` correct for the widest
    /// supported `n`.
    #[test]
    fn binomial_saturates_when_intermediate_product_overflows() {
        assert_eq!(binomial(200, 100), u128::MAX);
    }

    // Covers `l < 2`, where no DP buffer is needed and the capacity is 0.
    #[test]
    fn dp_layer_capacity_returns_zero_below_two() {
        assert_eq!(dp_layer_capacity(5, 0), 0);
        assert_eq!(dp_layer_capacity(5, 1), 0);
    }

    /// `dp_layer_capacity` must saturate to `usize::MAX` rather than
    /// silently truncate when a `binomial(n, p) * p` product exceeds
    /// `usize`. At `n = 127, max_length = 127` the peak `C(127, 63) * 63`
    /// is on the order of `2^127`, far above `usize::MAX` on any 64-bit
    /// host; saturating here is the only thing that lets
    /// [`DpScratch::allocate`] forward the request as a real
    /// `try_reserve_exact` failure instead of producing an undersized
    /// buffer that [`process_layer`] would write past.
    #[test]
    fn dp_layer_capacity_saturates_when_binomial_exceeds_usize() {
        assert_eq!(
            dp_layer_capacity(crate::mask::MAX_POINTS, crate::mask::MAX_POINTS),
            usize::MAX
        );
    }

    /// BINOM-read safety: every cell `process_layer` can index for any
    /// `(n, max_length)` whose `dp_layer_capacity` did not saturate must
    /// be non-saturated. Saturated cells exist deep in the table (rows
    /// past ~67 on a 64-bit host), but [`DpScratch::allocate`] rejects
    /// any `(n, max_length)` whose capacity saturates — so the algorithm
    /// only enters [`process_layer`] for shapes whose reachable BINOM
    /// rectangle is finite. Pinning that invariant by sweep guards
    /// against a future change to either the memory clamp or the
    /// `process_layer` indexing arithmetic that would silently let the
    /// DP read a `usize::MAX` cell.
    #[test]
    fn binom_reads_never_saturate_for_clampable_runs() {
        for n in 1..=mask::MAX_POINTS {
            // Largest `max_length` whose buffer the allocator could
            // theoretically satisfy. Past this boundary
            // `DpScratch::allocate` short-circuits to `Err` and
            // `process_layer` is never reached.
            let l_max = (2..=n)
                .rev()
                .find(|&l| dp_layer_capacity(n, l) != usize::MAX);
            let Some(l) = l_max else {
                continue;
            };
            // `process_layer` reads `BINOM[i][j]` with `i < n` and
            // `j` running through `i + 1`, `j + 2`, and `next_off + 1`
            // — all bounded by `next_p ≤ max_length`. Sweep the full
            // reachable rectangle.
            for (i, row) in BINOM.iter().enumerate().take(n) {
                for (j, &cell) in row.iter().enumerate().take(l + 1) {
                    assert_ne!(
                        cell,
                        usize::MAX,
                        "BINOM[{i}][{j}] saturated for n={n}, max_length={l} \
                         — process_layer would read a bogus value",
                    );
                }
            }
        }
    }

    // `zeroed_buffer` is a thin wrapper around `Vec::try_reserve_exact` plus a
    // zero-fill: a successful call produces a vector of the requested length
    // filled with zeros, and an impossible request surfaces the allocator's
    // error instead of aborting. The unreachable `usize::MAX` request triggers
    // a capacity overflow inside the allocator, exercising the `?` branch.
    #[test]
    fn zeroed_buffer_yields_zeroed_vec_or_propagates_failure() {
        let buf = zeroed_buffer(4).unwrap();
        assert_eq!(buf, vec![0u128; 4]);
        assert_eq!(buf.capacity(), 4);

        assert!(zeroed_buffer(usize::MAX).is_err());
    }

    // `DpScratch::allocate` skips the (potentially huge) allocation when
    // `count_patterns_dp` would itself bail out before touching the buffers —
    // either because the run is too short to enter the popcount loop or
    // because the unconstrained fast path will handle it.
    #[test]
    fn dp_scratch_allocate_is_empty_when_dp_body_short_circuits() {
        let constrained: Vec<u32> = compute_blocks(&build_grid_definition(&[3, 3], 0));
        for max_length in 0..=1 {
            let scratch = DpScratch::allocate::<u32>(9, &constrained, max_length).unwrap();
            assert_eq!(scratch.buf.len(), 0);
            assert_eq!(scratch.half, 0);
        }

        let unconstrained = vec![0u32; 9];
        let scratch = DpScratch::allocate::<u32>(3, &unconstrained, 3).unwrap();
        assert_eq!(scratch.buf.len(), 0);
        assert_eq!(scratch.half, 0);
    }

    // For a real constrained run the buffer holds two layers of
    // `dp_layer_capacity(n, max_length)` `u128` slots, all zeroed.
    #[test]
    fn dp_scratch_allocate_sizes_to_two_ping_pong_layers() {
        let blocks = compute_blocks::<u32>(&build_grid_definition(&[3, 3], 0));
        let scratch = DpScratch::allocate::<u32>(9, &blocks, 9).unwrap();
        let half = dp_layer_capacity(9, 9);
        assert_eq!(scratch.half, half);
        assert_eq!(scratch.buf.len(), 2 * half);
        assert!(scratch.buf.iter().all(|&v| v == 0));
    }

    // Allocator failure on the underlying request must propagate as an
    // error rather than aborting. `usize::MAX` saturates the doubled
    // request and is rejected up front by `Vec::try_reserve_exact`.
    #[test]
    fn dp_scratch_with_layer_capacity_propagates_failure() {
        assert!(DpScratch::with_layer_capacity(usize::MAX).is_err());
    }

    #[test]
    fn dp_mask_ticks_clamps_max_length_above_n() {
        for n in 1..=8 {
            let full = dp_mask_ticks(n, n);
            assert_eq!(dp_mask_ticks(n, n + 1), full, "n={n}");
            assert_eq!(dp_mask_ticks(n, 64), full, "n={n}");
        }
    }

    // The unconstrained fast path must emit zero Mask events regardless of
    // `max_length` — the bar never ticks but the run completes correctly.
    #[test]
    fn dp_mask_events_are_zero_on_unconstrained_fast_path() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![1, 1], vec![0, 1]]);
        let blocks = compute_blocks::<u32>(&g);
        assert!(blocks.iter().all(|&b| b == 0));
        let n = g.points.len();
        let mut scratch = DpScratch::allocate::<u32>(n, &blocks, n).unwrap();
        count_patterns_dp(&mut scratch, n, &blocks, n, |event| {
            assert!(
                !matches!(event, DpEvent::Mask),
                "unconstrained fast path must not emit Mask events",
            );
        });
    }

    /// Cross-width parity: every supported [`Mask`] impl, run on the same
    /// constrained 3×3 grid, must produce the bit-identical `counts`
    /// vector. This pins the contract that monomorphisation only changes
    /// how the visited-set is encoded — never the result — and catches a
    /// future impl that accidentally diverges in `bit`, `low_bits`,
    /// `gosper_next`, or any of the other forwarded methods.
    #[test]
    fn dp_widths_agree_on_constrained_grid() {
        let g = build_grid_definition(&[3, 3], 0);
        let n = g.points.len();

        let blocks_u32: Vec<u32> = compute_blocks(&g);
        let blocks_u64: Vec<u64> = compute_blocks(&g);
        let blocks_u128: Vec<u128> = compute_blocks(&g);

        let mut s32 = DpScratch::allocate::<u32>(n, &blocks_u32, n).unwrap();
        let mut s64 = DpScratch::allocate::<u64>(n, &blocks_u64, n).unwrap();
        let mut s128 = DpScratch::allocate::<u128>(n, &blocks_u128, n).unwrap();

        let c32 = count_patterns_dp(&mut s32, n, &blocks_u32, n, |_| {});
        let c64 = count_patterns_dp(&mut s64, n, &blocks_u64, n, |_| {});
        let c128 = count_patterns_dp(&mut s128, n, &blocks_u128, n, |_| {});

        assert_eq!(c32, c64);
        assert_eq!(c64, c128);
    }

    /// Cross-width parity on the unconstrained fast path. With every
    /// block mask zero the DP body short-circuits to
    /// `count_unconstrained` — but the all-zero check is itself
    /// per-width (`b == M::ZERO`), so this guards the fast-path branch
    /// at every supported width.
    #[test]
    fn dp_widths_agree_on_unconstrained_fast_path() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![1, 1], vec![0, 1]]);
        let n = g.points.len();
        let blocks_u32: Vec<u32> = compute_blocks(&g);
        let blocks_u64: Vec<u64> = compute_blocks(&g);
        let blocks_u128: Vec<u128> = compute_blocks(&g);

        let mut s32 = DpScratch::allocate::<u32>(n, &blocks_u32, n).unwrap();
        let mut s64 = DpScratch::allocate::<u64>(n, &blocks_u64, n).unwrap();
        let mut s128 = DpScratch::allocate::<u128>(n, &blocks_u128, n).unwrap();

        let c32 = count_patterns_dp(&mut s32, n, &blocks_u32, n, |_| {});
        let c64 = count_patterns_dp(&mut s64, n, &blocks_u64, n, |_| {});
        let c128 = count_patterns_dp(&mut s128, n, &blocks_u128, n, |_| {});

        let expected = vec![1u128, 4, 12, 24, 24];
        assert_eq!(c32, expected);
        assert_eq!(c64, expected);
        assert_eq!(c128, expected);
    }

    /// Cross-width parity past the `u32` ceiling. `n = 32` cannot be
    /// represented in `u32` (`MAX_POINTS = 31`), so the smaller parity
    /// tests above cannot pin the `u64`-vs-`u128` contract on its own
    /// — this one does, by running both wider monomorphisations on a
    /// constrained 1×32 grid and asserting bit-identical `counts`.
    /// `max_length` is capped to 4 so the run stays small while still
    /// exercising several popcount layers (the full DP body, not just
    /// the early-exit branches).
    #[test]
    fn dp_widths_u64_u128_agree_past_u32_ceiling() {
        let g = build_grid_definition(&[1, 32], 0);
        let n = g.points.len();
        assert_eq!(n, 32);

        let blocks_u64: Vec<u64> = compute_blocks(&g);
        let blocks_u128: Vec<u128> = compute_blocks(&g);
        let max_length = 4;

        let mut s64 = DpScratch::allocate::<u64>(n, &blocks_u64, max_length).unwrap();
        let mut s128 = DpScratch::allocate::<u128>(n, &blocks_u128, max_length).unwrap();

        let c64 = count_patterns_dp(&mut s64, n, &blocks_u64, max_length, |_| {});
        let c128 = count_patterns_dp(&mut s128, n, &blocks_u128, max_length, |_| {});

        assert_eq!(c64, c128);
    }
}
