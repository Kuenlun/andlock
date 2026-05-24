// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

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
