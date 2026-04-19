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
pub fn count_patterns_dp(
    n: usize,
    blocks: &[u32],
    max_length: usize,
    on_mask: impl Fn(),
) -> Vec<u128> {
    assert!(
        n <= MAX_POINTS,
        "N={n} exceeds the DP limit of {MAX_POINTS}"
    );
    assert_eq!(blocks.len(), n * n, "blocks matrix must be n × n");
    assert!(
        max_length <= n,
        "max_length={max_length} must not exceed n={n}"
    );

    if blocks.iter().all(|&b| b == 0) {
        return count_unconstrained(n, max_length);
    }

    let mut counts = vec![0u128; max_length + 1];
    counts[0] = 1;
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

    // Enumerate masks in ascending order so all proper-subset states are
    // already populated by the time we reach `mask`. Masks whose length
    // already equals `max_length` cannot be extended into a counted pattern,
    // so their inner loops are skipped entirely.
    for mask in 1u32..=full_mask {
        on_mask();
        let len = mask.count_ones() as usize;
        if len >= max_length {
            continue;
        }
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
                    counts[len + 1] += ways;
                    // Writes into the terminal layer would never be read,
                    // since masks of length `max_length` are skipped above.
                    if len + 1 < max_length {
                        let new_mask = (mask | next_bit) as usize;
                        dp[new_mask * n + next] += ways;
                    }
                }
            }
        }
    }

    counts
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::grid::{GridDefinition, build_grid_definition, compute_blocks};

    fn grid(dimensions: usize, points: Vec<Vec<i32>>) -> GridDefinition {
        GridDefinition { dimensions, points }
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
        let counts = count_patterns_dp(n, &blocks, n, || {});

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
        let counts = count_patterns_dp(n, &blocks, n, || {});
        assert_eq!(counts, vec![1, 4, 12, 24, 24]);
    }

    #[test]
    fn blocker_becomes_transparent_once_visited() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![2, 0]]);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let counts = count_patterns_dp(n, &blocks, n, || {});

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
        assert_eq!(count_patterns_dp(0, &blocks, 0, || {}), vec![1]);

        let single = grid(2, vec![vec![7, 7]]);
        single.validate().unwrap();
        let blocks = compute_blocks(&single);
        assert_eq!(blocks, vec![0]);
        assert_eq!(count_patterns_dp(1, &blocks, 1, || {}), vec![1, 1]);
    }

    #[test]
    fn generated_3x3_matches_known_pattern_counts() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let counts = count_patterns_dp(n, &blocks, n, || {});
        assert_eq!(counts[4..=9].iter().sum::<u128>(), 389_112);
    }

    #[test]
    fn max_length_truncates_counts_to_prefix_of_full_run() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let full = count_patterns_dp(n, &blocks, n, || {});
        for cap in 0..=n {
            let capped = count_patterns_dp(n, &blocks, cap, || {});
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
        let counts = count_patterns_dp(n, &blocks, n, || {});
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
        let counts = count_patterns_dp(n, &blocks, n, || {});
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
        let counts = count_patterns_dp(g.points.len(), &blocks, 0, || {});
        assert_eq!(counts, vec![1]);
    }

    #[test]
    fn max_length_one_reports_only_singletons() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let counts = count_patterns_dp(g.points.len(), &blocks, 1, || {});
        assert_eq!(counts, vec![1, 9]);
    }

    #[test]
    fn max_length_four_matches_android_minimum_run() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let counts = count_patterns_dp(g.points.len(), &blocks, 4, || {});
        assert_eq!(counts[4], 1_624);
    }

    // Verify the closed-form formula P(n,k) = n!/(n-k)! is mathematically
    // correct for n = 0..=7 by checking against the falling-factorial definition.
    #[test]
    fn unconstrained_formula_is_falling_factorial() {
        for n in 0..=7usize {
            let zero_blocks = vec![0u32; n * n];
            let counts = count_patterns_dp(n, &zero_blocks, n, || {});
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
        let counts = count_patterns_dp(n, &blocks, n, || {});
        // P(4, k) for k=0..4: [1, 4, 12, 24, 24]
        assert_eq!(counts, vec![1, 4, 12, 24, 24]);
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
        let counts = count_patterns_dp(n, &blocks, n, || {});
        // Constrained: only 4 valid length-3 patterns, not 3! = 6.
        assert_eq!(counts[3], 4);
        // Unconstrained formula would give P(3,3) = 6.
        assert_ne!(counts[3], 6);
    }
}
