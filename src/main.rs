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

use std::error::Error;
use std::time::Instant;

use serde::Deserialize;

/// Upper bound on the number of nodes the program accepts.
///
/// [`count_patterns_dp`] represents the visited set with a `u32` bitmask and
/// allocates a `2ⁿ × n` table of `u64` counts. At `n = 25` the table already
/// reaches ~6.7 GiB, which we treat as the ceiling of what is realistic to
/// run on a workstation.
const MAX_POINTS: usize = 25;

/// Finite set of integer-coordinate nodes in `dimensions`-dimensional space.
#[derive(Deserialize)]
struct GridDefinition {
    dimensions: usize,
    points: Vec<Vec<i32>>,
}

impl GridDefinition {
    /// Validates the structural invariants required by the rest of the pipeline.
    ///
    /// # Errors
    /// Returns an error if the point count exceeds [`MAX_POINTS`] or any point
    /// does not have exactly [`GridDefinition::dimensions`] coordinates.
    fn validate(&self) -> Result<(), String> {
        let n = self.points.len();
        if n > MAX_POINTS {
            return Err(format!(
                "{n} points exceeds the supported maximum of {MAX_POINTS}"
            ));
        }
        for (idx, point) in self.points.iter().enumerate() {
            if point.len() != self.dimensions {
                return Err(format!(
                    "point {idx} has {actual} coordinate(s); expected {expected}",
                    actual = point.len(),
                    expected = self.dimensions,
                ));
            }
        }
        Ok(())
    }
}

/// Builds the blocking-constraint matrix for the given grid.
///
/// Returns a flat `n × n` row-major matrix where `blocks[a * n + b]` is the
/// bitmask of nodes lying strictly on the open segment `(a, b)` — the nodes
/// that must already be visited for a direct move `a → b` to be legal.
///
/// The matrix is symmetric: `blocks[a * n + b] == blocks[b * n + a]`.
fn compute_blocks(grid: &GridDefinition) -> Vec<u32> {
    let n = grid.points.len();
    let dim = grid.dimensions;
    let mut blocks = vec![0u32; n * n];

    for a in 0..n {
        let origin = &grid.points[a];
        // Each unordered pair is processed once; the relation is symmetric.
        for b in (a + 1)..n {
            let target = &grid.points[b];

            for (c, probe) in grid.points.iter().enumerate() {
                if c == a || c == b {
                    continue;
                }

                // Bounding-box prefilter: the probe must lie inside the
                // axis-aligned box spanned by AB in every dimension.
                let in_box = (0..dim).all(|i| {
                    let lo = origin[i].min(target[i]);
                    let hi = origin[i].max(target[i]);
                    lo <= probe[i] && probe[i] <= hi
                });
                if !in_box {
                    continue;
                }

                // Collinearity: AC and AB must be parallel. In n dimensions
                // this is equivalent to every pairwise 2-D cross product
                // vanishing. Promotion to i64 avoids overflow on the product
                // of two i32 coordinate differences.
                let collinear = (0..dim).all(|i| {
                    ((i + 1)..dim).all(|j| {
                        let dx = i64::from(target[i] - origin[i]);
                        let dy = i64::from(target[j] - origin[j]);
                        let ex = i64::from(probe[i] - origin[i]);
                        let ey = i64::from(probe[j] - origin[j]);
                        ex * dy == ey * dx
                    })
                });

                if collinear {
                    let c_bit = 1u32 << c;
                    blocks[a * n + b] |= c_bit;
                    blocks[b * n + a] |= c_bit;
                }
            }
        }
    }

    blocks
}

/// Counts every valid pattern via bottom-up bitmask dynamic programming.
///
/// `blocks[i * n + j]` must hold the bitmask of nodes that must already be
/// visited before the move `i → j` is legal (see [`compute_blocks`]).
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
fn count_patterns_dp(n: usize, blocks: &[u32]) -> Vec<u64> {
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
    // set encoded by `mask` and end at `node`. The mask-major layout keeps
    // the inner scan over endpoints within a mask contiguous in memory.
    let mut dp = vec![0u64; num_masks * n];

    // Seed every length-1 state: a pattern visiting only `v` and ending at `v`.
    for v in 0..n {
        dp[(1usize << v) * n + v] = 1;
    }
    counts[1] = n as u64;

    // Enumerate masks in ascending order. Any proper subset of `mask` has a
    // strictly smaller value, so all prerequisite states are already set by
    // the time we reach `mask`.
    for mask in 1u32..=full_mask {
        let base = (mask as usize) * n;
        let len = mask.count_ones() as usize;

        // Walk every bit set in `mask` — each is a candidate endpoint.
        let mut visited = mask;
        while visited != 0 {
            let end_bit = visited & visited.wrapping_neg();
            visited ^= end_bit;
            let end = end_bit.trailing_zeros() as usize;

            let ways = dp[base + end];
            if ways == 0 {
                continue;
            }

            // Walk every bit NOT in `mask` — each is a candidate next node.
            let mut free = !mask & full_mask;
            while free != 0 {
                let next_bit = free & free.wrapping_neg();
                free ^= next_bit;
                let next = next_bit.trailing_zeros() as usize;

                // A move is legal iff every blocker is already visited.
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

fn main() -> Result<(), Box<dyn Error>> {
    // Classic 3×3 Android unlock grid.
    let json_input = r#"{
        "dimensions": 2,
        "points": [
            [0, 0], [1, 0], [2, 0],
            [0, 1], [1, 1], [2, 1],
            [0, 2], [1, 2], [2, 2]
        ]
    }"#;

    let grid: GridDefinition = serde_json::from_str(json_input)?;
    grid.validate()?;

    let n = grid.points.len();
    let dim = grid.dimensions;
    println!("Computing block constraints for {n} points in {dim}D...");

    let t0 = Instant::now();
    let blocks = compute_blocks(&grid);
    println!("Block matrix computed in {:?}\n", t0.elapsed());

    println!("Computing valid patterns for {n} points...");
    let t1 = Instant::now();
    let counts = count_patterns_dp(n, &blocks);
    let elapsed = t1.elapsed();

    let total: u64 = counts.iter().sum();
    for (k, c) in counts.iter().enumerate() {
        if *c > 0 {
            println!("  Length {k:>2}: {c}");
        }
    }
    println!("───────────────────────────");
    println!("  Total: {total}");
    println!("  Time:  {elapsed:?}");

    Ok(())
}
