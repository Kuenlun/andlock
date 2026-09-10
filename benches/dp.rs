// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Baseline benchmarks for [`count_patterns_dp`] across representative grids.

use std::hint::black_box;
use std::ops::ControlFlow;
use std::time::Duration;

use criterion::{Criterion, SamplingMode, Throughput, criterion_group, criterion_main};

use andlock::counter::{DpScratch, count_patterns_dp, dp_mask_ticks};
use andlock::grid::{build_grid_definition, compute_blocks};
use andlock::mask::Mask;

#[allow(clippy::expect_used)]
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
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(4));
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(dp_mask_ticks(n, max_length)));
    let mut scratch = DpScratch::allocate::<M>(n, &blocks, max_length).expect("scratch allocation");
    group.bench_function(label, |b| {
        b.iter(|| {
            count_patterns_dp(&mut scratch, n, &blocks, max_length, |event| {
                black_box(event);
                ControlFlow::Continue(())
            });
        });
    });
    group.finish();
}

fn run(c: &mut Criterion) {
    bench_case::<u32>(c, "android_3x3_l9", &[3, 3], 0, 9);
    bench_case::<u32>(c, "rectangle_3x4_l8", &[3, 4], 0, 8);
    bench_case::<u32>(c, "rectangle_3x4_l11", &[3, 4], 0, 11);
    bench_case::<u32>(c, "rectangle_3x5_l15", &[3, 5], 0, 15);
    bench_case::<u32>(c, "rectangle_3x6_l6", &[3, 6], 0, 6);
    bench_case::<u32>(c, "rectangle_3x7_l11", &[3, 7], 0, 11);
    bench_case::<u32>(c, "rectangle_3x8_l7", &[3, 8], 0, 7);
    bench_case::<u32>(c, "square_3x3_f1_l10", &[3, 3], 1, 10);
    bench_case::<u64>(c, "rectangle_3x11_l3", &[3, 11], 0, 3);
    bench_case::<u128>(c, "rectangle_3x22_l3", &[3, 22], 0, 3);
}

criterion_group!(benches, run);
criterion_main!(benches);
