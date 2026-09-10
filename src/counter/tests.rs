// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use super::*;
use crate::grid::{GridDefinition, build_grid_definition, compute_blocks};
use crate::visits::VisitFilters;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn inside_segment(origin: &[i32], target: &[i32], probe: &[i32]) -> bool {
    let delta: Vec<i128> = origin
        .iter()
        .zip(target)
        .map(|(&a, &b)| i128::from(b) - i128::from(a))
        .collect();
    let relative: Vec<i128> = origin
        .iter()
        .zip(probe)
        .map(|(&a, &b)| i128::from(b) - i128::from(a))
        .collect();
    let norm: i128 = delta.iter().map(|&d| d * d).sum();
    let projection: i128 = delta.iter().zip(&relative).map(|(&a, &b)| a * b).sum();
    projection > 0
        && projection < norm
        && delta
            .iter()
            .zip(relative)
            .all(|(&d, r)| d * projection == r * norm)
}

/// Enumerates ordered paths directly, without bitmasks, subset ranks, or DP.
fn enumerate(grid: &GridDefinition, max_length: usize, starts: u128) -> Vec<u128> {
    enumerate_with_filters(grid, max_length, starts, None)
}

fn enumerate_with_filters(
    grid: &GridDefinition,
    max_length: usize,
    starts: u128,
    allowed: Option<&[Vec<usize>]>,
) -> Vec<u128> {
    let n = grid.node_count();
    let mut blockers = vec![Vec::new(); n * n];
    for (a, origin) in grid.points.iter().enumerate() {
        for (b, target) in grid.points.iter().enumerate() {
            for (c, probe) in grid.points.iter().enumerate() {
                if inside_segment(origin, target, probe) {
                    blockers[a * n + b].push(c);
                }
            }
        }
    }
    let mut counts = vec![0; max_length + 1];
    counts[0] = 1;
    if max_length == 0 {
        return counts;
    }
    let mut visited = vec![false; n];
    for start in 0..n {
        if starts & (1 << start) != 0 && allowed.is_none_or(|visits| visits[0].contains(&start)) {
            visited[start] = true;
            enumerate_from(&blockers, &mut visited, start, 1, &mut counts, allowed);
            visited[start] = false;
        }
    }
    counts
}

fn enumerate_from(
    blockers: &[Vec<usize>],
    visited: &mut [bool],
    end: usize,
    length: usize,
    counts: &mut [u128],
    allowed: Option<&[Vec<usize>]>,
) {
    counts[length] += 1;
    if length + 1 == counts.len() {
        return;
    }
    let n = visited.len();
    for next in 0..n {
        if !visited[next]
            && allowed.is_none_or(|visits| visits[length].contains(&next))
            && blockers[end * n + next].iter().all(|&node| visited[node])
        {
            visited[next] = true;
            enumerate_from(blockers, visited, next, length + 1, counts, allowed);
            visited[next] = false;
        }
    }
}

fn collect<M: Mask>(
    scratch: &mut DpScratch,
    n: usize,
    blocks: &[M],
    max_length: usize,
    starts: M,
) -> Vec<u128> {
    let mut counts = Vec::new();
    count_patterns_seeded(scratch, n, blocks, max_length, starts, |event| {
        match event {
            DpEvent::LengthDone { length, count } => {
                assert_eq!(length, counts.len());
                counts.push(count);
            }
            DpEvent::Mask => {}
            DpEvent::Overflow => panic!("unexpected overflow"),
        }
        ControlFlow::Continue(())
    });
    counts
}

fn check_grid<M: Mask>(grid: &GridDefinition, max_length: usize) -> TestResult {
    let n = grid.node_count();
    let blocks = compute_blocks::<M>(grid);
    let mut scratch = DpScratch::allocate(n, &blocks, max_length)?;
    assert_eq!(
        collect(&mut scratch, n, &blocks, max_length, M::low_bits(n)),
        enumerate(grid, max_length, u128::MAX),
    );
    Ok(())
}

