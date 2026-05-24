// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! End-to-end counting pipeline: builds the block matrix, drives the DP, and
//! prints the table + summary block. Dispatches the generic counter to its
//! `u32` / `u64` / `u128` monomorphisation per run.

use std::ops::ControlFlow;
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
/// Panics if `grid.node_count() > mask::MAX_POINTS`. The CLI calls
/// [`GridDefinition::validate`](andlock::grid::GridDefinition::validate)
/// upstream, which rejects oversized grids with a user-facing error.
pub fn run_pipeline(grid: &GridDefinition, opts: RunOptions) -> Result<()> {
    let n = grid.node_count();
    let mp = tty::progress();

    let outcome = match mask::smallest_for(n) {
        Some(Width::U32) => run_dp_sequence::<u32>(grid, n, opts, mp),
        Some(Width::U64) => run_dp_sequence::<u64>(grid, n, opts, mp),
        Some(Width::U128) => run_dp_sequence::<u128>(grid, n, opts, mp),
        None => panic!("n={n} past mask::MAX_POINTS, validate first"),
    }?;

    print_report(&outcome, n, opts);
    if !opts.quiet {
        print_footer(&outcome);
        if outcome.total.is_none() && !outcome.entries.is_empty() {
            print_total_overflow_warning();
        }
    }
    Ok(())
}

struct DpRunOutcome {
    entries: Vec<(usize, u128)>,
    effective: usize,
    clamp: Option<(u64, u64)>,
    elapsed: Duration,
    cancelled: bool,
    /// Highest length that was counted exactly before the DP overflowed `u128`;
    /// `None` when no overflow happened.
    overflow_after: Option<usize>,
    /// Sum across every finalised length; `None` when the sum itself overflowed.
    total: Option<u128>,
}

fn run_dp_sequence<M: Mask>(
    grid: &GridDefinition,
    n: usize,
    opts: RunOptions,
    mp: &MultiProgress,
) -> Result<DpRunOutcome> {
    let RunOptions {
        min_length,
        max_length,
        memory_limit,
        quiet,
        human,
    } = opts;

    let block_pb = build_block_spinner(mp, n, grid.dimensions, quiet);
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
    let overflow_after = drive_dp::<M>(n, &blocks, effective, count_pb.as_ref(), &mut printer)?;
    let elapsed = t1.elapsed();
    let cancelled = tty::is_cancelled();

    if let Some(pb) = count_pb.as_ref() {
        pb.disable_steady_tick();
        pb.finish_and_clear();
    }
    let entries = printer.finish();
    drop(count_pb);

    if !quiet && let Some(last) = overflow_after {
        print_overflow_warning(last);
    }

    let total = entries
        .iter()
        .try_fold(0u128, |acc, &(_, c)| acc.checked_add(c));

    Ok(DpRunOutcome {
        entries,
        effective,
        clamp,
        elapsed,
        cancelled,
        overflow_after,
        total,
    })
}

fn print_clamp_warning(effective: usize, needed: u64, budget: u64) {
    let warn = style("warning:").yellow().bold();
    eprintln!(
        "{warn} insufficient memory, run limited to --max-length {effective} \
         (need {}, only {} available)",
        HumanBytes(needed),
        HumanBytes(budget),
    );
}

fn print_overflow_warning(last_exact: usize) {
    let warn = style("warning:").yellow().bold();
    eprintln!(
        "{warn} counts past length {last_exact} do not fit in u128, run limited to --max-length {last_exact}",
    );
}

fn print_total_overflow_warning() {
    let warn = style("warning:").yellow().bold();
    eprintln!("{warn} sum across all lengths overflows, omitted from the summary");
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
    pb.set_message(dp_progress_message(1, effective, n, mem_est));
    pb.enable_steady_tick(Duration::from_millis(80));
    Some(pb)
}

fn dp_progress_message(current: usize, effective: usize, n: usize, mem_bytes: u64) -> String {
    format!(
        "length {current} of {effective}, {n} points, ~{}",
        HumanBytes(mem_bytes),
    )
}

/// Each `LengthDone` advances the displayed length in lockstep with the DP.
/// Returns [`ControlFlow::Break`] on SIGINT so the DP yields its partial state.
/// Result is `Some(last_exact_length)` when the DP stopped because the next
/// count would not fit in `u128`, `None` otherwise.
fn drive_dp<M: Mask>(
    n: usize,
    blocks: &[M],
    effective: usize,
    count_pb: Option<&ProgressBar>,
    printer: &mut LengthPrinter<'_>,
) -> Result<Option<usize>> {
    let mem_est = dp_table_bytes(n, effective);
    let mut scratch = DpScratch::allocate::<M>(n, blocks, effective).map_err(|e| {
        anyhow!(
            "could not allocate ~{} of RAM for the DP buffers: {e}. \
             Lower --max-length or pass --memory-limit to clamp the run to a smaller cap.",
            HumanBytes(mem_est)
        )
    })?;

    let mut last_emitted: Option<usize> = None;
    let mut overflow = false;

    count_patterns_dp(&mut scratch, n, blocks, effective, |event| {
        match event {
            DpEvent::Mask => {
                if let Some(pb) = count_pb {
                    pb.inc(1);
                }
            }
            DpEvent::LengthDone { length, count } => {
                last_emitted = Some(length);
                printer.print(length, count);
                if let Some(pb) = count_pb {
                    let next = (length + 1).min(effective);
                    pb.set_message(dp_progress_message(next, effective, n, mem_est));
                }
            }
            DpEvent::Overflow => {
                overflow = true;
            }
        }
        if tty::is_cancelled() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });

    Ok(overflow.then(|| last_emitted.unwrap_or(0)))
}

/// Renders the per-length table, the separator, and the `Total`/`Points`
/// summary. `Total` sums every finalised entry, so clamped or interrupted
/// runs still show the partial total of what was counted; an empty table
/// omits the separator.
fn print_report(outcome: &DpRunOutcome, n: usize, opts: RunOptions) {
    let total_str = outcome
        .total
        .filter(|_| !outcome.entries.is_empty())
        .map(|t| format_count(t, opts.human));
    let points_str = n.to_string();
    let RenderedReport {
        table,
        summary,
        separator_width,
    } = render_final(
        &outcome.entries,
        opts.human,
        total_str.as_deref(),
        &points_str,
    );

    for line in &table {
        println!("{line}");
    }
    if !outcome.entries.is_empty() {
        println!("{}", "─".repeat(separator_width));
    }
    for line in &summary {
        println!("{line}");
    }
}

fn print_footer(outcome: &DpRunOutcome) {
    let elapsed = outcome.elapsed;
    if outcome.cancelled {
        match outcome.entries.last() {
            Some(&(l, _)) => eprintln!("  Interrupted at length {l} after {elapsed:.2?}"),
            None => eprintln!("  Interrupted after {elapsed:.2?}"),
        }
    } else if let Some(last) = outcome.overflow_after {
        eprintln!("  Counted up to length {last} in {elapsed:.2?}");
    } else if outcome.clamp.is_some() {
        eprintln!(
            "  Counted up to length {} in {elapsed:.2?}",
            outcome.effective
        );
    } else {
        eprintln!("  Counted in {elapsed:.2?}");
    }
}
