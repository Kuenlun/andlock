// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! End-to-end counting pipeline: builds the block matrix, drives the DP, and
//! prints the table + summary block. Dispatches the generic counter to its
//! `u32` / `u64` / `u128` monomorphisation per run.

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use console::style;
use indicatif::{HumanBytes, MultiProgress, ProgressBar, ProgressStyle};

use andlock::counter::{DpEvent, DpScratch, count_patterns_dp, dp_mask_ticks, dp_table_bytes};
use andlock::grid::{GridDefinition, compute_blocks};
use andlock::mask::{self, Mask, Width};

use crate::memory::resolve_memory_budget;
use crate::output::{LengthPrinter, RenderedReport, format_count, render_final, style_or_default};
use crate::tty;

#[derive(Copy, Clone)]
pub struct RunOptions {
    pub min_length: usize,
    pub max_length: usize,
    pub memory_limit: Option<u64>,
    pub quiet: bool,
    pub human: bool,
}

fn spinner_style() -> ProgressStyle {
    style_or_default(
        "{prefix:>12.cyan.bold} {spinner} {wide_msg}",
        ProgressStyle::default_spinner,
    )
}

fn bar_style() -> ProgressStyle {
    style_or_default(
        "{prefix:>12.cyan.bold} [{bar:27}] {msg}  eta {eta}",
        ProgressStyle::default_bar,
    )
    .progress_chars("=> ")
}

/// Runs the end-to-end counting pipeline for a single grid.
///
/// # Errors
/// DP scratch allocation failure; the budget estimate in the message points
/// the user at `--max-length` or `--memory-limit`.
///
/// # Panics
/// Panics if `grid.points.len() > mask::MAX_POINTS`. The CLI calls
/// [`GridDefinition::validate`](andlock::grid::GridDefinition::validate)
/// upstream, which rejects oversized grids with a user-facing error.
pub fn run_pipeline(grid: &GridDefinition, opts: RunOptions) -> Result<()> {
    let n = grid.points.len();
    let mp = tty::progress();
    let block_pb = build_block_spinner(mp, n, grid.dimensions, opts.quiet);

    let outcome = match mask::smallest_for(n) {
        Some(Width::U32) => run_dp_sequence::<u32>(grid, n, opts, mp, block_pb.as_ref()),
        Some(Width::U64) => run_dp_sequence::<u64>(grid, n, opts, mp, block_pb.as_ref()),
        Some(Width::U128) => run_dp_sequence::<u128>(grid, n, opts, mp, block_pb.as_ref()),
        None => panic!("n={n} past mask::MAX_POINTS, validate first"),
    }?;

    print_report(
        &outcome.entries,
        &outcome.counts,
        n,
        opts.min_length,
        opts.max_length,
        outcome.effective,
        opts.human,
    );
    if !opts.quiet {
        print_footer(outcome.clamp.map(|_| outcome.effective), outcome.elapsed);
    }
    Ok(())
}

/// Mask-erased outputs the finalisation phase reads.
struct DpRunOutcome {
    counts: Vec<u128>,
    entries: Vec<(usize, u128)>,
    effective: usize,
    clamp: Option<(u64, u64)>,
    elapsed: Duration,
}

/// Width-specialised driver: builds the block matrix, applies the memory
/// clamp, runs the DP, and collects everything finalisation needs.
fn run_dp_sequence<M: Mask>(
    grid: &GridDefinition,
    n: usize,
    opts: RunOptions,
    mp: &MultiProgress,
    block_pb: Option<&ProgressBar>,
) -> Result<DpRunOutcome> {
    let RunOptions {
        min_length,
        max_length,
        memory_limit,
        quiet,
        human,
    } = opts;

    let blocks: Vec<M> = compute_blocks(grid);
    if let Some(pb) = block_pb {
        pb.finish_and_clear();
    }

    // All-zero blocks take the closed-form path that allocates no DP buffer,
    // so the memory clamp must not truncate it (`grid 0 -f 31` would otherwise
    // be capped against a 143 GiB phantom estimate).
    let unconstrained = blocks.iter().all(|&b| b == M::ZERO);
    let (effective, clamp) = resolve_memory_budget(n, max_length, memory_limit, unconstrained);

    if !quiet && let Some((needed, budget)) = clamp {
        print_clamp_warning(effective, needed, budget);
    }

    let count_pb = build_dp_bar(mp, n, effective, quiet);
    let mut printer = LengthPrinter::new(mp, min_length, effective, human, count_pb.as_ref());

    let t1 = Instant::now();
    let counts = drive_dp::<M>(n, &blocks, effective, count_pb.as_ref(), &mut printer)?;
    let elapsed = t1.elapsed();

    let entries = printer.finish();
    if let Some(pb) = count_pb {
        pb.finish_and_clear();
    }

    Ok(DpRunOutcome {
        counts,
        entries,
        effective,
        clamp,
        elapsed,
    })
}

