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

use crate::grid::MAX_POINTS;

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

/// Progress / streaming event emitted by [`count_patterns_dfs`] during
/// execution.
///
/// IDDFS runs one pass per target length. Each pass counts patterns of exactly
/// that length independently, so [`DfsEvent::LengthDone`] carries a *final*
/// value the moment it fires — the caller can print it immediately and it will
/// never change.
pub enum DfsEvent {
    /// Beginning of the pass that counts patterns of exactly `target` nodes.
    /// Fires once per target length `≥ 2`. Not fired on the unconstrained
    /// fast path. `pair_total` is the number of top-level `(start, second)`
    /// pairs that will tick in this pass — use it to size a per-pass bar.
    PassStart { target: usize, pair_total: u64 },
    /// One top-level `(start, second)` pair for the current pass has been
    /// fully explored. Fires `pair_total` times per pass; not fired on the
    /// unconstrained fast path.
    PassTick { target: usize, pair_index: u64 },
    /// `counts[length]` is now final. Fires for every entry of the returned
    /// vector in strictly ascending order: lengths 0 and 1 fire before any
    /// `PassStart`; length `k ≥ 2` fires immediately after its pass
    /// completes.
    LengthDone { length: usize, count: u128 },
}

/// Number of [`DfsEvent::PassTick`] events fired during one IDDFS pass.
///
/// Equal to `n · (n − 1)`, clamped against overflow. The same count repeats
/// for every pass target; callers use it to size a per-pass progress bar.
#[must_use]
pub const fn dfs_pass_ticks(n: usize) -> u64 {
    let n = n as u64;
    n.saturating_mul(n.saturating_sub(1))
}

/// Which counting algorithm to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    /// Bottom-up bitmask DP — fast but allocates a `2ⁿ × n × 16`-byte table.
    Dp,
    /// Explicit DFS — slow but uses only `O(n)` memory.
    Dfs,
}

/// Exact binomial coefficient `C(n, k)`, returned as `u128`.
///
/// The intermediate products in the standard `result = result * (n-i) / (i+1)`
/// recurrence stay in `u128` so the routine remains exact for any `n` and `k`
/// the rest of the crate can produce — well beyond the `n ≤ MAX_POINTS` the
/// algorithm itself accepts.
fn binomial(n: usize, k: usize) -> u128 {
    if k > n {
        return 0;
    }
    let k = if k * 2 > n { n - k } else { k };
    let mut result: u128 = 1;
    for i in 0..k {
        result = result * (n - i) as u128 / (i + 1) as u128;
    }
    result
}

/// Returns the peak number of bytes [`count_patterns_dp`] would allocate for
/// `n` nodes when called with `max_length`.
///
/// The layered DP holds at most two adjacent popcount layers — popcount `p`
/// (read source) and popcount `p+1` (write destination). Each mask of
/// popcount `p` stores only `p` `u128` slots (one per valid endpoint), and
/// the layer-local index of a destination mask is computed on the fly as
/// its colex rank (a perfect hash into `[0, C(n, p+1))`), so no `2ⁿ`
/// auxiliary table is allocated. A destination layer for popcount `p+1` is
/// only allocated while `p+1 < max_length`; once the cap is reached the
/// source layer is consumed but no further layer is built.
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

    // Early exit in the DP body: when n == 0 or max_length == 0 the function
    // returns before allocating any DP layer.
    if n == 0 || max_length == 0 {
        return u64::try_from(counts_bytes).unwrap_or(u64::MAX);
    }

    let n_c = n.min(63);
    let l = max_length.min(n_c);

    // Peak DP layer pair, in u128 entries (×16 bytes at the end).
    //
    // Layer 1 (`dp_curr` initial) is unconditionally allocated to `n` entries.
    // For iteration `p`:
    //   * `dp_curr` holds the popcount-`p` layer — non-empty only when it was
    //     written as a destination by the previous iteration, i.e. `p < l`
    //     (or always for `p == 1`, hard-coded by the initialisation).
    //   * `dp_next` holds the popcount-`p+1` layer — allocated only while
    //     `p + 1 < l`.
    let mut peak_pair_entries: u128 = n_c as u128;

    // p == 1 (special-cased: dp_curr is always sized n).
    let dp_next_p1 = if 2 < l {
        binomial(n_c, 2).saturating_mul(2)
    } else {
        0
    };
    let pair_p1 = (n_c as u128).saturating_add(dp_next_p1);
    if pair_p1 > peak_pair_entries {
        peak_pair_entries = pair_p1;
    }

    // p >= 2.
    for p in 2..=n_c {
        let dp_curr = if p < l {
            binomial(n_c, p).saturating_mul(p as u128)
        } else {
            0
        };
        let dp_next = if p + 1 < l {
            binomial(n_c, p + 1).saturating_mul((p + 1) as u128)
        } else {
            0
        };
        let pair = dp_curr.saturating_add(dp_next);
        if pair > peak_pair_entries {
            peak_pair_entries = pair;
        }
    }

    let dp_bytes = peak_pair_entries.saturating_mul(16);
    let total = dp_bytes.saturating_add(counts_bytes);
    u64::try_from(total).unwrap_or(u64::MAX)
}

