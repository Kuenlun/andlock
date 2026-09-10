// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use std::ops::ControlFlow;

use andlock::counter::DpEvent;
use andlock::grid::{GridDefinition, build_grid_definition, compute_blocks};
use andlock::mask::Mask;
use andlock::search::{count_patterns_bounded, count_plan};

fn visit(n: usize, blocks: &[u128], seen: u128, last: usize, counts: &mut [u128]) {
    let length = seen.count_ones() as usize;
    counts[length] += 1;
    if length + 1 == counts.len() {
        return;
    }
    for next in 0..n {
        if seen & (1 << next) == 0 && blocks[last * n + next] & !seen == 0 {
            visit(n, blocks, seen | (1 << next), next, counts);
        }
    }
}

fn geometric_counts(grid: &GridDefinition, limit: usize) -> Vec<u128> {
    let n = grid.node_count();
    let mut required = vec![0u128; n * n];
    // Independent segment test, without the production blocking-matrix builder.
    for (a, p) in grid.points.iter().enumerate() {
        for (b, q) in grid.points.iter().enumerate() {
            if a == b {
                continue;
            }
            let axis = (0..grid.dimensions).find(|&i| p[i] != q[i]);
            let Some(axis) = axis else { continue };
            for (c, r) in grid.points.iter().enumerate() {
                if c == a || c == b {
                    continue;
                }
                let inside =
                    (0..grid.dimensions).all(|i| r[i] >= p[i].min(q[i]) && r[i] <= p[i].max(q[i]));
                let collinear = (0..grid.dimensions).all(|i| {
                    i128::from(r[i] - p[i]) * i128::from(q[axis] - p[axis])
                        == i128::from(r[axis] - p[axis]) * i128::from(q[i] - p[i])
                });
                if inside && collinear {
                    required[a * n + b] |= 1 << c;
                }
            }
        }
    }
    let mut counts = vec![0; limit + 1];
    counts[0] = 1;
    if limit > 0 {
        for start in 0..n {
            visit(n, &required, 1 << start, start, &mut counts);
        }
    }
    counts
}

fn check<M: Mask>(
    grid: &GridDefinition,
    limit: usize,
    budget: u64,
    expected: &[u128],
) -> Result<(), Box<dyn std::error::Error>> {
    let blocks = compute_blocks::<M>(grid);
    let mut actual = vec![None; limit + 1];
    count_patterns_bounded(grid, &blocks, limit, budget, |event| {
        if let DpEvent::LengthDone { length, count } = event {
            assert!(actual[length].replace(count).is_none());
        }
        assert!(!matches!(event, DpEvent::Overflow));
        ControlFlow::Continue(())
    })?;
    assert_eq!(
        actual,
        expected.iter().copied().map(Some).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn every_budget_matches_independent_geometry() -> Result<(), Box<dyn std::error::Error>> {
    let mut grids = vec![
        build_grid_definition(&[0], 0)?,
        build_grid_definition(&[3], 0)?,
        build_grid_definition(&[6], 0)?,
        build_grid_definition(&[2, 3], 0)?,
        build_grid_definition(&[3, 3], 0)?,
        build_grid_definition(&[2, 2, 2], 0)?,
        build_grid_definition(&[3], 3)?,
        build_grid_definition(&[0], 7)?,
    ];
    for sample in 0..12 {
        let mut points: Vec<Vec<i32>> = (0..9)
            .filter(|&i| (i * 7 + sample * 3) % 11 < 8)
            .map(|i| vec![i % 3, i / 3])
            .collect();
        if sample % 2 == 0 {
            points.reverse();
        }
        grids.push(GridDefinition {
            dimensions: 2,
            points,
            free_points: 1,
        });
    }
    for grid in grids {
        let limit = grid.node_count().min(6);
        let expected = geometric_counts(&grid, limit);
        for budget in [0, 1, 32, 256, 1024, 4096, u64::MAX] {
            check::<u32>(&grid, limit, budget, &expected)?;
            check::<u64>(&grid, limit, budget, &expected)?;
            check::<u128>(&grid, limit, budget, &expected)?;
        }
    }
    Ok(())
}

#[test]
fn planner_never_exceeds_budget() {
    for n in [0usize, 1, 2, 7, 15, 31, 32, 60, 63, 64, 90, 126, 127] {
        for length in [0, n / 2, n.saturating_sub(1), n] {
            for budget in [0, 32, 1024, 1 << 20, 1 << 30] {
                let plan = count_plan(n, length, budget);
                assert!(plan.table_bytes <= budget);
                assert!(plan.prefix_length <= length);
            }
        }
    }
    let plan = count_plan(60, 8, 512 << 20);
    assert_eq!(plan.prefix_length, 1);
}

#[test]
fn proposed_symmetries_must_preserve_supplied_movement_rules()
-> Result<(), Box<dyn std::error::Error>> {
    let grid = build_grid_definition(&[0], 3)?;
    let mut blocks = [0u128; 9];
    blocks[1] = 1 << 2;
    let mut counts = Vec::new();
    count_patterns_bounded(&grid, &blocks, 3, 0, |event| {
        if let DpEvent::LengthDone { count, .. } = event {
            counts.push(count);
        }
        ControlFlow::Continue(())
    })?;
    assert_eq!(counts, [1, 3, 5, 5]);
    Ok(())
}

#[test]
fn cancellation_discards_unfinished_partition_totals() -> Result<(), Box<dyn std::error::Error>> {
    let grid = build_grid_definition(&[3, 4], 0)?;
    let blocks = compute_blocks::<u32>(&grid);
    let mut ticks = 0;
    let mut lengths = Vec::new();
    count_patterns_bounded(&grid, &blocks, 8, 0, |event| {
        match event {
            DpEvent::Mask => ticks += 1,
            DpEvent::LengthDone { length, count } => lengths.push((length, count)),
            DpEvent::Overflow => panic!("unexpected overflow"),
        }
        if ticks == 40 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    assert_eq!(ticks, 40);
    let expected = geometric_counts(&grid, 4);
    assert!(lengths.len() >= 3 && lengths.len() < expected.len());
    assert!(
        lengths
            .iter()
            .enumerate()
            .all(|(i, &(length, count))| length == i && count == expected[i])
    );
    Ok(())
}
