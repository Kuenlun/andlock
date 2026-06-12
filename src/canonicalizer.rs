// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Canonical-form normalisation for [`GridDefinition`] grids.
//!
//! Two grids are equivalent when one can be reached from the other by
//! integer translation and/or per-axis integer scaling. Both moves preserve
//! every collinearity, direction, and skip-over relationship the counter
//! reads. [`canonicalize`] picks a unique representative: divide each axis by
//! the GCD of its pairwise coordinate differences, then translate the node
//! closest to the centroid into the origin. The GCDs are computed on
//! differences, which makes them translation-invariant, and the anchor is
//! chosen in the fully scaled metric — so the output is a fixed point and
//! canonicalising twice yields the same grid.

use crate::grid::GridDefinition;

#[must_use]
pub fn canonicalize(grid: &GridDefinition) -> GridDefinition {
    if grid.points.is_empty() {
        return grid.clone();
    }
    // Scale differences to points[0]: they are exact in i64 and divisible by
    // the axis GCDs. Which member is subtracted does not matter — the GCD of
    // member differences equals the GCD of all pairwise differences, and the
    // recentre below erases the interim translation.
    let divisors: Vec<i64> = (0..grid.dimensions)
        .map(|axis| axis_divisor(&grid.points, axis))
        .collect();
    let scaled: Vec<Vec<i64>> = grid
        .points
        .iter()
        .map(|p| {
            p.iter()
                .zip(&grid.points[0])
                .zip(&divisors)
                .map(|((&c, &o), &g)| (i64::from(c) - i64::from(o)) / g)
                .collect()
        })
        .collect();

    let anchor = scaled[centroid_anchor_index(&scaled)].clone();
    let points = scaled
        .iter()
        .map(|p| {
            p.iter()
                .zip(&anchor)
                .map(|(&c, &o)| to_coord(c - o))
                .collect()
        })
        .collect();

    GridDefinition {
        dimensions: grid.dimensions,
        points,
        free_points: grid.free_points,
    }
}

/// GCD of the pairwise coordinate differences along `axis` (computed against
/// `points[0]`, which yields the same value), or `1` when the axis is
/// constant so that division is a no-op.
fn axis_divisor(points: &[Vec<i32>], axis: usize) -> i64 {
    let origin = i64::from(points[0][axis]);
    points
        .iter()
        .map(|p| (i64::from(p[axis]) - origin).abs())
        .fold(0, gcd)
        .max(1)
}

/// GCD over non-negative inputs; `gcd(0, x) = x` absorbs zero differences.
const fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Index of the node minimising squared distance to the centroid, with ties
/// broken in favour of the lowest index.
///
/// Comparing `(n * p[axis] - sum[axis])^2` instead of
/// `(p[axis] - centroid[axis])^2` keeps everything in integers without
/// changing the ordering. The metric is translation-invariant, which is what
/// makes the recentring step idempotent.
fn centroid_anchor_index(points: &[Vec<i64>]) -> usize {
    let n = points.len() as i128;
    let dim = points.first().map_or(0, Vec::len);
    let sums: Vec<i128> = (0..dim)
        .map(|axis| points.iter().map(|p| i128::from(p[axis])).sum())
        .collect();

    points
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
        .map_or(0, |(idx, _)| idx)
}

/// Converts a canonical coordinate back to `i32`. Differences of coordinates
/// within [`crate::grid::MAX_COORD`] always fit.
///
/// # Panics
/// Panics on unvalidated grids whose coordinate differences leave `i32` —
/// loudly, rather than silently wrapping into a non-equivalent grid.
fn to_coord(value: i64) -> i32 {
    i32::try_from(value).unwrap_or_else(|_| {
        panic!("canonical coordinate {value} does not fit i32; validate the grid first")
    })
}
