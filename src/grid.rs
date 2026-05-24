// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use std::collections::HashMap;

use serde::Deserialize;

use crate::mask::Mask;

/// Maximum supported point count (`= 127`), re-exported from [`crate::mask`].
pub use crate::mask::MAX_POINTS;

/// Finite set of integer-coordinate base nodes in `dimensions`-dimensional
/// space, optionally accompanied by `free_points` isolated nodes that sit on
/// no line and never block any move.
#[derive(Clone, Deserialize)]
pub struct GridDefinition {
    pub dimensions: usize,
    pub points: Vec<Vec<i32>>,
    #[serde(default)]
    pub free_points: usize,
}

impl GridDefinition {
    /// Total node count fed to the DP: base points plus free points.
    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.points.len() + self.free_points
    }

    /// # Errors
    /// Returns an error when the total node count exceeds [`MAX_POINTS`], a
    /// base point has the wrong arity, or two base points share coordinates.
    pub fn validate(&self) -> Result<(), String> {
        let n = self.node_count();
        if n > MAX_POINTS {
            return Err(format!(
                "{n} nodes ({} base + {} free) exceeds the supported maximum of {MAX_POINTS}",
                self.points.len(),
                self.free_points,
            ));
        }
        let mut seen: HashMap<&Vec<i32>, usize> = HashMap::with_capacity(self.points.len());
        for (idx, point) in self.points.iter().enumerate() {
            if point.len() != self.dimensions {
                return Err(format!(
                    "point {idx} has {} coordinate(s); expected {}",
                    point.len(),
                    self.dimensions,
                ));
            }
            if let Some(first) = seen.insert(point, idx) {
                return Err(format!(
                    "points {first} and {idx} have the same coordinates {point:?}"
                ));
            }
        }
        Ok(())
    }
}

/// Symmetric `n x n` row-major matrix where `blocks[a * n + b]` is the
/// bitmask of nodes lying strictly on the open segment `(a, b)`.
///
/// `n = grid.node_count()`; the trailing `grid.free_points` indices have all
/// zero rows and columns because free points never lie on any base segment.
///
/// # Panics
/// Panics if `grid.node_count() > M::MAX_POINTS`; pick `M` via
/// [`crate::mask::smallest_for`].
#[must_use]
pub fn compute_blocks<M: Mask>(grid: &GridDefinition) -> Vec<M> {
    let n_base = grid.points.len();
    let n = grid.node_count();
    assert!(
        n <= M::MAX_POINTS,
        "compute_blocks called with n={n} > Mask::MAX_POINTS={}",
        M::MAX_POINTS,
    );
    let dim = grid.dimensions;
    let mut blocks: Vec<M> = vec![M::ZERO; n * n];
    let mut delta: Vec<i64> = Vec::with_capacity(dim);
    let mut probe_rel: Vec<i64> = Vec::with_capacity(dim);

    for a in 0..n_base {
        let origin = &grid.points[a];
        for b in (a + 1)..n_base {
            let target = &grid.points[b];

            delta.clear();
            delta.extend((0..dim).map(|i| i64::from(target[i] - origin[i])));

            for (c, probe) in grid.points.iter().enumerate() {
                if c == a || c == b {
                    continue;
                }
                if !in_bounding_box(origin, target, probe) {
                    continue;
                }

                probe_rel.clear();
                probe_rel.extend((0..dim).map(|i| i64::from(probe[i] - origin[i])));

                // Collinearity: every pairwise 2-D cross product vanishes.
                let collinear = (0..dim).all(|i| {
                    ((i + 1)..dim).all(|j| probe_rel[i] * delta[j] == probe_rel[j] * delta[i])
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

fn in_bounding_box(origin: &[i32], target: &[i32], probe: &[i32]) -> bool {
    origin.iter().zip(target).zip(probe).all(|((&o, &t), &p)| {
        let (lo, hi) = if o <= t { (o, t) } else { (t, o) };
        lo <= p && p <= hi
    })
}

/// Parses dimension specs like `"3x3"`, `"10"`, `"0x1"` into axis sizes.
/// A `0` component yields an empty grid.
///
/// # Errors
/// Empty spec, non-integer component, or negative component.
pub fn parse_dims(spec: &str) -> Result<Vec<i32>, String> {
    if spec.is_empty() {
        return Err("dimensions string must not be empty".into());
    }
    spec.split(['x', 'X'])
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

/// Every lattice point of `dims` in row-major (last axis fastest) order.
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

/// Rectangular base grid with `free_points` isolated extra nodes, base
/// coordinates canonicalised.
#[must_use]
pub fn build_grid_definition(dims: &[i32], free_points: usize) -> GridDefinition {
    crate::canonicalizer::canonicalize(&GridDefinition {
        dimensions: dims.len(),
        points: generate_grid_points(dims),
        free_points,
    })
}
