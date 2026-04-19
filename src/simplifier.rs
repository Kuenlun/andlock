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

//! Pattern simplification passes.
//!
//! Each pass takes a [`GridDefinition`] by reference and returns an owned
//! [`GridDefinition`] that is *geometrically equivalent* to the input
//! (identical collinearity, direction and visibility relationships) but has
//! smaller coordinates. Because every pass has that same shape, outputs can
//! be piped back through any simplifier — the same one or another — to form
//! arbitrary chains.
//!
//! All passes are idempotent: `f(f(g))` has the same coordinates as `f(g)`.

use crate::grid::GridDefinition;

/// Runs every simplification pass in canonical order and returns the
/// resulting grid.
///
/// This is the single source of truth for "what does *canonical form* mean"
/// in this crate. Grid constructors and the CLI both funnel through here so
/// that adding a new pass automatically propagates everywhere a canonical
/// grid is expected.
///
/// The pipeline is a composition of individually idempotent, count-preserving
/// passes, so the result is itself idempotent: `canonicalize(canonicalize(g))`
/// has the same coordinates as `canonicalize(g)`.
#[must_use]
pub fn canonicalize(grid: &GridDefinition) -> GridDefinition {
    compress_axes(&translate_to_origin(grid))
}

/// Step 1 — translate the pattern so that the node closest to its centroid
/// lands at the origin.
///
/// Translation is an isometry, so distances to the centroid are preserved;
/// applying the pass a second time finds the same anchor — already at the
/// origin — and leaves the grid untouched.
///
/// Ties are broken by lowest index, keeping the result deterministic.
#[must_use]
pub fn translate_to_origin(grid: &GridDefinition) -> GridDefinition {
    pick_centroid_anchor(grid).map_or_else(
        || grid.clone(),
        |idx| {
            let anchor = grid.points[idx].clone();
            translate_by(grid, &anchor)
        },
    )
}

/// Step 2 — compress every axis independently by dividing its coordinates
/// by the greatest common divisor of their absolute values.
///
/// After one pass, each axis either has a GCD of 1 (already at its coarsest
/// integer lattice) or is entirely zero; re-running the pass is a no-op.
/// Collinearity, direction and visibility are untouched because scaling
/// individual axes by a non-zero constant preserves all three.
#[must_use]
pub fn compress_axes(grid: &GridDefinition) -> GridDefinition {
    let gcds: Vec<i64> = (0..grid.dimensions)
        .map(|axis| axis_gcd(grid, axis))
        .collect();

    let points = grid
        .points
        .iter()
        .map(|p| {
            p.iter()
                .zip(gcds.iter())
                .map(|(&coord, &g)| divide_exact(coord, g))
                .collect()
        })
        .collect();

    GridDefinition {
        dimensions: grid.dimensions,
        points,
    }
}

/// Index of the node minimising the squared Euclidean distance to the
/// centroid, or `None` for an empty grid.
///
/// The comparison is carried out in integer arithmetic: for `n` points the
/// centroid has coordinates `sum / n`, and `(n · pᵢⱼ − sumⱼ)² = n² · (pᵢⱼ −
/// centroidⱼ)²`. Scaling every candidate by the same positive `n²` keeps
/// the ordering, so we never need floating point.
fn pick_centroid_anchor(grid: &GridDefinition) -> Option<usize> {
    if grid.points.is_empty() {
        return None;
    }
    let n = i128::try_from(grid.points.len()).unwrap_or(0);
    let sums: Vec<i128> = (0..grid.dimensions)
        .map(|axis| grid.points.iter().map(|p| i128::from(p[axis])).sum())
        .collect();

    grid.points
        .iter()
        .enumerate()
        .min_by_key(|(_, p)| {
            p.iter()
                .zip(sums.iter())
                .map(|(&coord, &sum)| {
                    let diff = i128::from(coord) * n - sum;
                    diff * diff
                })
                .sum::<i128>()
        })
        .map(|(idx, _)| idx)
}