#[test]
fn geometry_enumeration_matches_all_mask_widths() -> TestResult {
    for (dims, free_points) in [
        (vec![3], 0),
        (vec![4], 2),
        (vec![2, 3], 0),
        (vec![3, 3], 0),
        (vec![2, 2, 2], 0),
        (vec![0], 5),
        (vec![0], 0),
    ] {
        let grid = build_grid_definition(&dims, free_points)?;
        let n = grid.node_count();
        check_grid::<u32>(&grid, n)?;
        check_grid::<u64>(&grid, n)?;
        check_grid::<u128>(&grid, n)?;
    }
    let grid = GridDefinition {
        dimensions: 3,
        points: vec![
            vec![-3, -3, -3],
            vec![0, 0, 0],
            vec![3, 3, 3],
            vec![1, 0, 2],
            vec![0, 2, 1],
        ],
        free_points: 2,
    };
    check_grid::<u32>(&grid, grid.node_count())?;
    check_grid::<u64>(&build_grid_definition(&[3, 11], 0)?, 3)?;
    check_grid::<u128>(&build_grid_definition(&[3, 22], 0)?, 3)
}

#[test]
fn every_requested_length_matches_geometry() -> TestResult {
    let grid = build_grid_definition(&[3, 3], 0)?;
    let expected = enumerate(&grid, 9, u128::MAX);
    let blocks = compute_blocks::<u32>(&grid);
    for max_length in 0..=9 {
        let mut scratch = DpScratch::allocate(9, &blocks, max_length)?;
        assert_eq!(
            collect(&mut scratch, 9, &blocks, max_length, u32::low_bits(9)),
            expected[..=max_length],
        );
    }
    Ok(())
}

#[test]
fn android_reference_counts() -> TestResult {
    let grid = build_grid_definition(&[3, 3], 0)?;
    let blocks = compute_blocks::<u32>(&grid);
    let mut scratch = DpScratch::allocate(9, &blocks, 9)?;
    assert_eq!(
        collect(&mut scratch, 9, &blocks, 9, u32::low_bits(9)),
        [1, 9, 56, 320, 1624, 7152, 26016, 72912, 140_704, 140_704],
    );
    Ok(())
}

#[test]
fn scratch_reuse_resets_seeds_and_layers() -> TestResult {
    let grid = build_grid_definition(&[3, 3], 0)?;
    let blocks = compute_blocks::<u32>(&grid);
    let unconstrained = vec![0; 81];
    let mut scratch = DpScratch::allocate_constrained(9, 9)?;
    for starts in [1, 1 << 4, 0, u32::low_bits(9), 0b10101] {
        let expected = enumerate(&grid, 9, u128::from(starts));
        assert_eq!(collect(&mut scratch, 9, &blocks, 9, starts), expected);
        let mut permutations = 1;
        let expected_free: Vec<_> = (0usize..=9)
            .map(|length| {
                if length == 1 {
                    permutations = u128::from(starts.count_ones());
                } else if length > 1 {
                    permutations *= (10 - length) as u128;
                }
                permutations
            })
            .collect();
        assert_eq!(
            collect(&mut scratch, 9, &unconstrained, 9, starts),
            expected_free,
        );
        assert_eq!(collect(&mut scratch, 9, &blocks, 9, starts), expected);
    }
    Ok(())
}

#[test]
fn widths_are_the_smallest_safe_factorial_widths() {
    let mut bound = 1u128;
    for p in 1..=35 {
        if p > 1 {
            bound *= (p - 1) as u128;
        }
        let width = [1, 2, 4, 8, 16]
            .into_iter()
            .find(|&width| width == 16 || bound < (1 << (width * 8)));
        assert_eq!(Some(cell_bytes(p)), width, "layer {p}");
    }
    assert_eq!(cell_bytes(127), 16);
}

