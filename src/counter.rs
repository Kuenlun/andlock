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
    /// One outer-loop mask has been processed (fired `2ⁿ − 1` times during the
    /// constrained DP; not fired at all on the unconstrained fast path).
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

/// Returns the number of bytes the DP table would occupy for `n` nodes.
///
/// Formula: `2ⁿ × n × 16` (each of the `2ⁿ × n` entries is a `u128`).
/// Uses saturating arithmetic so callers can safely pass any `n`.
#[must_use]
pub fn dp_table_bytes(n: usize) -> u64 {
    (1u64 << n.min(63))
        .saturating_mul(n as u64)
        .saturating_mul(16)
}

/// Returns [`Algorithm::Dp`] if the DP table fits within `memory_budget`
/// bytes, otherwise [`Algorithm::Dfs`].
#[must_use]
pub fn choose_algorithm(n: usize, memory_budget: u64) -> Algorithm {
    if dp_table_bytes(n) <= memory_budget {
        Algorithm::Dp
    } else {
        Algorithm::Dfs
    }
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

/// Counts every valid pattern via bottom-up bitmask dynamic programming.
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
/// # Complexity
/// With `L = max_length`, extension work is bounded by the prefixes of length
/// `< L`, so the runtime shrinks from the full `O(N² · 2ᴺ)` to
/// `O(N² · Σ_{k<L} C(N, k))` when `L < N`.
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

    let num_masks: usize = 1 << n;
    let full_mask: u32 = (1u32 << n) - 1;

    // dp[mask * n + node] = number of valid orderings that visit exactly the
    // set encoded by `mask` and end at `node`. Mask-major layout keeps the
    // inner scan over endpoints contiguous in memory.
    //
    // Per-state values can reach (len-1)! at the full mask, which exceeds
    // u64::MAX starting around n=22 — hence u128 (same rationale as `counts`).
    let mut dp = vec![0u128; num_masks * n];

    for v in 0..n {
        dp[(1usize << v) * n + v] = 1;
    }
    counts[1] = n as u128;
    on_event(DpEvent::LengthDone {
        length: 1,
        count: counts[1],
    });

    // Enumerate masks grouped by popcount (ascending). Every proper subset of
    // a popcount-p mask has popcount < p, so subset states are still
    // guaranteed to be populated before use. Grouping by popcount lets us
    // report `counts[p+1]` the moment the last popcount-p mask is processed,
    // enabling streaming output without waiting for the full run.
    //
    // Masks whose length already equals `max_length` cannot be extended into
    // a counted pattern, so their inner loops are skipped — but they are
    // still ticked so the caller's mask-granular progress bar reaches 100%.
    for p in 1..=n {
        let mut mask: u32 = (1u32 << p) - 1;
        let last: u32 = mask << (n - p);
        loop {
            on_event(DpEvent::Mask);
            if p < max_length {
                let base = (mask as usize) * n;

                let mut visited = mask;
                while visited != 0 {
                    let end_bit = visited & visited.wrapping_neg();
                    visited ^= end_bit;
                    let end = end_bit.trailing_zeros() as usize;

                    let ways = dp[base + end];
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
                            counts[p + 1] += ways;
                            // Writes into the terminal layer would never be
                            // read, since masks of length `max_length` are
                            // skipped by the outer `p < max_length` guard.
                            if p + 1 < max_length {
                                let new_mask = (mask | next_bit) as usize;
                                dp[new_mask * n + next] += ways;
                            }
                        }
                    }
                }
            }
            if mask == last {
                break;
            }
            mask = gosper_next(mask);
        }
        // All contributions to counts[p+1] came from popcount-p masks, so the
        // value is now final.
        if p < max_length {
            on_event(DpEvent::LengthDone {
                length: p + 1,
                count: counts[p + 1],
            });
        }
    }

    counts
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

    // Regression test: with n=21 the sum of patterns exceeds u64::MAX (≈1.84×10¹⁹)
    // because 21! ≈ 5.1×10¹⁹. Before the fix, `counts` used u64 and panicked with
    // "attempt to add with overflow" on `grid 4x4 -f 5`.
    // This test requires ~700 MB of DP table.
    #[test]
    #[ignore = "allocates ~700 MB — run manually with: cargo test -- --ignored"]
    fn count_4x4_plus_5_free_does_not_overflow() {
        let g = build_grid_definition(&[4, 4], 5);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        assert_eq!(n, 21);
        let counts = count_patterns_dp(n, &blocks, n, |_| {});
        // In a zero-blocker graph, counts[n] = n!; free points guarantee many
        // paths exceed u64::MAX, so u128 is necessary.
        assert!(
            counts[n] > u128::from(u64::MAX),
            "expected counts[21] to exceed u64::MAX but got {}",
            counts[n]
        );
    }

    // Regression test for silent overflow in the DP table itself. At n=24, the
    // per-endpoint `ways` stored at mask length 23 reaches ≈21! ≈ 5.1×10¹⁹,
    // which wraps in u64 and produces counts[24] < counts[23] — a monotonicity
    // violation that is provably impossible (every full pattern of length 24
    // extends some length-23 prefix). We assert on every suffix length to lock
    // the invariant in place.
    //
    // This allocates ~6.4 GB for the DP table, so it is ignored by default.
    #[test]
    #[ignore = "allocates ~6.4 GB — run manually with: cargo test -- --ignored"]
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
