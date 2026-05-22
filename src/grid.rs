// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use std::collections::HashMap;

use serde::Deserialize;

use crate::mask::Mask;

/// Hard upper bound on the number of nodes the program accepts.
///
/// Re-exported from [`crate::mask::MAX_POINTS`] so callers can name a
/// single ceiling without having to know which mask width the dispatcher
/// will pick at runtime. Equal to `<u128 as Mask>::MAX_POINTS = 127`.
pub use crate::mask::MAX_POINTS;

/// Finite set of integer-coordinate nodes in `dimensions`-dimensional space.
#[derive(Clone, Deserialize)]
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
/// bitmask of nodes lying strictly on the open segment `(a, b)`, encoded
/// in the [`Mask`] type the caller selects.
///
/// The matrix is symmetric: `blocks[a * n + b] == blocks[b * n + a]`.
///
/// # Panics
/// Panics if `grid.points.len() > M::MAX_POINTS` — callers are expected
/// to pick `M` via [`crate::mask::smallest_for`] (or the equivalent
/// ladder) so the chosen width can hold the grid; this assertion is a
/// safety net rather than a documented user-facing error.
#[must_use]
pub fn compute_blocks<M: Mask>(grid: &GridDefinition) -> Vec<M> {
    let n = grid.points.len();
    assert!(
        n <= M::MAX_POINTS,
        "compute_blocks called with n={n} > Mask::MAX_POINTS={}",
        M::MAX_POINTS,
    );
    let dim = grid.dimensions;
    let mut blocks: Vec<M> = vec![M::ZERO; n * n];

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
                    let c_bit = M::bit(c);
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
