// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Canonical-form normalization for [`GridDefinition`] grids.
//!
//! Two grids are *geometrically equivalent* when they share the same
//! collinearity, direction, and visibility (skip-over) relationships — i.e.
//! when one can be obtained from the other by integer translation and/or
//! per-axis integer scaling.  The **canonical form** is the unique smallest
//! representative of each equivalence class.
//!
//! # Canonicalization pipeline
//!
//! [`canonicalize`] applies two passes in sequence.
//!
//! ## Pass 1 — centroid-anchor translation ([`translate_to_origin`])
//!
//! The node closest to the centroid of the grid (the *centroid anchor*) is
//! translated to the origin.  Translation is an isometry, so all geometric
//! relationships are preserved verbatim.  Once the anchor sits at
//! `(0, …, 0)`, the GCD calculation in pass 2 sees its coordinates as zeros
//! and ignores them, which is correct: the GCD should be determined by the
//! *relative* positions of the remaining nodes, not an arbitrary global
//! offset.
//!
//! ## Pass 2 — per-axis GCD compression ([`compress_axes`])
//!
//! Each axis is compressed independently: the coordinates of all nodes along
//! that axis are divided by their greatest common divisor.  This brings every
//! axis to the coarsest integer lattice that still expresses the same relative
//! spacings.
//!
//! Compressing axes independently is more aggressive than applying a single
//! global GCD, because a global factor can only cancel what is common to
//! *all* axes simultaneously, while per-axis factors cancel each dimension's
//! slack separately.  The trade-off is that per-axis scaling does not preserve
//! absolute angles: two segments at 45° may no longer be so after one axis is
//! compressed more than the other.  This is intentional — the canonicalizer
//! preserves *topological* equivalence (collinearity, direction, visibility),
//! not metric equivalence.  A use case that requires angle preservation would
//! need a single global GCD instead.
//!
//! # Properties
//!
//! | Property | Guarantee |
//! |---|---|
//! | Correctness | Pattern counts are identical before and after canonicalization. |
//! | Idempotency | `canonicalize(canonicalize(g))` equals `canonicalize(g)`. |
//! | Composability | Every pass has the shape `&GridDefinition → GridDefinition`, so passes can be freely chained and each pass's output is a valid input for any other. |

use crate::grid::GridDefinition;

/// Returns the canonical form of `grid` by running all normalization passes
/// in order.
///
/// This is the single source of truth for what *canonical form* means in
/// this crate.  Grid constructors and the CLI both funnel through here so
/// that adding a new pass automatically propagates everywhere a canonical
/// grid is expected.
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