/// Up-front clamp warning, styled like rustc/cargo.
fn print_clamp_warning(effective: usize, needed: u64, budget: u64) {
    let warn = style("warning:").yellow().bold();
    eprintln!(
        "{warn} insufficient memory, run limited to --max-length {effective} \
         (need {}, only {} available)",
        HumanBytes(needed),
        HumanBytes(budget),
    );
}

fn build_block_spinner(
    mp: &MultiProgress,
    n: usize,
    dim: usize,
    quiet: bool,
) -> Option<ProgressBar> {
    if quiet {
        return None;
    }
    let pb = mp.add(ProgressBar::new_spinner());
    pb.set_style(spinner_style());
    pb.set_prefix("Building");
    pb.set_message(format!("block matrix ({n} points, {dim}D)"));
    pb.enable_steady_tick(Duration::from_millis(80));
    Some(pb)
}

fn build_dp_bar(
    mp: &MultiProgress,
    n: usize,
    effective: usize,
    quiet: bool,
) -> Option<ProgressBar> {
    let dp_ticks = dp_mask_ticks(n, effective);
    if quiet || dp_ticks == 0 {
        return None;
    }
    let mem_est = dp_table_bytes(n, effective);
    let pb = mp.add(ProgressBar::new(dp_ticks));
    pb.set_style(bar_style());
    pb.set_prefix("Counting");
    pb.set_message(dp_progress_message(effective.min(1), effective, n, mem_est));
    pb.enable_steady_tick(Duration::from_millis(80));
    Some(pb)
}

fn dp_progress_message(current: usize, effective: usize, n: usize, mem_bytes: u64) -> String {
    format!(
        "length {current} of {effective}, {n} points, ~{}",
        HumanBytes(mem_bytes),
    )
}

/// Allocates scratch and runs the counter, forwarding events to the bar and
/// printer. Each `LengthDone` advances the displayed length in lockstep.
fn drive_dp<M: Mask>(
    n: usize,
    blocks: &[M],
    effective: usize,
    count_pb: Option<&ProgressBar>,
    printer: &mut LengthPrinter<'_>,
) -> Result<Vec<u128>> {
    let mem_est = dp_table_bytes(n, effective);
    let mut scratch = DpScratch::allocate::<M>(n, blocks, effective).map_err(|e| {
        anyhow!(
            "could not allocate ~{} of RAM for the DP buffers: {e}. \
             Lower --max-length or pass --memory-limit to clamp the run to a smaller cap.",
            HumanBytes(mem_est)
        )
    })?;
    Ok(count_patterns_dp(
        &mut scratch,
        n,
        blocks,
        effective,
        |event| match event {
            DpEvent::Mask => {
                if let Some(pb) = count_pb {
                    pb.inc(1);
                }
            }
            DpEvent::LengthDone { length, count } => {
                printer.print(length, count);
                if let Some(pb) = count_pb {
                    let next = (length + 1).min(effective);
                    pb.set_message(dp_progress_message(next, effective, n, mem_est));
                }
            }
        },
    ))
}

/// Paints the table + separator + summary block on stdout. A clamped run
/// omits the `Total` row; the clamp banner went to stderr earlier.
fn print_report(
    entries: &[(usize, u128)],
    counts: &[u128],
    n: usize,
    min_length: usize,
    max_length: usize,
    effective: usize,
    human: bool,
) {
    let total_str = (effective >= max_length)
        .then(|| format_count(counts[min_length..=effective].iter().sum(), human));
    let points_str = n.to_string();
    let RenderedReport {
        table,
        summary,
        separator_width,
    } = render_final(entries, human, total_str.as_deref(), &points_str);

    for line in &table {
        println!("{line}");
    }
    println!("{}", "─".repeat(separator_width));
    for line in &summary {
        println!("{line}");
    }
}

/// `clamp_effective = Some(eff)` restates the truncated cap; `None` is a
/// clean run.
fn print_footer(clamp_effective: Option<usize>, elapsed: Duration) {
    match clamp_effective {
        Some(effective) => eprintln!("  Counted up to length {effective} in {elapsed:.2?}"),
        None => eprintln!("  Counted in {elapsed:.2?}"),
    }
}
