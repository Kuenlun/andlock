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

use serde::{Deserialize, Serialize};

/// Upper bound on the number of nodes the program accepts.
///
/// [`crate::dp::count_patterns_dp`] represents the visited set with a `u32` bitmask and
/// allocates a `2ⁿ × n` table of `u64` counts. At `n = 25` the table already
/// reaches ~6.7 GiB, which we treat as the ceiling of what is realistic to
/// run on a workstation.
pub const MAX_POINTS: usize = 25;

/// Finite set of integer-coordinate nodes in `dimensions`-dimensional space.
#[derive(Serialize, Deserialize)]
pub struct GridDefinition {
    pub dimensions: usize,
    pub points: Vec<Vec<i32>>,
}

impl GridDefinition {
    /// # Errors
    /// Returns an error if the point count exceeds [`MAX_POINTS`] or any point
    /// does not have exactly [`GridDefinition::dimensions`] coordinates.
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
        Ok(())
    }
}

/// Returns a flat `n × n` row-major matrix where `blocks[a * n + b]` is the
/// bitmask of nodes lying strictly on the open segment `(a, b)`.
///
/// The matrix is symmetric: `blocks[a * n + b] == blocks[b * n + a]`.
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

/// Parses a dimension spec like `"3x3"`, `"10"`, or `"2x3x2"` into axis sizes.
///
/// # Errors
/// Returns an error if the spec is empty, contains a non-integer component,
/// or contains a component less than `1`.
pub fn parse_dims(spec: &str) -> Result<Vec<i32>, String> {
    if spec.is_empty() {
        return Err("dimensions string must not be empty".into());
    }
    let normalized = spec.to_ascii_lowercase();
    normalized
        .split('x')
        .map(|part| {
            let value: i32 = part.parse().map_err(|_| {
                format!("invalid dimension component '{part}': expected a positive integer")
            })?;
            if value < 1 {
                return Err(format!(
                    "invalid dimension component '{part}': must be >= 1"
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

    GridDefinition {
        dimensions: total_dim,
        points,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
        let points = vec![vec![0i32, 0]; MAX_POINTS];
        assert!(grid(2, points).validate().is_ok());
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
    fn parse_dims_rejects_zero_component() {
        assert!(parse_dims("3x0x2").is_err());
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
    fn build_grid_without_free_points_matches_base() {
        let g = build_grid_definition(&[3, 3], 0);
        assert_eq!(g.dimensions, 2);
        assert_eq!(g.points.len(), 9);
        assert_eq!(g.points[0], vec![0, 0]);
        assert_eq!(g.points[8], vec![2, 2]);
        g.validate().unwrap();
    }

    #[test]
    fn build_grid_with_free_points_promotes_to_orthogonal_axes() {
        let g = build_grid_definition(&[3, 3], 2);
        assert_eq!(g.dimensions, 4);
        assert_eq!(g.points.len(), 11);
        assert_eq!(g.points[0], vec![0, 0, 0, 0]);
        assert_eq!(g.points[8], vec![2, 2, 0, 0]);
        assert_eq!(g.points[9], vec![0, 0, 1, 0]);
        assert_eq!(g.points[10], vec![0, 0, 0, 1]);
        g.validate().unwrap();
    }
}
