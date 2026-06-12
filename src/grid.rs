// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Grid model: the JSON-loadable definition, dimension-spec parsing, lattice
//! generation, and the blocking matrix the DP consumes.

use std::collections::HashMap;

use serde::Deserialize;

use crate::mask::Mask;

/// Maximum supported point count (`= 127`), re-exported from [`crate::mask`].
pub use crate::mask::MAX_POINTS;

/// Largest accepted coordinate magnitude (`2^30 - 1`).
///
/// With every coordinate in `[-MAX_COORD, MAX_COORD]`, coordinate differences
/// fit `i32`, so the canonical form of a valid grid is itself a valid grid
/// and every intermediate the crate computes stays exact.
pub const MAX_COORD: i32 = (1 << 30) - 1;

/// Finite set of integer-coordinate base nodes in `dimensions`-dimensional
/// space, optionally accompanied by `free_points` isolated nodes that sit on
/// no line and never block any move.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridDefinition {
    pub dimensions: usize,
    pub points: Vec<Vec<i32>>,
    #[serde(default)]
    pub free_points: usize,
}

impl GridDefinition {
    /// Total node count fed to the DP: base points plus free points.
    /// Saturates instead of wrapping on absurd `free_points` values so that
    /// [`validate`](Self::validate) rejects them rather than mis-sizing a run.
    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.points.len().saturating_add(self.free_points)
    }

    /// # Errors
    /// Returns an error when the total node count exceeds [`MAX_POINTS`], a
    /// base point has the wrong arity, a coordinate falls outside
    /// `[-MAX_COORD, MAX_COORD]`, or two base points share coordinates.
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
            if let Some(&coord) = point
                .iter()
                .find(|c| c.unsigned_abs() > MAX_COORD.unsigned_abs())
            {
                return Err(format!(
                    "point {idx} coordinate {coord} is outside the supported range \
                     [-{MAX_COORD}, {MAX_COORD}]"
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
/// `n = grid.node_count()`. The trailing `grid.free_points` indices have all
/// zero rows and columns because free points never lie on any base segment.
/// Exact over the full `i32` coordinate range: deltas are widened to `i64`
/// and the proportionality products to `i128`.
///
/// # Panics
/// Panics if `grid.node_count() > M::MAX_POINTS`. Pick `M` via
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
            delta.extend((0..dim).map(|i| i64::from(target[i]) - i64::from(origin[i])));
            // Distinct points differ on some axis; that axis anchors the
            // proportionality test below. Guard anyway so an unvalidated
            // duplicate degenerates to "no interior" instead of misfiring.
            let Some(j0) = delta.iter().position(|&d| d != 0) else {
                continue;
            };

            for (c, probe) in grid.points.iter().enumerate() {
                if c == a || c == b {
                    continue;
                }
                if !in_bounding_box(origin, target, probe) {
                    continue;
                }

                probe_rel.clear();
                probe_rel.extend((0..dim).map(|i| i64::from(probe[i]) - i64::from(origin[i])));

                // Collinearity: probe_rel must be proportional to delta. With
                // delta[j0] != 0, the 2-D cross products against axis j0 alone
                // are sufficient — O(dim) instead of all-pairs O(dim^2).
                let collinear = (0..dim).all(|i| {
                    i128::from(probe_rel[i]) * i128::from(delta[j0])
                        == i128::from(probe_rel[j0]) * i128::from(delta[i])
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
            let value: i32 = part.parse().map_err(|e: std::num::ParseIntError| {
                if *e.kind() == std::num::IntErrorKind::PosOverflow {
                    format!(
                        "invalid dimension component '{part}': any grid this size would \
                         exceed the {MAX_POINTS}-node maximum"
                    )
                } else {
                    format!("invalid dimension component '{part}': expected a non-negative integer")
                }
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
///
/// # Errors
/// Returns an error when an axis size is negative or the lattice plus free
/// points would exceed [`MAX_POINTS`] nodes. The count is checked before any
/// point is materialised, so absurd axis sizes fail fast instead of
/// exhausting memory.
pub fn build_grid_definition(dims: &[i32], free_points: usize) -> Result<GridDefinition, String> {
    let base = dims
        .iter()
        .try_fold(1u128, |acc, &d| {
            u128::try_from(d).ok().map(|d| acc.saturating_mul(d))
        })
        .ok_or("axis sizes must be non-negative")?;
    let total = base.saturating_add(free_points as u128);
    if total > MAX_POINTS as u128 {
        return Err(format!(
            "{total} nodes ({base} base + {free_points} free) exceeds the supported \
             maximum of {MAX_POINTS}"
        ));
    }
    Ok(crate::canonicalizer::canonicalize(&GridDefinition {
        dimensions: dims.len(),
        points: generate_grid_points(dims),
        free_points,
    }))
}
