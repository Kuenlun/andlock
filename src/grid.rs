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

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Hard upper bound on the number of nodes the program accepts.
///
/// The DP algorithm represents the visited set as a `u32` bitmask, so
/// `1u32 << n` must not overflow — limiting `n` to at most 31.
pub const MAX_POINTS: usize = 31;

/// Finite set of integer-coordinate nodes in `dimensions`-dimensional space.
#[derive(Clone, Serialize, Deserialize)]
pub struct GridDefinition {
    pub dimensions: usize,
    pub points: Vec<Vec<i32>>,
}

impl GridDefinition {
    /// # Errors
    /// Returns an error if:
    /// - the point count exceeds [`MAX_POINTS`],
    /// - any point does not have exactly [`GridDefinition::dimensions`] coordinates, or
    /// - two points share the same coordinates (duplicates are not allowed).
    pub fn validate(&self) -> Result<(), String> {
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
        let mut seen: HashMap<&Vec<i32>, usize> = HashMap::new();
        for (idx, point) in self.points.iter().enumerate() {
            if let Some(&first) = seen.get(point) {
                return Err(format!(
                    "points {first} and {idx} have the same coordinates {point:?}"
                ));
            }
            seen.insert(point, idx);
        }
        Ok(())
    }
}

/// Returns a flat `n × n` row-major matrix where `blocks[a * n + b]` is the
/// bitmask of nodes lying strictly on the open segment `(a, b)`.
///
/// The matrix is symmetric: `blocks[a * n + b] == blocks[b * n + a]`.
#[must_use]
pub fn compute_blocks(grid: &GridDefinition) -> Vec<u32> {
    let n = grid.points.len();
    let dim = grid.dimensions;
    let mut blocks = vec![0u32; n * n];

    for a in 0..n {
        let origin = &grid.points[a];
        for b in (a + 1)..n {
            let target = &grid.points[b];

            for (c, probe) in grid.points.iter().enumerate() {
                if c == a || c == b {
                    continue;
                }

                let in_box = (0..dim).all(|i| {
                    let lo = origin[i].min(target[i]);
                    let hi = origin[i].max(target[i]);
                    lo <= probe[i] && probe[i] <= hi
                });
                if !in_box {
                    continue;
                }

                // Collinearity: every pairwise 2-D cross product must vanish.
                // Promotion to i64 avoids overflow on the product of two i32 deltas.
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

/// Parses a dimension spec like `"3x3"`, `"10"`, `"0x0x1"`, or `"2x3x2"` into axis sizes.
///
/// A component of `0` means that axis has no points; any spec containing `0`
/// produces an empty grid (the only valid pattern is the empty one).
///
/// # Errors
/// Returns an error if the spec is empty, contains a non-integer component,
/// or contains a negative component.
pub fn parse_dims(spec: &str) -> Result<Vec<i32>, String> {
    if spec.is_empty() {
        return Err("dimensions string must not be empty".into());
    }
    let normalized = spec.to_ascii_lowercase();
    normalized
        .split('x')
        .map(|part| {
            let value: i32 = part.parse().map_err(|_| {
                format!("invalid dimension component '{part}': expected a non-negative integer")
            })?;
            if value < 0 {
                return Err(format!(
                    "invalid dimension component '{part}': must be >= 0"
                ));
            }
            Ok(value)
        })
        .collect()
}

/// Enumerates every lattice point of the rectangular grid described by `dims`
/// in row-major (last axis fastest) order.
fn generate_grid_points(dims: &[i32]) -> Vec<Vec<i32>> {
    fn recurse(dims: &[i32], current: &mut Vec<i32>, out: &mut Vec<Vec<i32>>) {
        match dims.split_first() {
            Some((&head, tail)) => {
                for i in 0..head {
                    current.push(i);
                    recurse(tail, current, out);
                    current.pop();
                }
            }
            None => out.push(current.clone()),
        }
    }
    let mut out = Vec::new();
    let mut current = Vec::with_capacity(dims.len());
    recurse(dims, &mut current, &mut out);
    out
}

/// Assembles a [`GridDefinition`] from a rectangular base grid and a number of
/// "free points".
///
/// Each free point gets its own orthogonal axis (zero-padded on all base axes),
/// guaranteeing zero collinearity without any numeric tolerance.
///
/// The result is routed through [`crate::canonicalizer::canonicalize`] so that
/// generated grids are always in the crate's canonical form — any two calls
/// with the same arguments are byte-for-byte identical, and extending the
/// canonical pipeline automatically updates what this function emits.
#[must_use]
pub fn build_grid_definition(dims: &[i32], free_points: usize) -> GridDefinition {
    let base_dim = dims.len();
    let total_dim = base_dim + free_points;

    let mut points: Vec<Vec<i32>> = generate_grid_points(dims)
        .into_iter()
        .map(|mut p| {
            p.resize(total_dim, 0);
            p
        })
        .collect();

    for i in 0..free_points {
        let mut fp = vec![0i32; total_dim];
        fp[base_dim + i] = 1;
        points.push(fp);
    }

    crate::canonicalizer::canonicalize(&GridDefinition {
        dimensions: total_dim,
        points,
    })
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn grid(dimensions: usize, points: Vec<Vec<i32>>) -> GridDefinition {
        GridDefinition { dimensions, points }
    }

    #[test]
    fn validate_rejects_more_than_max_points() {
        let points = vec![vec![0i32, 0]; MAX_POINTS + 1];
        let err = grid(2, points).validate().unwrap_err();
        assert!(err.contains("exceeds"), "unexpected error: {err}");
    }

    #[test]
    fn validate_accepts_exactly_max_points() {
        let points: Vec<Vec<i32>> = (0i32..).take(MAX_POINTS).map(|i| vec![i, 0]).collect();
        assert!(grid(2, points).validate().is_ok());
    }

    #[test]
    fn validate_rejects_duplicate_points() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![0, 0]]);
        let err = g.validate().unwrap_err();
        assert!(err.contains("points 0 and 2"), "unexpected error: {err}");
        assert!(err.contains("[0, 0]"), "unexpected error: {err}");
    }

    #[test]
    fn validate_rejects_adjacent_duplicate_points() {
        let g = grid(2, vec![vec![1, 2], vec![1, 2]]);
        let err = g.validate().unwrap_err();
        assert!(err.contains("points 0 and 1"), "unexpected error: {err}");
    }

    #[test]
    fn validate_rejects_dimension_mismatch() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0, 2], vec![2, 0]]);
        let err = g.validate().unwrap_err();
        assert!(err.contains("point 1"), "unexpected error: {err}");
        assert!(err.contains('3'), "unexpected error: {err}");
    }

    #[test]
    fn linear_triplet_records_midpoint_as_blocker() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![2, 0]]);
        let blocks = compute_blocks(&g);
        let b_bit = 1u32 << 1;
        assert_eq!(blocks[2], b_bit); // A → C
        assert_eq!(blocks[6], b_bit); // C → A
        assert_eq!(blocks[1], 0); // A → B (adjacent, no blocker)
        assert_eq!(blocks[5], 0); // B → C (adjacent, no blocker)
    }

    #[test]
    fn bounding_box_alone_does_not_imply_collinearity() {
        // A=(0,0), B=(2,2), probe=(1,0): in the box, not on the diagonal.
        let g = grid(2, vec![vec![0, 0], vec![2, 2], vec![1, 0]]);
        let blocks = compute_blocks(&g);
        assert_eq!(blocks[1], 0);
        assert_eq!(blocks[3], 0);
    }

    #[test]
    fn collinearity_detected_in_three_dimensions() {
        let g = grid(3, vec![vec![0, 0, 0], vec![1, 1, 1], vec![2, 2, 2]]);
        let blocks = compute_blocks(&g);
        let b_bit = 1u32 << 1;
        assert_eq!(blocks[2], b_bit);
        assert_eq!(blocks[6], b_bit);
    }

    #[test]
    fn diverging_third_axis_is_not_collinear() {
        // A=(0,0,0), B=(2,2,2), probe=(1,1,0): xy agrees, z does not.
        let g = grid(3, vec![vec![0, 0, 0], vec![2, 2, 2], vec![1, 1, 0]]);
        let blocks = compute_blocks(&g);
        assert_eq!(blocks[1], 0);
    }

    #[test]
    fn block_matrix_is_symmetric() {
        #[rustfmt::skip]
        let g = grid(
            2,
            vec![
                vec![0, 0], vec![1, 0], vec![2, 0],
                vec![0, 1], vec![1, 1], vec![2, 1],
                vec![0, 2], vec![1, 2], vec![2, 2],
            ],
        );
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        for a in 0..n {
            for b in 0..n {
                assert_eq!(blocks[a * n + b], blocks[b * n + a]);
            }
        }
    }

    #[test]
    fn parse_dims_single_axis() {
        assert_eq!(parse_dims("10").unwrap(), vec![10]);
    }

    #[test]
    fn parse_dims_two_axes() {
        assert_eq!(parse_dims("3x3").unwrap(), vec![3, 3]);
    }

    #[test]
    fn parse_dims_three_axes_preserves_order() {
        assert_eq!(parse_dims("2x3x2").unwrap(), vec![2, 3, 2]);
    }

    #[test]
    fn parse_dims_rejects_empty_string() {
        let err = parse_dims("").unwrap_err();
        assert!(err.contains("empty"), "unexpected error: {err}");
    }

    #[test]
    fn parse_dims_accepts_uppercase_separator() {
        assert_eq!(parse_dims("3X3").unwrap(), vec![3, 3]);
        assert_eq!(parse_dims("2X3X2").unwrap(), vec![2, 3, 2]);
    }

    #[test]
    fn parse_dims_rejects_non_integer_component() {
        assert!(parse_dims("3xabc").is_err());
    }

    #[test]
    fn parse_dims_accepts_zero_component() {
        assert_eq!(parse_dims("0").unwrap(), vec![0]);
        assert_eq!(parse_dims("3x0x2").unwrap(), vec![3, 0, 2]);
    }

    #[test]
    fn parse_dims_rejects_negative_component() {
        assert!(parse_dims("3x-1").is_err());
    }

    #[test]
    fn parse_dims_rejects_empty_component() {
        assert!(parse_dims("3x").is_err());
        assert!(parse_dims("x3").is_err());
    }

    #[test]
    fn build_grid_with_zero_dimension_produces_empty_grid() {
        let g = build_grid_definition(&[0, 0, 1], 0);
        assert_eq!(g.dimensions, 3);
        assert!(g.points.is_empty());
        g.validate().unwrap();
    }

    #[test]
    fn build_grid_without_free_points_is_centroid_anchored() {
        // 3x3 grid: raw points are (0..3, 0..3); the centroid (1, 1) is itself a
        // node, so canonicalisation translates it to the origin and axis GCDs
        // remain 1.
        let g = build_grid_definition(&[3, 3], 0);
        assert_eq!(g.dimensions, 2);
        assert_eq!(g.points.len(), 9);
        assert_eq!(g.points[0], vec![-1, -1]);
        assert_eq!(g.points[4], vec![0, 0]);
        assert_eq!(g.points[8], vec![1, 1]);
        g.validate().unwrap();
    }

    #[test]
    fn build_grid_with_free_points_promotes_to_orthogonal_axes() {
        // The central base node (1, 1, 0, 0) is nearest the (9/11, 9/11, 1/11,
        // 1/11) centroid so canonicalisation anchors there; free-point axes
        // retain their unit spacing.
        let g = build_grid_definition(&[3, 3], 2);
        assert_eq!(g.dimensions, 4);
        assert_eq!(g.points.len(), 11);
        assert_eq!(g.points[0], vec![-1, -1, 0, 0]);
        assert_eq!(g.points[4], vec![0, 0, 0, 0]);
        assert_eq!(g.points[8], vec![1, 1, 0, 0]);
        assert_eq!(g.points[9], vec![-1, -1, 1, 0]);
        assert_eq!(g.points[10], vec![-1, -1, 0, 1]);
        g.validate().unwrap();
    }

    #[test]
    fn build_grid_is_deterministic_for_repeated_calls() {
        let a = build_grid_definition(&[4, 2, 3], 1);
        let b = build_grid_definition(&[4, 2, 3], 1);
        assert_eq!(a.dimensions, b.dimensions);
        assert_eq!(a.points, b.points);
    }

    #[test]
    fn build_grid_always_emits_canonical_form() {
        // Across a representative sweep of shapes, the generator must be a
        // fixed point of the canonical pipeline — calling `canonicalize` on
        // the result must not change it.
        let shapes: &[(&[i32], usize)] = &[
            (&[1], 0),
            (&[2], 0),
            (&[3, 3], 0),
            (&[2, 2], 0),
            (&[4, 3], 0),
            (&[2, 3, 2], 0),
            (&[3, 3], 1),
            (&[3, 3], 2),
            (&[2, 2, 2], 2),
        ];
        for &(dims, free) in shapes {
            let g = build_grid_definition(dims, free);
            let again = crate::canonicalizer::canonicalize(&g);
            assert_eq!(
                g.points, again.points,
                "generator output was not canonical for dims={dims:?} free={free}"
            );
            assert_eq!(g.dimensions, again.dimensions);
        }
    }
}
