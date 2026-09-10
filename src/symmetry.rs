// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Verified node permutations used to partition starting points into orbits.

use std::collections::{HashMap, HashSet};

use crate::grid::GridDefinition;
use crate::mask::Mask;

pub fn starting_orbits<M: Mask>(grid: &GridDefinition, blocks: &[M]) -> Vec<(usize, u128)> {
    starting_orbits_with(grid, blocks, None)
}

pub fn starting_orbits_filtered<M: Mask>(
    grid: &GridDefinition,
    blocks: &[M],
    allowed: &[M],
) -> Vec<(usize, u128)> {
    starting_orbits_with(grid, blocks, Some(allowed))
}

fn starting_orbits_with<M: Mask>(
    grid: &GridDefinition,
    blocks: &[M],
    allowed: Option<&[M]>,
) -> Vec<(usize, u128)> {
    let n = grid.node_count();
    let base = grid.points.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let index: HashMap<&[i32], usize> = grid
        .points
        .iter()
        .enumerate()
        .map(|(i, point)| (point.as_slice(), i))
        .collect();

    let bounds: Vec<(i32, i32)> = (0..grid.dimensions)
        .map(|axis| {
            grid.points
                .iter()
                .fold((i32::MAX, i32::MIN), |(lo, hi), p| {
                    (lo.min(p[axis]), hi.max(p[axis]))
                })
        })
        .collect();

    let active = distinct_active_axes(grid, &bounds);
    for &axis in &active {
        let (lo, hi) = bounds[axis];
        let permutation = map_points(grid, &index, |point| {
            point[axis] = lo + (hi - point[axis]);
        });
        if let Some(permutation) = permutation {
            join_verified(&mut parent, blocks, &permutation, allowed);
        }
    }
    if base > 0 {
        let permutation = map_points(grid, &index, |point| {
            for (value, &(lo, hi)) in point.iter_mut().zip(&bounds) {
                *value = lo + (hi - *value);
            }
        });
        if let Some(permutation) = permutation {
            join_verified(&mut parent, blocks, &permutation, allowed);
        }
    }
    // Swapping equal-sized axes also captures rotations of square and cubic grids.
    for (position, &a) in active.iter().enumerate() {
        for &b in &active[position + 1..] {
            let ((lo_a, hi_a), (lo_b, hi_b)) = (bounds[a], bounds[b]);
            if lo_a >= hi_a || hi_a - lo_a != hi_b - lo_b {
                continue;
            }
            let permutation = map_points(grid, &index, |point| {
                let x = point[a] - lo_a;
                point[a] = lo_a + (point[b] - lo_b);
                point[b] = lo_b + x;
            });
            if let Some(permutation) = permutation {
                join_verified(&mut parent, blocks, &permutation, allowed);
            }
        }
    }
    for node in base + 1..n {
        let mut permutation: Vec<usize> = (0..n).collect();
        permutation.swap(base, node);
        join_verified(&mut parent, blocks, &permutation, allowed);
    }
    let mut weights = vec![0u128; n];
    for node in 0..n {
        if allowed.is_some_and(|allowed| {
            allowed
                .first()
                .is_none_or(|mask| *mask & M::bit(node) == M::ZERO)
        }) {
            continue;
        }
        let root = root(&parent, node);
        weights[root] += 1;
    }
    weights
        .into_iter()
        .enumerate()
        .filter(|&(_, weight)| weight != 0)
        .collect()
}

fn distinct_active_axes(grid: &GridDefinition, bounds: &[(i32, i32)]) -> Vec<usize> {
    let mut profiles = HashSet::new();
    bounds
        .iter()
        .enumerate()
        .filter_map(|(axis, &(lo, hi))| {
            if lo >= hi {
                return None;
            }
            let profile: Vec<i32> = grid.points.iter().map(|point| point[axis] - lo).collect();
            profiles.insert(profile).then_some(axis)
        })
        .collect()
}

fn map_points(
    grid: &GridDefinition,
    index: &HashMap<&[i32], usize>,
    transform: impl Fn(&mut [i32]),
) -> Option<Vec<usize>> {
    let mut permutation = Vec::with_capacity(grid.node_count());
    for point in &grid.points {
        let mut mapped = point.clone();
        transform(&mut mapped);
        permutation.push(*index.get(mapped.as_slice())?);
    }
    permutation.extend(grid.points.len()..grid.node_count());
    Some(permutation)
}

fn root(parent: &[usize], mut node: usize) -> usize {
    while parent[node] != node {
        node = parent[node];
    }
    node
}

fn join_verified<M: Mask>(
    parent: &mut [usize],
    blocks: &[M],
    permutation: &[usize],
    allowed: Option<&[M]>,
) {
    let n = parent.len();
    if allowed.is_some_and(|allowed| {
        allowed.iter().any(|&mask| {
            permutation.iter().enumerate().any(|(source, &target)| {
                (mask & M::bit(source) != M::ZERO) != (mask & M::bit(target) != M::ZERO)
            })
        })
    }) {
        return;
    }
    for a in 0..n {
        for b in 0..n {
            let mut source = blocks[a * n + b];
            let mut mapped = M::ZERO;
            while source != M::ZERO {
                let bit = source & source.wrapping_neg();
                source ^= bit;
                mapped |= M::bit(permutation[bit.trailing_zeros() as usize]);
            }
            if mapped != blocks[permutation[a] * n + permutation[b]] {
                return;
            }
        }
    }
    for (a, &b) in permutation.iter().enumerate() {
        let left = root(parent, a);
        let right = root(parent, b);
        parent[left.max(right)] = left.min(right);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::compute_blocks;

    #[test]
    fn repeated_coordinate_profiles_propose_only_one_active_axis() {
        let grid = GridDefinition {
            dimensions: 4096,
            points: vec![vec![0; 4096], vec![1; 4096], vec![2; 4096]],
            free_points: 0,
        };
        let bounds = vec![(0, 2); 4096];
        assert_eq!(distinct_active_axes(&grid, &bounds), [0]);
        let blocks = compute_blocks::<u32>(&grid);
        assert_eq!(starting_orbits(&grid, &blocks), [(0, 2), (1, 1)]);
        assert_eq!(
            starting_orbits_filtered(&grid, &blocks, &[0b111, 0b001]),
            [(0, 1), (1, 1), (2, 1)]
        );
    }
}