fn check_cells<const BYTES: usize>(maximum: u128) {
    let mut bytes = vec![0; 3 * BYTES];
    add_cell::<BYTES>(&mut bytes, 1, maximum - 1);
    add_cell::<BYTES>(&mut bytes, 1, 1);
    assert_eq!(read_cell::<BYTES>(&bytes, 0), 0);
    assert_eq!(read_cell::<BYTES>(&bytes, 1), maximum);
    assert_eq!(read_cell::<BYTES>(&bytes, 2), 0);
    assert_eq!(cell_reader(BYTES)(&bytes, 1), maximum);
}

#[test]
fn all_cell_widths_preserve_values_and_neighbors() {
    check_cells::<1>(u128::from(u8::MAX));
    check_cells::<2>(u128::from(u16::MAX));
    check_cells::<4>(u128::from(u32::MAX));
    check_cells::<8>(u128::from(u64::MAX));
    check_cells::<16>(u128::MAX);
}

#[test]
fn dense_layers_cross_widths_and_both_parities() -> TestResult {
    let n = 15;
    let mut blocks = vec![0u32; n * n];
    // A diagonal blocker forces the dense path but cannot block a move to a
    // distinct unvisited node. Every ordering remains valid.
    blocks[0] = 1;
    let mut scratch = DpScratch::allocate(n, &blocks, n)?;
    let counts = collect(&mut scratch, n, &blocks, n, u32::low_bits(n));
    let mut permutations = 1;
    for (length, &count) in counts.iter().enumerate() {
        if length != 0 {
            permutations *= (n - length + 1) as u128;
        }
        assert_eq!(count, permutations);
    }
    Ok(())
}

#[test]
fn footprint_matches_reserved_storage_and_parity_peaks() -> TestResult {
    for n in 0..=16 {
        let mut previous = 0;
        for max_length in 0..=n + 1 {
            let scratch = DpScratch::allocate_constrained(n, max_length)?;
            let bytes = dp_table_bytes(n, max_length);
            assert_eq!(scratch.buf.len() as u64, bytes);
            assert_eq!(scratch.buf.capacity() as u64, bytes);
            assert!(bytes >= previous);
            previous = bytes;
        }
    }
    assert_eq!(dp_table_bytes(9, 0), 0);
    assert_eq!(dp_table_bytes(9, 1), 0);
    assert_eq!(dp_table_bytes(9, 2), 9);
    assert_eq!(dp_table_bytes(9, 3), 9 + 72);
    assert_eq!(dp_table_bytes(9, 7), 630 + 504);
    assert_eq!(dp_table_bytes(9, 8), 630 + 504);
    assert_eq!(dp_table_bytes(127, 127), u64::MAX);
    assert!(DpScratch::allocate_constrained(127, 127).is_err());
    for budget in [0, 8, 9, 80, 81, 1133, 1134, 4096] {
        let expected = (0..=9)
            .filter(|&length| dp_table_bytes(9, length) <= budget)
            .max()
            .unwrap_or(0);
        assert_eq!(effective_max_length(9, 9, budget), expected);
    }
    assert_eq!(effective_max_length(0, 0, 0), 0);
    Ok(())
}

fn event_record(event: &DpEvent) -> (u8, usize, u128) {
    match event {
        DpEvent::Mask => (0, 0, 0),
        DpEvent::LengthDone { length, count } => (1, *length, *count),
        DpEvent::Overflow => (2, 0, 0),
    }
}