/// Returns [`Algorithm::Dp`] if [`dp_table_bytes`] for `(n, max_length)` fits
/// within `memory_budget` bytes, otherwise [`Algorithm::Dfs`].
///
/// Callers that want to *force* an algorithm should bypass this helper and
/// construct the [`Algorithm`] variant directly.
#[must_use]
pub fn choose_algorithm(n: usize, max_length: usize, memory_budget: u64) -> Algorithm {
    if dp_table_bytes(n, max_length) <= memory_budget {
        Algorithm::Dp
    } else {
        Algorithm::Dfs
    }
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
    let n_c = n.min(63);
    let l = max_length.min(n_c);
    let mut total: u128 = 0;
    for p in 1..l {
        total = total.saturating_add(binomial(n_c, p));
    }
    u64::try_from(total).unwrap_or(u64::MAX)
}

/// Counts every valid pattern, routing to DP or DFS based on `algorithm`.
///
/// The `on_tick` callback is invoked once per outer-loop step: once per
/// bitmask for DP (up to `2ⁿ − 1` calls) or once per `(start, second)`
/// top-level pair for DFS (up to `n · (n − 1)` calls). Per-length streaming
/// is not available through this interface; use [`count_patterns_dp`] or
/// [`count_patterns_dfs`] directly to observe [`DpEvent::LengthDone`] /
/// [`DfsEvent::LengthDone`].
///
/// # Panics
/// Panics under the same conditions as the underlying algorithm.
pub fn count_patterns(
    n: usize,
    blocks: &[u32],
    max_length: usize,
    algorithm: Algorithm,
    on_tick: impl Fn(),
) -> Vec<u128> {
    match algorithm {
        Algorithm::Dp => count_patterns_dp(n, blocks, max_length, |event| {
            if matches!(event, DpEvent::Mask) {
                on_tick();
            }
        }),
        Algorithm::Dfs => count_patterns_dfs(n, blocks, max_length, |event| {
            if matches!(event, DfsEvent::PassTick { .. }) {
                on_tick();
            }
        }),
    }
}

/// Next bitmask with the same popcount as `x` (Gosper's hack).
///
/// Used to enumerate masks in popcount-ascending order so that each popcount
/// layer can emit a [`DpEvent::LengthDone`] as soon as it completes.
const fn gosper_next(x: u32) -> u32 {
    let c = x & x.wrapping_neg();
    let r = x.wrapping_add(c);
    (((r ^ x) >> 2) / c) | r
}

