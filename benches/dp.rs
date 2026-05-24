// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Baseline benchmarks for [`count_patterns_dp`] across representative grids.

use std::hint::black_box;
use std::ops::ControlFlow;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};

use andlock::canonicalizer::canonicalize;
use andlock::counter::{DpScratch, count_patterns_dp, dp_mask_ticks, effective_max_length};
use andlock::grid::{build_grid_definition, compute_blocks};
use andlock::mask::Mask;

/// Memory cap matching the CLI default, so the bench measures real runs.
const BUDGET: u64 = 1 << 30;

#[allow(clippy::expect_used)]
fn bench_case<M: Mask>(
    c: &mut Criterion,
    label: &str,
    dims: &[i32],
    free_points: usize,
    sample_size: usize,
) {
    let grid = canonicalize(&build_grid_definition(dims, free_points));
    let n = grid.node_count();
    let blocks: Vec<M> = compute_blocks::<M>(&grid);
    let max_length = effective_max_length(n, n, BUDGET);

    let mut group = c.benchmark_group("dp");
    group.sample_size(sample_size);
    group.throughput(Throughput::Elements(dp_mask_ticks(n, max_length)));
    group.bench_function(label, |b| {
        b.iter_batched(
            || DpScratch::allocate::<M>(n, &blocks, max_length).expect("scratch allocation"),
            |mut scratch| {
                count_patterns_dp(&mut scratch, n, &blocks, max_length, |ev| {
                    black_box(&ev);
                    ControlFlow::Continue(())
                });
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn run(c: &mut Criterion) {
    bench_case::<u32>(c, "android_3x3", &[3, 3], 0, 10000);
    bench_case::<u32>(c, "square_3x3_f1", &[3, 3], 1, 10000);
    bench_case::<u32>(c, "square_4x4", &[4, 4], 4, 100);
    bench_case::<u32>(c, "cube_3x3x3", &[3, 3, 3], 0, 50);
    bench_case::<u64>(c, "square_7x8", &[7, 8], 0, 20);
    bench_case::<u128>(c, "square_9x8", &[9, 8], 0, 10);
}

criterion_group!(benches, run);
criterion_main!(benches);
