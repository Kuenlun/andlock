// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Baseline benchmarks for [`count_patterns_dp`] across representative grids.

// Cargo shares package dependencies across its library, binary and test targets.
use {
    anyhow as _, clap as _, clap_cargo as _, clap_complete as _, console as _, ctrlc as _,
    indicatif as _, num_bigint as _, parse_size as _, portable_pty as _, serde as _,
    serde_json as _, sysinfo as _, vt100 as _,
};

use std::hint::black_box;
use std::ops::ControlFlow;
use std::time::Duration;

use criterion::{Criterion, SamplingMode, Throughput, criterion_group, criterion_main};

use andlock::counter::{DpScratch, count_patterns_dp, dp_mask_ticks};
use andlock::grid::{build_grid_definition, compute_blocks};
use andlock::mask::Mask;

#[expect(
    clippy::expect_used,
    reason = "Fixed benchmark grids and their scratch allocations must be valid before timing."
)]
fn bench_case<M: Mask>(
    c: &mut Criterion,
    label: &str,
    dims: &[i32],
    free_points: usize,
    max_length: usize,
) {
    let grid = build_grid_definition(dims, free_points).expect("bench grid within MAX_POINTS");
    let n = grid.node_count();
    let blocks: Vec<M> = compute_blocks::<M>(&grid);

    let mut group = c.benchmark_group("dp");
    let _group = group
        .sample_size(20)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(4))
        .sampling_mode(SamplingMode::Flat)
        .throughput(Throughput::Elements(dp_mask_ticks(n, max_length)));
    let mut scratch = DpScratch::allocate::<M>(n, &blocks, max_length).expect("scratch allocation");
    let _benchmark = group.bench_function(label, |b| {
        b.iter(|| {
            count_patterns_dp(&mut scratch, n, &blocks, max_length, |event| {
                let _event = black_box(event);
                ControlFlow::Continue(())
            });
        });
    });
    group.finish();
}

fn run(c: &mut Criterion) {
    bench_case::<u32>(c, "android_3x3_l9", &[3_i32, 3_i32], 0, 9);
    bench_case::<u32>(c, "rectangle_3x4_l8", &[3_i32, 4_i32], 0, 8);
    bench_case::<u32>(c, "rectangle_3x4_l11", &[3_i32, 4_i32], 0, 11);
    bench_case::<u32>(c, "rectangle_3x5_l15", &[3_i32, 5_i32], 0, 15);
    bench_case::<u32>(c, "rectangle_3x6_l6", &[3_i32, 6_i32], 0, 6);
    bench_case::<u32>(c, "rectangle_3x7_l11", &[3_i32, 7_i32], 0, 11);
    bench_case::<u32>(c, "rectangle_3x8_l7", &[3_i32, 8_i32], 0, 7);
    bench_case::<u32>(c, "square_3x3_f1_l10", &[3_i32, 3_i32], 1, 10);
    bench_case::<u64>(c, "rectangle_3x11_l3", &[3_i32, 11_i32], 0, 3);
    bench_case::<u128>(c, "rectangle_3x22_l3", &[3_i32, 22_i32], 0, 3);
}

criterion_group!(benches, run);
criterion_main!(benches);