/// Pascal's triangle, indexed `[n][k] = C(n, k)`. Sized to cover every
/// `n ≤ MAX_POINTS` (= 31) plus a small margin for off-by-one indexing
/// inside [`colex_rank`]. `C(32, 16) ≈ 6.0 × 10⁸` fits in `u32`.
const BINOM: [[u32; 33]; 33] = {
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
///
/// Replaces the `2ⁿ × u32` mask→index lookup table the layered DP used to
/// allocate. Crucially, Gosper enumeration walks popcount-`k` masks in
/// numeric ascending order — which equals colex-rank order for fixed
/// popcount — so the source layer can still be read with an incrementing
/// counter; only writes into the destination layer need an explicit rank.
#[inline]
const fn colex_rank(mut mask: u32) -> u32 {
    let mut rank: u32 = 0;
    let mut k: usize = 1;
    while mask != 0 {
        let bit = mask & mask.wrapping_neg();
        let pos = bit.trailing_zeros() as usize;
        rank += BINOM[pos][k];
        mask ^= bit;
        k += 1;
    }
    rank
}

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
/// # Memory
/// Two adjacent popcount layers are alive at any time: the source layer
/// (popcount `p`, read) and the destination (popcount `p+1`, written).
/// Each mask of popcount `p` packs only `p` `u128` slots — one per valid
/// endpoint. Layer-local indices are computed via [`colex_rank`] instead
/// of stored in a `2ⁿ × u32` lookup table, which keeps the working set
/// proportional to the actual popcount layers rather than `2ⁿ`. The source
/// layer is read with an incrementing counter that mirrors Gosper order
/// (= colex order for fixed popcount); only writes into the destination
/// layer pay the O(p) rank computation. See [`dp_table_bytes`].
///
/// # Complexity
/// With `L = max_length`, extension work is bounded by the prefixes of length
/// `< L`, so the runtime shrinks from the full `O(N² · 2ᴺ)` to
/// `O(N² · Σ_{k<L} C(N, k))` when `L < N` — identical to the flat-table
/// version; layering only changes storage.
///
/// # Panics
/// Panics if `n > MAX_POINTS`, `blocks.len() != n * n`, or `max_length > n`.
pub fn count_patterns_dp<F: FnMut(DpEvent)>(
    n: usize,
    blocks: &[u32],
    max_length: usize,
    mut on_event: F,
) -> Vec<u128> {
    assert!(n <= MAX_POINTS, "N={n} exceeds the maximum of {MAX_POINTS}");
    assert_eq!(blocks.len(), n * n, "blocks matrix must be n × n");
    assert!(
        max_length <= n,
        "max_length={max_length} must not exceed n={n}"
    );

    if blocks.iter().all(|&b| b == 0) {
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
    if n == 0 || max_length == 0 {
        return counts;
    }

    let full_mask: u32 = (1u32 << n) - 1;

    // dp_curr stores the current popcount layer in mask-major layout, packing
    // only the `p` valid endpoints per popcount-`p` mask. The endpoint offset
    // within a mask is the popcount of (mask & (bit−1)) — a single hardware
    // instruction. Layer 1 has `n` masks each with one slot.
    //
    // Per-state values can reach (len-1)! at the full mask, which exceeds
    // u64::MAX starting around n=22 — hence u128 (same rationale as `counts`).
    //
    // Source-layer indices are an incrementing counter (Gosper order ==
    // colex order for fixed popcount); destination-layer indices are
    // computed on the fly via `colex_rank`, so no `2ⁿ` lookup table is
    // allocated.
    let mut dp_curr: Vec<u128> = vec![1u128; n];
    counts[1] = n as u128;
    on_event(DpEvent::LengthDone {
        length: 1,
        count: counts[1],
    });

    // Enumerate popcount classes ascending so every proper subset of a
    // popcount-`p` mask is already final by the time we read it. Streaming
    // `LengthDone` events fire as soon as a class completes. The loop
    // stops at `max_length - 1`: the popcount-`(max_length-1)` layer is
    // the last one whose extensions contribute to `counts[max_length]`,
    // and no caller-visible state is produced at higher popcounts.
    for p in 1..max_length {
        let next_p = p + 1;
        // dp_next is only allocated when a future iteration will read from
        // it (`next_p < max_length`). At `next_p == max_length` we still
        // accumulate `counts[max_length]` from the source layer but skip
        // the dp_next writes — they would never be read.
        let need_dp_next = next_p < max_length;
        let mut dp_next: Vec<u128> = if need_dp_next {
            vec![0u128; binomial(n, next_p) as usize * next_p]
        } else {
            Vec::new()
        };

        process_layer(LayerCtx {
            n,
            full_mask,
            blocks,
            p,
            need_dp_next,
            dp_curr: &dp_curr,
            dp_next: &mut dp_next,
            counts: &mut counts,
            on_event: &mut on_event,
        });

        // All contributions to counts[p+1] came from popcount-p masks, so
        // the value is now final.
        on_event(DpEvent::LengthDone {
            length: next_p,
            count: counts[next_p],
        });

        // Hand the destination layer to the next iteration. When
        // `need_dp_next` was false, `dp_next` is empty — `dp_curr` becomes
        // empty too, releasing the previous layer's allocation early.
        dp_curr = dp_next;
    }

    counts
}

/// Bundle of state passed into [`process_layer`].
///
/// Pulled into its own struct so the helper avoids `clippy::too_many_arguments`
/// while still threading the streaming `on_event` callback through.
struct LayerCtx<'a, F: FnMut(DpEvent)> {
    n: usize,
    full_mask: u32,
    blocks: &'a [u32],
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
fn process_layer<F: FnMut(DpEvent)>(ctx: LayerCtx<'_, F>) {
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
    let mut idx_curr: u32 = 0;
    let mut mask: u32 = (1u32 << p) - 1;
    let last: u32 = mask << (n - p);
    loop {
        on_event(DpEvent::Mask);
        let base_curr = (idx_curr as usize) * p;
        let mut visited = mask;
        while visited != 0 {
            let end_bit = visited & visited.wrapping_neg();
            visited ^= end_bit;
            let end = end_bit.trailing_zeros() as usize;
            let end_off = (mask & (end_bit - 1)).count_ones() as usize;
            let ways = dp_curr[base_curr + end_off];
            if ways == 0 {
                continue;
            }

            let mut free = !mask & full_mask;
            while free != 0 {
                let next_bit = free & free.wrapping_neg();
                free ^= next_bit;
                let next = next_bit.trailing_zeros() as usize;

                let blockers = blocks[end * n + next];
                if mask & blockers == blockers {
                    counts[next_p] += ways;
                    if need_dp_next {
                        let idx_new = colex_rank(mask | next_bit) as usize;
                        // `next_bit` is not in `mask`, so the set bits of
                        // (mask | next_bit) below `next_bit` are exactly
                        // the bits of `mask` below it.
                        let next_off = (mask & (next_bit - 1)).count_ones() as usize;
                        dp_next[idx_new * next_p + next_off] += ways;
                    }
                }
            }
        }
        idx_curr = idx_curr.wrapping_add(1);
        if mask == last {
            break;
        }
        mask = gosper_next(mask);
    }
}

/// Counts every valid pattern via Iterative Deepening DFS with `O(N)` memory.
///
/// Runs one pass per target length `L = 2, 3, …, max_length`. Each pass
/// counts *only* patterns of exactly `L` nodes, so `counts[L]` is final as
/// soon as that pass ends — emitted immediately via
/// [`DfsEvent::LengthDone`] — before the next pass begins. The caller can
/// kill the process after any number of passes and retain exact counts for
/// every completed length.
///
/// `blocks[i * n + j]` must hold the bitmask of nodes that must already be
/// visited before the move `i → j` is legal (see [`crate::grid::compute_blocks`]).
///
/// `max_length` bounds the target lengths: passes run for
/// `target ∈ 2..=max_length`.
///
/// Returns a `Vec<u128>` of length `max_length + 1` where `counts[k]` is the
/// number of valid patterns of exactly `k` nodes. `counts[0] = 1`.
///
/// `u128` is used because for `n ≥ 21` the total count exceeds `u64::MAX`.
///
/// # Memory
/// `O(max_length)` — one `u128` per length slot plus a recursion stack whose
/// depth never exceeds `max_length`. No exponential table is allocated.
///
/// # Panics
/// Panics if `n > MAX_POINTS`, `blocks.len() != n * n`, or `max_length > n`.
pub fn count_patterns_dfs<F: FnMut(DfsEvent)>(
    n: usize,
    blocks: &[u32],
    max_length: usize,
    mut on_event: F,
) -> Vec<u128> {
    assert!(
        n <= MAX_POINTS,
        "N={n} exceeds the supported maximum of {MAX_POINTS}"
    );
    assert_eq!(blocks.len(), n * n, "blocks matrix must be n × n");
    assert!(
        max_length <= n,
        "max_length={max_length} must not exceed n={n}"
    );

    if blocks.iter().all(|&b| b == 0) {
        let counts = count_unconstrained(n, max_length);
        for (k, &c) in counts.iter().enumerate() {
            on_event(DfsEvent::LengthDone {
                length: k,
                count: c,
            });
        }
        return counts;
    }

    let mut counts = vec![0u128; max_length + 1];
    counts[0] = 1;
    on_event(DfsEvent::LengthDone {
        length: 0,
        count: 1,
    });
    if n == 0 || max_length == 0 {
        return counts;
    }
    counts[1] = n as u128;
    on_event(DfsEvent::LengthDone {
        length: 1,
        count: n as u128,
    });
    if max_length < 2 {
        return counts;
    }

    let full_mask: u32 = (1u32 << n) - 1;
    let pair_total = dfs_pass_ticks(n);

    for (i, count_slot) in counts[2..].iter_mut().enumerate() {
        let target = i + 2;
        on_event(DfsEvent::PassStart { target, pair_total });
        let mut count_target = 0u128;
        let mut pair_index: u64 = 0;
        for start in 0..n {
            let start_bit = 1u32 << start;
            let row = start * n;
            for second in 0..n {
                if second == start {
                    continue;
                }
                let second_bit = 1u32 << second;
                let blockers = blocks[row + second];
                if start_bit & blockers == blockers {
                    if target == 2 {
                        count_target += 1;
                    } else {
                        count_target += iddfs_count(
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
                pair_index += 1;
                on_event(DfsEvent::PassTick { target, pair_index });
            }
        }
        *count_slot = count_target;
        on_event(DfsEvent::LengthDone {
            length: target,
            count: count_target,
        });
    }

    counts
}

/// Counts patterns of exactly `target` nodes that extend the prefix
/// `(mask, end)` currently at depth `depth`. The recursion short-circuits at
/// `depth + 1 == target` to avoid any work beyond the target layer.
fn iddfs_count(
    mask: u32,
    end: usize,
    depth: usize,
    target: usize,
    blocks: &[u32],
    n: usize,
    full_mask: u32,
) -> u128 {
    let mut total = 0u128;
    let row = end * n;
    let mut free = !mask & full_mask;
    while free != 0 {
        let next_bit = free & free.wrapping_neg();
        free ^= next_bit;
        let next = next_bit.trailing_zeros() as usize;
        let blockers = blocks[row + next];
        if mask & blockers == blockers {
            if depth + 1 == target {
                total += 1;
            } else {
                total += iddfs_count(
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::grid::{GridDefinition, build_grid_definition, compute_blocks};

    fn grid(dimensions: usize, points: Vec<Vec<i32>>) -> GridDefinition {
        GridDefinition { dimensions, points }
    }

    // Runs both counters, asserts they agree, and returns the result.
    // Every test that checks output values goes through this helper so that
    // both algorithms are verified in a single pass.
    fn count(n: usize, blocks: &[u32], max_length: usize) -> Vec<u128> {
        let dp = count_patterns_dp(n, blocks, max_length, |_| {});
        let dfs = count_patterns_dfs(n, blocks, max_length, |_| {});
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
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let counts = count(n, &blocks, n);

        assert_eq!(counts[0], 1);
        assert_eq!(counts[1], 9);
        assert_eq!(counts[2], 56);
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
        let blocks = compute_blocks(&g);
        assert!(blocks.iter().all(|&b| b == 0));
        let n = g.points.len();
        assert_eq!(count(n, &blocks, n), vec![1, 4, 12, 24, 24]);
    }

    #[test]
    fn blocker_becomes_transparent_once_visited() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![2, 0]]);
        let blocks = compute_blocks(&g);
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
        let blocks = compute_blocks(&empty);
        assert!(blocks.is_empty());
        assert_eq!(count(0, &blocks, 0), vec![1]);

        let single = grid(2, vec![vec![7, 7]]);
        single.validate().unwrap();
        let blocks = compute_blocks(&single);
        assert_eq!(blocks, vec![0]);
        assert_eq!(count(1, &blocks, 1), vec![1, 1]);
    }

    #[test]
    fn generated_3x3_matches_known_pattern_counts() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let counts = count(n, &blocks, n);
        assert_eq!(counts[4..=9].iter().sum::<u128>(), 389_112);
    }

    #[test]
    fn max_length_truncates_counts_to_prefix_of_full_run() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
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
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        assert_eq!(n, 21);
        let counts = count_patterns_dp(n, &blocks, n, |_| {});
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
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        assert_eq!(n, 24);
        let counts = count_patterns_dp(n, &blocks, n, |_| {});
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
        let blocks = compute_blocks(&g);
        assert_eq!(count(g.points.len(), &blocks, 0), vec![1]);
    }

    #[test]
    fn max_length_one_reports_only_singletons() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        assert_eq!(count(g.points.len(), &blocks, 1), vec![1, 9]);
    }

    #[test]
    fn max_length_four_matches_android_minimum_run() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
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
        let blocks = compute_blocks(&g);
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
        let blocks = compute_blocks(&g);
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

    // LengthDone must fire for every slot of the returned vector with values
    // matching the vector, in strictly ascending order.
    #[test]
    fn dfs_length_done_events_match_returned_counts() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let seen = std::cell::RefCell::new(Vec::<(usize, u128)>::new());
        let counts = count_patterns_dfs(n, &blocks, n, |event| {
            if let DfsEvent::LengthDone { length, count } = event {
                seen.borrow_mut().push((length, count));
            }
        });
        let expected: Vec<(usize, u128)> = counts.iter().copied().enumerate().collect();
        assert_eq!(*seen.borrow(), expected);
    }

    #[test]
    fn dfs_length_done_events_fire_on_fast_path() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![1, 1], vec![0, 1]]);
        let blocks = compute_blocks(&g);
        assert!(blocks.iter().all(|&b| b == 0));
        let n = g.points.len();
        let seen = std::cell::RefCell::new(Vec::<(usize, u128)>::new());
        let counts = count_patterns_dfs(n, &blocks, n, |event| {
            if let DfsEvent::LengthDone { length, count } = event {
                seen.borrow_mut().push((length, count));
            }
        });
        let expected: Vec<(usize, u128)> = counts.iter().copied().enumerate().collect();
        assert_eq!(*seen.borrow(), expected);
    }

    #[test]
    fn iddfs_length_done_events_fire_in_ascending_order() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let seen = std::cell::RefCell::new(Vec::<usize>::new());
        let counts = count_patterns_dfs(n, &blocks, n, |event| {
            if let DfsEvent::LengthDone { length, count } = event {
                seen.borrow_mut().push(length);
                assert_eq!(count, counts_oracle(&g)[length]);
            }
        });
        let expected: Vec<usize> = (0..=n).collect();
        assert_eq!(*seen.borrow(), expected);
        let _ = counts;
    }

    // Oracle for iddfs_length_done_events_fire_in_ascending_order.
    fn counts_oracle(g: &crate::grid::GridDefinition) -> Vec<u128> {
        let blocks = compute_blocks(g);
        count_patterns_dp(g.points.len(), &blocks, g.points.len(), |_| {})
    }

    #[derive(Debug, PartialEq)]
    enum PassEv {
        Start(usize),
        Tick(usize),
        Done(usize),
    }

    #[test]
    fn iddfs_pass_start_precedes_pass_ticks_and_length_done_for_each_target() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let log = std::cell::RefCell::new(Vec::<PassEv>::new());
        count_patterns_dfs(n, &blocks, n, |event| match event {
            DfsEvent::PassStart { target, .. } => log.borrow_mut().push(PassEv::Start(target)),
            DfsEvent::PassTick { target, .. } => log.borrow_mut().push(PassEv::Tick(target)),
            DfsEvent::LengthDone { length, .. } => log.borrow_mut().push(PassEv::Done(length)),
        });

        let log = log.into_inner();
        // Lengths 0 and 1 emit Done before the first pass.
        assert_eq!(log[0], PassEv::Done(0));
        assert_eq!(log[1], PassEv::Done(1));

        // For each target ≥ 2: one Start, then pair_total Ticks, then one Done.
        let pair_total = n * (n - 1);
        let mut pos = 2usize;
        for target in 2..=n {
            assert_eq!(log[pos], PassEv::Start(target), "target={target}");
            pos += 1;
            for _ in 0..pair_total {
                assert_eq!(log[pos], PassEv::Tick(target), "target={target}");
                pos += 1;
            }
            assert_eq!(log[pos], PassEv::Done(target), "target={target}");
            pos += 1;
        }
        assert_eq!(pos, log.len());
    }

    #[test]
    fn iddfs_pass_tick_count_matches_pair_total() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let expected_per_pass = dfs_pass_ticks(n);

        let current_total = std::cell::Cell::new(0u64);
        count_patterns_dfs(n, &blocks, n, |event| match event {
            DfsEvent::PassStart { pair_total, .. } => {
                assert_eq!(pair_total, expected_per_pass);
                current_total.set(0);
            }
            DfsEvent::PassTick { pair_index, .. } => {
                current_total.set(pair_index);
            }
            DfsEvent::LengthDone { .. } => {
                // After Done, the tick counter must have reached pair_total
                // (only meaningful for passes ≥ 2 where a Start was emitted).
            }
        });
        // Final pass must have ticked all the way to pair_total.
        assert_eq!(current_total.get(), expected_per_pass);
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
        // n = 31, full run: peak DP layer pair plus the 2³¹ · 4 B index table.
        // Must not saturate to u64::MAX — we only need a finite, sensible bound.
        let bytes = dp_table_bytes(31, 31);
        assert!(bytes < u64::MAX);
        assert!(bytes > (1u64 << 30), "expected at least 1 GiB, got {bytes}");
    }

    // Hand-verified expected sizes for n = 3 across every max_length.
    //   p=1: dp_curr always = n = 3 entries (init).
    //   p>=2: dp_curr = C(n,p)·p iff p < l, dp_next = C(n,p+1)·(p+1) iff p+1 < l.
    //   counts = (l+1)·16 B. No mask→index lookup table — destination
    //   indices come from `colex_rank`.
    #[test]
    fn dp_table_bytes_n3_known_values() {
        // max_length = 0 → early exit, counts only (1·16 = 16 B).
        assert_eq!(dp_table_bytes(3, 0), 16);
        // max_length = 1 → peak = 3 entries (48 B) + counts 32 B.
        assert_eq!(dp_table_bytes(3, 1), 48 + 32);
        // max_length = 2 → same peak = 3 entries (no dp_next allocated).
        //   counts = 3·16 = 48 B.
        assert_eq!(dp_table_bytes(3, 2), 48 + 48);
        // max_length = 3 → peak at p=1 = 3 + C(3,2)·2 = 3 + 6 = 9 entries (144 B).
        //   counts = 4·16 = 64 B.
        assert_eq!(dp_table_bytes(3, 3), 144 + 64);
    }

    // Regression: a previous implementation allocated a `2ⁿ × u32` mask→index
    // table during the DP regardless of `max_length`. For `n = 28` that table
    // alone weighed exactly 1 GiB, so `grid 3x3x2 -f 10 --max-length 4`
    // (n = 28, peak DP layers ≈ 169 KiB) was incorrectly routed to DFS under
    // the default 1 GiB budget. With the colex-rank rewrite the index table
    // is gone, and the call must select DP — even with a far tighter budget.
    #[test]
    fn choose_algorithm_picks_dp_for_high_n_with_tight_max_length() {
        let one_gib: u64 = 1024 * 1024 * 1024;
        assert_eq!(choose_algorithm(28, 4, one_gib), Algorithm::Dp);
        // 1 MiB still leaves 5+ orders of magnitude of headroom over the
        // ~169 KiB the DP actually needs.
        assert_eq!(choose_algorithm(28, 4, 1024 * 1024), Algorithm::Dp);
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
                mask = gosper_next(mask);
                expected += 1;
            }
        }
    }

    #[test]
    fn choose_algorithm_picks_dp_when_capped_max_length_fits() {
        // n = 24 with max_length = 24 needs roughly 1 GB.
        // The mask→index table alone costs 2²⁴·4 B = 64 MiB regardless of
        // max_length, so the budget must comfortably exceed that for the
        // capped run to fit. 256 MiB does it.
        let budget: u64 = 256 * 1024 * 1024;
        assert_eq!(choose_algorithm(24, 24, budget), Algorithm::Dfs);
        // …but the same n with a tight max_length cap fits comfortably,
        // since `dp_next` is never allocated past the cap.
        assert_eq!(choose_algorithm(24, 4, budget), Algorithm::Dp);
    }

    #[test]
    fn choose_algorithm_zero_budget_always_picks_dfs_for_constrained_grids() {
        // With a zero budget DP can never fit (at minimum it allocates the
        // counts vector + index table), so the router must fall back to DFS.
        for n in 1..=8 {
            assert_eq!(choose_algorithm(n, n, 0), Algorithm::Dfs);
        }
    }

    #[test]
    fn choose_algorithm_huge_budget_always_picks_dp() {
        for n in 0..=15 {
            assert_eq!(choose_algorithm(n, n, u64::MAX), Algorithm::Dp);
        }
    }

    // The constrained DP must emit exactly `dp_mask_ticks(n, max_length)`
    // `Mask` events — the contract the CLI relies on to size its progress
    // bar so it reaches 100% precisely when the run finishes. Sweep across
    // every `max_length` cap to cover the early-exit branches as well.
    #[test]
    fn dp_mask_event_count_matches_dp_mask_ticks() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        assert!(
            blocks.iter().any(|&b| b != 0),
            "3×3 grid must be constrained for this test to exercise the popcount loop"
        );
        let n = g.points.len();
        for cap in 0..=n {
            let mask_count = std::cell::Cell::new(0u64);
            count_patterns_dp(n, &blocks, cap, |event| {
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
        let blocks = compute_blocks(&g);
        assert!(blocks.iter().all(|&b| b == 0));
        let n = g.points.len();
        let mask_count = std::cell::Cell::new(0u64);
        count_patterns_dp(n, &blocks, n, |event| {
            if matches!(event, DpEvent::Mask) {
                mask_count.set(mask_count.get() + 1);
            }
        });
        assert_eq!(mask_count.get(), 0);
    }

    #[test]
    fn iddfs_no_pass_events_on_unconstrained_fast_path() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![1, 1], vec![0, 1]]);
        let blocks = compute_blocks(&g);
        assert!(blocks.iter().all(|&b| b == 0));
        let n = g.points.len();
        let pass_starts = std::cell::Cell::new(0usize);
        let pass_ticks = std::cell::Cell::new(0usize);
        count_patterns_dfs(n, &blocks, n, |event| match event {
            DfsEvent::PassStart { .. } => pass_starts.set(pass_starts.get() + 1),
            DfsEvent::PassTick { .. } => pass_ticks.set(pass_ticks.get() + 1),
            DfsEvent::LengthDone { .. } => {}
        });
        assert_eq!(pass_starts.get(), 0);
        assert_eq!(pass_ticks.get(), 0);
    }
}
