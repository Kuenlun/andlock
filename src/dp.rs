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

/// Counts every valid pattern via bottom-up bitmask dynamic programming.
///
/// `blocks[i * n + j]` must hold the bitmask of nodes that must already be
/// visited before the move `i → j` is legal (see [`crate::grid::compute_blocks`]).
///
/// Returns a `Vec<u64>` of length `n + 1` where `counts[k]` is the number of
/// valid patterns of exactly `k` nodes. `counts[0] = 1` (the empty pattern).
///
/// # Complexity
/// - Time: `O(N² · 2ᴺ)`
/// - Space: `O(N · 2ᴺ)`
///
/// # Panics
/// Panics if `n > MAX_POINTS` or `blocks.len() != n * n`.
pub fn count_patterns_dp(n: usize, blocks: &[u32]) -> Vec<u64> {
    assert!(
        n <= MAX_POINTS,
        "N={n} exceeds the DP limit of {MAX_POINTS}"
    );
    assert_eq!(blocks.len(), n * n, "blocks matrix must be n × n");

    let mut counts = vec![0u64; n + 1];
    counts[0] = 1;
    if n == 0 {
        return counts;
    }

    let num_masks: usize = 1 << n;
    let full_mask: u32 = (1u32 << n) - 1;

    // dp[mask * n + node] = number of valid orderings that visit exactly the
    // set encoded by `mask` and end at `node`. Mask-major layout keeps the
    // inner scan over endpoints contiguous in memory.
    let mut dp = vec![0u64; num_masks * n];

    for v in 0..n {
        dp[(1usize << v) * n + v] = 1;
    }
    counts[1] = n as u64;

    // Enumerate masks in ascending order so all proper-subset states are
    // already populated by the time we reach `mask`.
    for mask in 1u32..=full_mask {
        let base = (mask as usize) * n;
        let len = mask.count_ones() as usize;

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
                    let new_mask = (mask | next_bit) as usize;
                    dp[new_mask * n + next] += ways;
                    counts[len + 1] += ways;
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
        let counts = count_patterns_dp(g.points.len(), &blocks);

        assert_eq!(counts[0], 1);
        assert_eq!(counts[1], 9);
        assert_eq!(counts[2], 56);
        assert_eq!(counts[4], 1_624);
        assert_eq!(counts[5], 7_152);
        assert_eq!(counts[6], 26_016);
        assert_eq!(counts[7], 72_912);
        assert_eq!(counts[8], 140_704);
        assert_eq!(counts[9], 140_704);
        assert_eq!(counts[4..=9].iter().sum::<u64>(), 389_112);
    }

    #[test]
    fn no_three_collinear_collapses_to_permutations() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![1, 1], vec![0, 1]]);
        let blocks = compute_blocks(&g);
        assert!(blocks.iter().all(|&b| b == 0));
        let counts = count_patterns_dp(g.points.len(), &blocks);
        assert_eq!(counts, vec![1, 4, 12, 24, 24]);
    }

    #[test]
    fn blocker_becomes_transparent_once_visited() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![2, 0]]);
        let blocks = compute_blocks(&g);
        let counts = count_patterns_dp(g.points.len(), &blocks);

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
        assert_eq!(count_patterns_dp(0, &blocks), vec![1]);

        let single = grid(2, vec![vec![7, 7]]);
        single.validate().unwrap();
        let blocks = compute_blocks(&single);
        assert_eq!(blocks, vec![0]);
        assert_eq!(count_patterns_dp(1, &blocks), vec![1, 1]);
    }

    #[test]
    fn generated_3x3_matches_known_pattern_counts() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let counts = count_patterns_dp(g.points.len(), &blocks);
        assert_eq!(counts[4..=9].iter().sum::<u64>(), 389_112);
    }
}
