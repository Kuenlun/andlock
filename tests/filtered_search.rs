// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use std::ops::ControlFlow;

use andlock::grid::{GridDefinition, build_grid_definition, compute_blocks};
use andlock::mask::Mask;
use andlock::numeric::{CountEvent, GlobalCount};
use andlock::search::{count_patterns_bounded_filtered, filtered_table_bytes};
use andlock::visits::VisitFilters;
use num_bigint::BigUint;

fn legal(grid: &GridDefinition, seen: &[bool], origin: usize, target: usize) -> bool {
    if origin >= grid.points.len() || target >= grid.points.len() {
        return true;
    }
    let a = &grid.points[origin];
    let b = &grid.points[target];
    let delta: Vec<i128> = a
        .iter()
        .zip(b)
        .map(|(&a, &b)| i128::from(b) - i128::from(a))
        .collect();
    let Some(axis) = delta.iter().position(|&d| d != 0) else {
        return false;
    };
    grid.points.iter().enumerate().all(|(index, point)| {
        if seen[index] || index == target {
            return true;
        }
        let offset: Vec<i128> = a
            .iter()
            .zip(point)
            .map(|(&a, &p)| i128::from(p) - i128::from(a))
            .collect();
        let along = offset[axis];
        let step = delta[axis];
        let between = if step > 0 {
            0 < along && along < step
        } else {
            step < along && along < 0
        };
        !between
            || offset
                .iter()
                .zip(&delta)
                .any(|(&x, &d)| x * step != along * d)
    })
}

fn brute(grid: &GridDefinition, filters: &VisitFilters) -> Vec<u128> {
    fn visit(
        grid: &GridDefinition,
        filters: &[Vec<usize>],
        seen: &mut [bool],
        last: Option<usize>,
        length: usize,
        counts: &mut [u128],
    ) {
        counts[length] += 1;
        if length == filters.len() {
            return;
        }
        for &next in &filters[length] {
            if !seen[next] && last.is_none_or(|last| legal(grid, seen, last, next)) {
                seen[next] = true;
                visit(grid, filters, seen, Some(next), length + 1, counts);
                seen[next] = false;
            }
        }
    }
    let mut counts = vec![0; filters.len() + 1];
    visit(
        grid,
        filters.allowed(),
        &mut vec![false; grid.node_count()],
        None,
        0,
        &mut counts,
    );
    counts
}

fn collect<M: Mask, C: GlobalCount>(
    grid: &GridDefinition,
    filters: &VisitFilters,
    maximum: usize,
    budget: u64,
) -> Result<Vec<C>, Box<dyn std::error::Error>> {
    let blocks = compute_blocks::<M>(grid);
    let allowed = filters.masks::<M>();
    let mut counts = Vec::new();
    count_patterns_bounded_filtered(grid, &blocks, maximum, budget, &allowed, |event| {
        match event {
            CountEvent::LengthDone { length, count } => {
                assert_eq!(length, counts.len());
                counts.push(count);
            }
            CountEvent::Overflow => panic!("unexpected overflow"),
            CountEvent::Mask => {}
        }
        ControlFlow::Continue(())
    })?;
    Ok(counts)
}

fn check<M: Mask>(
    grid: &GridDefinition,
    allowed: Vec<Vec<usize>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let filters = VisitFilters::new(grid.node_count(), allowed)?;
    let expected = brute(grid, &filters);
    for maximum in 0..=filters.len() {
        for budget in [0, 1, 9, 64, 257, 1024, 1 << 20] {
            assert_eq!(
                collect::<M, u128>(grid, &filters, maximum, budget)?,
                expected[..=maximum],
                "maximum={maximum}, budget={budget}"
            );
            assert_eq!(
                collect::<M, BigUint>(grid, &filters, maximum, budget)?,
                expected[..=maximum]
                    .iter()
                    .copied()
                    .map(BigUint::from)
                    .collect::<Vec<_>>()
            );
        }
    }
    Ok(())
}

#[test]
fn every_budget_matches_independent_geometry_with_restricted_orbits()
-> Result<(), Box<dyn std::error::Error>> {
    let grid = build_grid_definition(&[3, 3], 0).unwrap();
    check::<u32>(
        &grid,
        vec![
            (0..9).collect(),
            vec![0, 1],
            vec![2, 4, 7],
            vec![3, 5, 8],
            vec![6, 7],
        ],
    )?;
    check::<u32>(
        &grid,
        vec![vec![0], vec![1], vec![2, 3, 4], vec![5, 6], vec![7, 8]],
    )?;
    check::<u32>(
        &grid,
        vec![
            vec![0, 2, 6, 8],
            vec![1, 3, 5, 7],
            vec![4],
            vec![0, 2, 6, 8],
        ],
    )?;
    check::<u32>(&grid, vec![vec![0, 1], vec![2, 3], vec![], vec![5, 6]])?;
    check::<u32>(&grid, vec![vec![0], vec![2], vec![3, 4]])?;
    Ok(())
}

#[test]
fn free_nodes_and_all_mask_widths_match_brute() -> Result<(), Box<dyn std::error::Error>> {
    check::<u32>(
        &build_grid_definition(&[2, 3], 2).unwrap(),
        vec![vec![0, 6, 7], vec![1, 4, 7], vec![2, 5, 6], vec![3, 4]],
    )?;
    for n in [31usize, 32, 63, 64, 127] {
        let grid = build_grid_definition(&[i32::try_from(n).unwrap()], 0).unwrap();
        let allowed = vec![vec![0, n - 1], vec![1, n - 2], vec![2, n - 3]];
        if n <= 31 {
            check::<u32>(&grid, allowed.clone())?;
        }
        if n <= 63 {
            check::<u64>(&grid, allowed.clone())?;
        }
        check::<u128>(&grid, allowed)?;
    }
    Ok(())
}