/// Returns a new grid obtained by subtracting `offset` from every node.
fn translate_by(grid: &GridDefinition, offset: &[i32]) -> GridDefinition {
    let points = grid
        .points
        .iter()
        .map(|p| p.iter().zip(offset.iter()).map(|(&c, &o)| c - o).collect())
        .collect();

    GridDefinition {
        dimensions: grid.dimensions,
        points,
    }
}

/// GCD of the absolute values of every coordinate on `axis`, skipping zeros.
///
/// Returns `0` when the axis is entirely zero (nothing to divide by); this
/// is treated as a no-op sentinel by [`divide_exact`].
fn axis_gcd(grid: &GridDefinition, axis: usize) -> i64 {
    grid.points
        .iter()
        .filter_map(|p| {
            let v = i64::from(p[axis]).unsigned_abs();
            (v != 0).then_some(v)
        })
        .reduce(gcd)
        .and_then(|g| i64::try_from(g).ok())
        .unwrap_or(0)
}

/// Euclidean algorithm on non-negative integers.
const fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Divides `coord` by a positive `divisor` that is guaranteed to divide it
/// exactly. Values of `divisor <= 1` are a no-op (either the axis is all
/// zero or already at its coarsest lattice).
fn divide_exact(coord: i32, divisor: i64) -> i32 {
    if divisor <= 1 {
        return coord;
    }
    // `divisor` divides `coord` exactly and `|coord / divisor| <= |coord|`,
    // so the quotient always fits back in i32.
    let reduced = i64::from(coord) / divisor;
    i32::try_from(reduced).unwrap_or(coord)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::dp::count_patterns_dp;
    use crate::grid::compute_blocks;

    fn grid(dimensions: usize, points: Vec<Vec<i32>>) -> GridDefinition {
        GridDefinition { dimensions, points }
    }

    fn counts_of(g: &GridDefinition) -> Vec<u128> {
        g.validate().unwrap();
        let blocks = compute_blocks(g);
        let n = g.points.len();
        count_patterns_dp(n, &blocks, n, || {})
    }

    /// 3×3 grid translated by (10, 20) and scaled by 3 on both axes.
    fn scaled_shifted_3x3() -> GridDefinition {
        let mut points = Vec::with_capacity(9);
        for y in 0..3 {
            for x in 0..3 {
                points.push(vec![10 + 3 * x, 20 + 3 * y]);
            }
        }
        grid(2, points)
    }

    /// 3D sample with per-axis GCDs of 5, 7 and 1.
    fn mixed_gcd_3d() -> GridDefinition {
        grid(
            3,
            vec![
                vec![0, 0, 0],
                vec![5, 7, 1],
                vec![10, 14, 2],
                vec![0, 7, 3],
                vec![5, 0, 4],
            ],
        )
    }

    #[test]
    fn step1_places_a_node_at_the_origin() {
        let g = scaled_shifted_3x3();
        let s = translate_to_origin(&g);
        assert!(
            s.points.iter().any(|p| p.iter().all(|&c| c == 0)),
            "no node was translated to the origin: {:?}",
            s.points
        );
    }

    #[test]
    fn step1_picks_the_node_closest_to_the_centroid() {
        // Centroid of this grid is (1, 0); point index 1 is exactly on it.
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![2, 0]]);
        let s = translate_to_origin(&g);
        assert_eq!(s.points, vec![vec![-1, 0], vec![0, 0], vec![1, 0]]);
    }

    #[test]
    fn step1_ties_resolve_to_the_lowest_index() {
        // Two nodes are equidistant from the centroid (0.5, 0); pick index 0.
        let g = grid(2, vec![vec![0, 0], vec![1, 0]]);
        let s = translate_to_origin(&g);
        assert_eq!(s.points, vec![vec![0, 0], vec![1, 0]]);
    }

    #[test]
    fn step2_divides_each_axis_by_its_gcd() {
        let g = mixed_gcd_3d();
        let s = compress_axes(&g);
        assert_eq!(
            s.points,
            vec![
                vec![0, 0, 0],
                vec![1, 1, 1],
                vec![2, 2, 2],
                vec![0, 1, 3],
                vec![1, 0, 4],
            ],
        );
    }

    #[test]
    fn step2_leaves_all_zero_axes_untouched() {
        // Y axis is entirely zero and must be preserved verbatim.
        let g = grid(2, vec![vec![0, 0], vec![6, 0], vec![9, 0]]);
        let s = compress_axes(&g);
        assert_eq!(s.points, vec![vec![0, 0], vec![2, 0], vec![3, 0]]);
    }

    #[test]
    fn both_passes_are_individually_idempotent() {
        for g in [scaled_shifted_3x3(), mixed_gcd_3d()] {
            let once = translate_to_origin(&g);
            let twice = translate_to_origin(&once);
            assert_eq!(once.points, twice.points);

            let once = compress_axes(&g);
            let twice = compress_axes(&once);
            assert_eq!(once.points, twice.points);
        }
    }

    /// The core chainability contract: every simplifier accepts any other
    /// simplifier's output (including its own) and every resulting chain is
    /// still a valid [`GridDefinition`].
    #[test]
    fn simplifier_outputs_can_feed_every_simplifier() {
        type Simplifier = fn(&GridDefinition) -> GridDefinition;
        const PASSES: &[Simplifier] = &[translate_to_origin, compress_axes];

        let fixtures = [scaled_shifted_3x3(), mixed_gcd_3d()];

        for g in &fixtures {
            for &f in PASSES {
                let after_one = f(g);
                after_one.validate().unwrap();

                for &next in PASSES {
                    let after_two = next(&after_one);
                    after_two.validate().unwrap();

                    for &last in PASSES {
                        let after_three = last(&after_two);
                        after_three.validate().unwrap();
                    }
                }
            }
        }
    }

    /// Equivalence contract: running the counting pipeline on any simplified
    /// variant of a grid must yield exactly the same pattern counts as the
    /// original. This covers every 1-, 2- and 3-step composition of the
    /// available simplifiers on several fixtures, so each link in any chain
    /// is checked to preserve the count.
    #[test]
    fn every_simplifier_chain_preserves_pattern_counts() {
        type Simplifier = fn(&GridDefinition) -> GridDefinition;
        const PASSES: &[Simplifier] = &[translate_to_origin, compress_axes];

        let fixtures = [scaled_shifted_3x3(), mixed_gcd_3d()];

        for g in &fixtures {
            let expected = counts_of(g);

            for &f in PASSES {
                let after_one = f(g);
                assert_eq!(counts_of(&after_one), expected);

                for &next in PASSES {
                    let after_two = next(&after_one);
                    assert_eq!(counts_of(&after_two), expected);

                    for &last in PASSES {
                        let after_three = last(&after_two);
                        assert_eq!(counts_of(&after_three), expected);
                    }
                }
            }
        }
    }

    #[test]
    fn canonicalize_is_idempotent() {
        for g in [scaled_shifted_3x3(), mixed_gcd_3d()] {
            let once = canonicalize(&g);
            let twice = canonicalize(&once);
            assert_eq!(once.points, twice.points);
            assert_eq!(once.dimensions, twice.dimensions);
        }
    }

    #[test]
    fn canonicalize_preserves_pattern_counts() {
        for g in [scaled_shifted_3x3(), mixed_gcd_3d()] {
            assert_eq!(counts_of(&canonicalize(&g)), counts_of(&g));
        }
    }

    #[test]
    fn empty_grid_is_handled_by_both_passes() {
        let g = grid(2, vec![]);
        let s1 = translate_to_origin(&g);
        let s2 = compress_axes(&g);
        assert!(s1.points.is_empty());
        assert!(s2.points.is_empty());
        assert_eq!(s1.dimensions, 2);
        assert_eq!(s2.dimensions, 2);
    }

    #[test]
    fn single_node_grids_collapse_to_canonical_form() {
        let g = grid(2, vec![vec![5, 7]]);
        assert_eq!(translate_to_origin(&g).points, vec![vec![0, 0]]);
        // A single non-zero node has axis GCDs equal to its coordinates, so
        // compression takes it to ±1 / 0 entries.
        assert_eq!(compress_axes(&g).points, vec![vec![1, 1]]);
    }
}
