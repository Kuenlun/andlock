// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Canonical-form normalisation for [`GridDefinition`] grids.
//!
//! Two grids are equivalent when one can be reached from the other by
//! integer translation and/or per-axis integer scaling; this preserves every
//! collinearity, direction, and skip-over relationship the counter reads.
//! [`canonicalize`] picks the unique smallest representative: anchor the
//! node closest to the centroid at the origin, then divide each axis by the
//! GCD of its coordinate magnitudes. Both passes are idempotent.

use crate::grid::GridDefinition;

#[must_use]
pub fn canonicalize(grid: &GridDefinition) -> GridDefinition {
    let Some(anchor_idx) = pick_centroid_anchor(grid) else {
        return grid.clone();
    };
    let offset = grid.points[anchor_idx].clone();
    let gcds: Vec<u32> = (0..grid.dimensions)
        .map(|axis| axis_gcd(grid, axis, offset[axis]))
        .collect();

    let points = grid
        .points
        .iter()
        .map(|p| {
            p.iter()
                .zip(&offset)
                .zip(&gcds)
                .map(|((&c, &o), &g)| divide_exact(c - o, g))
                .collect()
        })
        .collect();

    GridDefinition {
        dimensions: grid.dimensions,
        points,
    }
}

/// Index of the node minimising squared distance to the centroid; ties break
/// to the lowest index. `None` only on an empty grid.
///
/// Comparing `(n * p[j] - sum[j])^2` instead of `(p[j] - centroid[j])^2`
/// keeps everything in integers without changing the ordering.
fn pick_centroid_anchor(grid: &GridDefinition) -> Option<usize> {
    let n = i128::try_from(grid.points.len()).unwrap_or(0);
    let sums: Vec<i128> = (0..grid.dimensions)
        .map(|axis| grid.points.iter().map(|p| i128::from(p[axis])).sum())
        .collect();

    grid.points
        .iter()
        .enumerate()
        .min_by_key(|(_, p)| {
            p.iter()
                .zip(&sums)
                .map(|(&coord, &sum)| {
                    let diff = i128::from(coord) * n - sum;
                    diff * diff
                })
                .sum::<i128>()
        })
        .map(|(idx, _)| idx)
}

/// GCD of `|p[axis] - offset|` across every non-zero translated coordinate.
/// Returns `0` when the axis is entirely zero after translation; that case
/// turns [`divide_exact`] into a no-op.
fn axis_gcd(grid: &GridDefinition, axis: usize, offset: i32) -> u32 {
    grid.points
        .iter()
        .filter_map(|p| {
            let v = (p[axis] - offset).unsigned_abs();
            (v != 0).then_some(v)
        })
        .reduce(gcd)
        .unwrap_or(0)
}

const fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Exact division; `divisor <= 1` is a no-op so the per-axis GCD pass can
/// pass `0` for zero-only axes. The quotient always fits back in `i32`.
fn divide_exact(coord: i32, divisor: u32) -> i32 {
    if divisor <= 1 {
        return coord;
    }
    i32::try_from(i64::from(coord) / i64::from(divisor)).unwrap_or(coord)
}