#[test]
fn cancellation_emits_no_further_events_and_scratch_can_be_reused() -> TestResult {
    let grid = build_grid_definition(&[3], 0)?;
    let blocks = compute_blocks::<u32>(&grid);
    for matrix in [&blocks, &vec![0; 9]] {
        let mut scratch = DpScratch::allocate(3, matrix, 3)?;
        let mut expected = Vec::new();
        count_patterns_dp(&mut scratch, 3, matrix, 3, |event| {
            expected.push(event_record(&event));
            ControlFlow::Continue(())
        });
        for stop in 1..=expected.len() {
            let mut actual = Vec::new();
            count_patterns_dp(&mut scratch, 3, matrix, 3, |event| {
                actual.push(event_record(&event));
                if actual.len() == stop {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            });
            assert_eq!(actual, expected[..stop]);
        }
        let mut actual = Vec::new();
        count_patterns_dp(&mut scratch, 3, matrix, 3, |event| {
            actual.push(event_record(&event));
            ControlFlow::Continue(())
        });
        assert_eq!(actual, expected);
    }
    Ok(())
}

#[test]
fn unconstrained_overflow_stops_after_last_exact_length() -> TestResult {
    let n = 40;
    let blocks = vec![0u64; n * n];
    let mut scratch = DpScratch::allocate(n, &blocks, n)?;
    assert!(scratch.buf.is_empty());
    let mut actual = Vec::new();
    count_patterns_dp(&mut scratch, n, &blocks, n, |event| {
        actual.push(event_record(&event));
        ControlFlow::Continue(())
    });
    let mut expected = vec![(1, 0, 1)];
    let mut permutations = 1u128;
    for length in 1..=n {
        let Some(next) = permutations.checked_mul((n - length + 1) as u128) else {
            expected.push((2, 0, 0));
            break;
        };
        permutations = next;
        expected.push((1, length, permutations));
    }
    assert_eq!(actual, expected);
    assert_eq!(actual.last(), Some(&(2, 0, 0)));
    Ok(())
}

#[test]
fn layer_overflow_checks_sum_and_final_degree_product() {
    let blocks = [0u32; 9];
    for final_layer in [false, true] {
        let mut next = [0; 96];
        let layer = Layer {
            n: 3,
            p: 1,
            blocks: &blocks,
            allowed: u32::low_bits(3),
            current: &[],
            next: &mut next,
        };
        let mut on_event = |_| ControlFlow::Continue(());
        let result = if final_layer {
            layer.count::<0, _>(|_, _| u128::MAX, &mut on_event)
        } else {
            layer.count::<16, _>(|_, _| u128::MAX, &mut on_event)
        };
        assert!(matches!(result, Err(LayerStop::Overflow)));
    }
}

fn check_layer_ranks<const BYTES: usize>(unit: u128) {
    let n = 6;
    let blocks = vec![0u32; n * n];
    for p in [2, 3] {
        let masks: Vec<u32> = (0u32..1 << n)
            .filter(|mask| mask.count_ones() as usize == p)
            .collect();
        let next_masks: Vec<u32> = (0u32..1 << n)
            .filter(|mask| mask.count_ones() as usize == p + 1)
            .collect();
        let mut current = vec![0; masks.len() * p * BYTES];
        let mut expected = vec![0; next_masks.len() * (p + 1)];
        for (rank, &mask) in masks.iter().enumerate() {
            let endpoints: Vec<usize> = (0..n).filter(|&end| mask & (1 << end) != 0).collect();
            for (offset, &end) in endpoints.iter().enumerate() {
                let ways = unit * (end + 1) as u128;
                add_cell::<BYTES>(&mut current, rank * p + offset, ways);
                for next in (0..n).filter(|&next| mask & (1 << next) == 0) {
                    let next_mask = mask | (1 << next);
                    let next_rank = next_masks.iter().position(|&value| value == next_mask);
                    let Some(next_rank) = next_rank else {
                        panic!("extended subset missing");
                    };
                    let next_offset = (0..next)
                        .filter(|&node| next_mask & (1 << node) != 0)
                        .count();
                    expected[next_rank * (p + 1) + next_offset] += ways;
                }
            }
        }
        let mut next = vec![0; expected.len() * BYTES];
        let result = Layer {
            n,
            p,
            blocks: &blocks,
            allowed: u32::low_bits(n),
            current: &current,
            next: &mut next,
        }
        .count::<BYTES, _>(read_cell::<BYTES>, &mut |_| ControlFlow::Continue(()));
        assert!(matches!(result, Ok(count) if count == expected.iter().sum()));
        for (index, &value) in expected.iter().enumerate() {
            assert_eq!(read_cell::<BYTES>(&next, index), value);
        }
    }
}

#[test]
fn every_cell_width_uses_correct_destination_ranks() {
    check_layer_ranks::<1>(1);
    check_layer_ranks::<2>(1 << 8);
    check_layer_ranks::<4>(1 << 16);
    check_layer_ranks::<8>(1 << 32);
    check_layer_ranks::<16>(1 << 80);
}

fn collect_filtered<M: Mask>(
    scratch: &mut DpScratch,
    n: usize,
    blocks: &[M],
    max_length: usize,
    allowed: &[M],
) -> Vec<u128> {
    let mut counts = Vec::new();
    count_patterns_filtered(scratch, n, blocks, max_length, allowed, |event| {
        match event {
            DpEvent::LengthDone { length, count } => {
                assert_eq!(length, counts.len());
                counts.push(count);
            }
            DpEvent::Overflow => panic!("unexpected filtered overflow"),
            DpEvent::Mask => {}
        }
        ControlFlow::Continue(())
    });
    counts
}

fn check_filters<M: Mask>(grid: &GridDefinition, filters: &VisitFilters) -> TestResult {
    let n = grid.node_count();
    let blocks = compute_blocks::<M>(grid);
    let allowed = filters.masks::<M>();
    let expected = enumerate_with_filters(grid, filters.len(), u128::MAX, Some(filters.allowed()));
    for max_length in 0..=filters.len() {
        let mut scratch = DpScratch::allocate_filtered(n, &blocks, max_length, &allowed)?;
        assert_eq!(
            collect_filtered(&mut scratch, n, &blocks, max_length, &allowed),
            expected[..=max_length],
        );
    }
    Ok(())
}

#[test]
fn filtered_prefixes_and_groups_match_geometry_enumeration() -> TestResult {
    let grid = build_grid_definition(&[3, 3], 0)?;
    for nodes in [
        vec![],
        vec![vec![]],
        vec![vec![0], vec![1], vec![2, 3]],
        vec![vec![0], vec![2], vec![1]],
        vec![vec![0, 1, 2], vec![3, 4, 5], vec![6, 7, 8]],
        vec![vec![0, 1], vec![], vec![2, 3]],
        vec![vec![0], (0..9).collect(), (0..9).collect()],
        vec![(0..9).collect(); 9],
    ] {
        let filters = VisitFilters::new(9, nodes)?;
        check_filters::<u32>(&grid, &filters)?;
        check_filters::<u64>(&grid, &filters)?;
        check_filters::<u128>(&grid, &filters)?;
    }
    let grid = build_grid_definition(&[4], 2)?;
    check_filters::<u32>(
        &grid,
        &VisitFilters::new(6, vec![vec![4, 5], vec![0, 3], vec![1, 2], vec![4, 5]])?,
    )
}

#[test]
fn free_grids_honor_future_filters_and_high_mask_bits() -> TestResult {
    let grid = build_grid_definition(&[0], 31)?;
    check_filters::<u32>(
        &grid,
        &VisitFilters::new(31, vec![vec![0, 15, 30], vec![1, 16, 29]])?,
    )?;
    let grid = build_grid_definition(&[0], 63)?;
    check_filters::<u64>(
        &grid,
        &VisitFilters::new(63, vec![vec![0, 31, 62], vec![1, 32, 61]])?,
    )?;
    let grid = build_grid_definition(&[0], 127)?;
    check_filters::<u128>(
        &grid,
        &VisitFilters::new(127, vec![vec![0, 63, 126], vec![1, 64, 125]])?,
    )
}

#[test]
fn filtered_allocation_distinguishes_zero_seeds_and_restricted_future() -> TestResult {
    let n = 9;
    let free = vec![0u32; n * n];
    let full = u32::low_bits(n);
    let mut allowed = vec![full; n];
    allowed[0] = 1;
    let mut scratch = DpScratch::allocate_filtered(n, &free, n, &allowed)?;
    assert!(scratch.buf.is_empty());
    let seeded = collect_filtered(&mut scratch, n, &free, n, &allowed);
    assert_eq!(seeded[1], 1);
    assert_eq!(seeded[n], 40320);

    allowed[n - 1] = 1 << (n - 1);
    let mut scratch = DpScratch::allocate_filtered(n, &free, n, &allowed)?;
    assert_eq!(scratch.buf.len() as u64, dp_table_bytes(n, n));
    let restricted = collect_filtered(&mut scratch, n, &free, n, &allowed);
    assert_eq!(restricted[..n], seeded[..n]);
    assert_eq!(restricted[n], 5040);

    allowed[0] = 0;
    let mut scratch = DpScratch::allocate_filtered(n, &free, n, &allowed)?;
    assert!(scratch.buf.is_empty());
    assert_eq!(
        collect_filtered(&mut scratch, n, &free, n, &allowed),
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    Ok(())
}

#[test]
fn filtered_scratch_reuse_resets_changed_visit_sets() -> TestResult {
    let grid = build_grid_definition(&[3, 3], 0)?;
    let blocks = compute_blocks::<u32>(&grid);
    let mut scratch = DpScratch::allocate_constrained(9, 3)?;
    for visits in [
        vec![vec![0], vec![1], vec![2, 3]],
        vec![vec![0], vec![2], vec![1]],
        vec![vec![], vec![1], vec![2]],
        vec![(0..9).collect(); 3],
        vec![vec![8], vec![7], vec![6, 5]],
    ] {
        let filters = VisitFilters::new(9, visits)?;
        assert_eq!(
            collect_filtered(&mut scratch, 9, &blocks, 3, &filters.masks::<u32>()),
            enumerate_with_filters(&grid, 3, u128::MAX, Some(filters.allowed())),
        );
    }
    Ok(())
}

#[test]
fn filtered_cancellation_stops_at_every_event() -> TestResult {
    let grid = build_grid_definition(&[3], 0)?;
    let blocks = compute_blocks::<u32>(&grid);
    let allowed = [1, 0b110, 0b101];
    let mut scratch = DpScratch::allocate_filtered(3, &blocks, 3, &allowed)?;
    let mut expected = Vec::new();
    count_patterns_filtered(&mut scratch, 3, &blocks, 3, &allowed, |event| {
        expected.push(event_record(&event));
        ControlFlow::Continue(())
    });
    for stop in 1..=expected.len() {
        let mut actual = Vec::new();
        count_patterns_filtered(&mut scratch, 3, &blocks, 3, &allowed, |event| {
            actual.push(event_record(&event));
            if actual.len() == stop {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        assert_eq!(actual, expected[..stop]);
    }
    Ok(())
}

#[test]
fn future_filters_preserve_width_transitions_and_exact_prefix_counts() -> TestResult {
    let n = 15;
    let blocks = vec![0u32; n * n];
    let mut allowed = vec![u32::low_bits(n); n];
    allowed[n - 1] = 1 | (1 << 4) | (1 << 8);
    let mut scratch = DpScratch::allocate_filtered(n, &blocks, n, &allowed)?;
    let counts = collect_filtered(&mut scratch, n, &blocks, n, &allowed);
    let mut permutations = 1;
    for (length, &count) in counts.iter().enumerate() {
        if length != 0 {
            permutations *= (n - length + 1) as u128;
        }
        let expected = if length == n {
            permutations / 5
        } else {
            permutations
        };
        assert_eq!(count, expected);
    }
    Ok(())
}

#[test]
fn empty_first_visit_needs_no_tables_even_for_the_largest_grid() -> TestResult {
    let n = 127;
    let mut blocks = vec![0u128; n * n];
    blocks[0] = 1;
    let mut allowed = vec![u128::low_bits(n); n];
    allowed[0] = 0;
    let mut scratch = DpScratch::allocate_filtered(n, &blocks, n, &allowed)?;
    assert!(scratch.buf.is_empty());
    let counts = collect_filtered(&mut scratch, n, &blocks, n, &allowed);
    assert_eq!(counts.len(), n + 1);
    assert_eq!(counts[0], 1);
    assert!(counts[1..].iter().all(|&count| count == 0));
    Ok(())
}