#[test]
fn deterministic_and_impossible_large_filters_need_no_combinatorial_work()
-> Result<(), Box<dyn std::error::Error>> {
    let grid = build_grid_definition(&[3, 20], 0).unwrap();
    let filters = VisitFilters::new(60, (0..8).map(|node| vec![node]).collect()).unwrap();
    assert_eq!(
        filtered_table_bytes::<u64, u128>(
            60,
            &compute_blocks::<u64>(&grid),
            8,
            u64::MAX,
            &filters.masks::<u64>()
        ),
        0
    );
    assert_eq!(
        collect::<u64, u128>(&grid, &filters, 8, u64::MAX)?,
        vec![1; 9]
    );
    let grid = build_grid_definition(&[127], 0).unwrap();
    for first in [vec![], vec![0]] {
        let mut allowed = vec![(0..127).collect::<Vec<_>>(); 127];
        allowed[0] = first.clone();
        if !first.is_empty() {
            allowed[1] = vec![2];
        }
        let filters = VisitFilters::new(127, allowed).unwrap();
        assert_eq!(
            filtered_table_bytes::<u128, u128>(
                127,
                &compute_blocks::<u128>(&grid),
                127,
                u64::MAX,
                &filters.masks::<u128>()
            ),
            0
        );
        let counts = collect::<u128, u128>(&grid, &filters, 127, u64::MAX)?;
        assert_eq!(counts[0], 1);
        assert_eq!(counts[1], u128::from(!first.is_empty()));
        assert!(counts[2..].iter().all(|&count| count == 0));
    }
    Ok(())
}

#[test]
fn forced_free_prefix_uses_arbitrary_precision_for_its_unrestricted_tail()
-> Result<(), Box<dyn std::error::Error>> {
    let grid = GridDefinition {
        dimensions: 0,
        points: Vec::new(),
        free_points: 40,
    };
    let mut allowed = vec![(2..40).collect::<Vec<_>>(); 40];
    allowed[0] = vec![0];
    allowed[1] = vec![1];
    let filters = VisitFilters::new(40, allowed).unwrap();
    let factorial = (1u128..=38).fold(BigUint::from(1u128), |value, factor| value * factor);
    for budget in [0, 1024, 1 << 20, u64::MAX] {
        assert_eq!(
            filtered_table_bytes::<u64, BigUint>(
                40,
                &compute_blocks::<u64>(&grid),
                40,
                budget,
                &filters.masks::<u64>()
            ),
            0
        );
        let counts = collect::<u64, BigUint>(&grid, &filters, 40, budget)?;
        assert_eq!(counts[0..=2], vec![BigUint::from(1u128); 3]);
        assert_eq!(counts[40], factorial);
    }
    Ok(())
}

#[test]
fn cancellation_stops_every_filtered_event_without_finalizing_later_lengths() {
    let grid = build_grid_definition(&[2, 2], 0).unwrap();
    let blocks = compute_blocks::<u32>(&grid);
    for sets in [
        vec![vec![0, 1], vec![2, 3], vec![0, 1]],
        vec![vec![0], vec![1], vec![2]],
        vec![vec![0, 1], vec![], vec![2]],
        vec![vec![0], vec![0, 1, 2, 3], vec![0, 1, 2, 3]],
    ] {
        let filters = VisitFilters::new(4, sets).unwrap();
        let allowed = filters.masks::<u32>();
        for budget in [0, 1024] {
            let mut complete: Vec<CountEvent<u128>> = Vec::new();
            count_patterns_bounded_filtered(&grid, &blocks, 3, budget, &allowed, |event| {
                complete.push(event);
                ControlFlow::Continue(())
            })
            .unwrap();
            for stop in 1..=complete.len() {
                let mut partial = Vec::new();
                count_patterns_bounded_filtered(&grid, &blocks, 3, budget, &allowed, |event| {
                    partial.push(event);
                    if partial.len() == stop {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                })
                .unwrap();
                assert_eq!(partial, complete[..stop]);
            }
        }
    }
}

#[test]
fn filtered_fixed_width_overflow_preserves_the_exact_prefix() {
    let grid = GridDefinition {
        dimensions: 0,
        points: Vec::new(),
        free_points: 40,
    };
    let blocks = compute_blocks::<u64>(&grid);
    let mut allowed = vec![u64::low_bits(40); 40];
    allowed[0] = 1;
    let mut events: Vec<CountEvent<u128>> = Vec::new();
    count_patterns_bounded_filtered(&grid, &blocks, 40, 0, &allowed, |event| {
        events.push(event);
        ControlFlow::Continue(())
    })
    .unwrap();
    let mut expected = vec![
        CountEvent::LengthDone {
            length: 0,
            count: 1,
        },
        CountEvent::LengthDone {
            length: 1,
            count: 1,
        },
    ];
    let mut count = 1u128;
    for length in 2..=40 {
        let Some(next) = count.checked_mul((41 - length) as u128) else {
            break;
        };
        count = next;
        expected.push(CountEvent::LengthDone { length, count });
    }
    expected.push(CountEvent::Overflow);
    assert_eq!(events, expected);
}
