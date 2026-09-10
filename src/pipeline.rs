// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! End-to-end counting pipeline: builds the block matrix, drives the DP, and
//! prints the final report. Dispatches the generic counter to its
//! `u32` / `u64` / `u128` monomorphisation per run.

use std::collections::TryReserveError;
use std::ops::ControlFlow;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use console::style;
use indicatif::{HumanBytes, MultiProgress, ProgressBar, ProgressStyle};
use num_bigint::BigUint;

use andlock::counter::{dp_mask_ticks, dp_table_bytes, is_unconstrained};
use andlock::grid::{GridDefinition, compute_blocks};
use andlock::mask::{self, Mask, Width};
use andlock::numeric::{CountEvent, GlobalCount, local_counts_fit};
use andlock::search::{
    count_patterns_bounded_filtered, count_patterns_bounded_with, count_plan_with,
    filtered_table_bytes,
};
use andlock::visits::VisitFilters;

use crate::memory::resolve_memory_budget;
use crate::output::{
    LengthPrinter, LengthRange, RenderedReport, RunStatus, format_count, render_final, render_json,
    style_or_default,
};
use crate::tty;

#[derive(Copy, Clone)]
pub enum Precision {
    Fixed,
    Arbitrary,
}

#[derive(Copy, Clone)]
pub struct RunOptions<'a> {
    pub min_length: usize,
    pub max_length: usize,
    pub memory_limit: Option<u64>,
    pub quiet: bool,
    pub human: bool,
    pub json: bool,
    pub precision: Precision,
    pub visits: Option<&'a VisitFilters>,
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
/// Allocation failure, an incomplete requested range, or a selected-range
/// total that does not fit in the selected representation. Finalised counts are printed before
/// reporting incomplete results.
///
/// # Panics
/// Panics if `grid.node_count() > mask::MAX_POINTS`. The CLI calls
/// [`GridDefinition::validate`](andlock::grid::GridDefinition::validate)
/// upstream, which rejects oversized grids with a user-facing error.
pub fn run_pipeline(grid: &GridDefinition, opts: RunOptions<'_>) -> Result<()> {
    match opts.precision {
        Precision::Fixed => run_with_count::<u128>(grid, opts),
        Precision::Arbitrary => run_with_count::<BigUint>(grid, opts),
    }
}

fn run_with_count<C: GlobalCount>(grid: &GridDefinition, opts: RunOptions<'_>) -> Result<()> {
    let n = grid.node_count();
    let mp = tty::progress();

    let outcome = match mask::smallest_for(n) {
        Some(Width::U32) => run_dp_sequence::<u32, C>(grid, n, opts, mp),
        Some(Width::U64) => run_dp_sequence::<u64, C>(grid, n, opts, mp),
        Some(Width::U128) => run_dp_sequence::<u128, C>(grid, n, opts, mp),
        None => panic!("n={n} past mask::MAX_POINTS, validate first"),
    };

    print_report(&outcome, grid, opts)?;
    print_footer(&outcome, opts);
    if outcome.total.is_none() {
        print_total_overflow_warning();
    }
    outcome.ensure_complete(opts.min_length, opts.max_length)
}

struct DpRunOutcome<C> {
    entries: Vec<(usize, C)>,
    elapsed: Duration,
    status: RunStatus,
    /// Highest finalised length, including lengths excluded from the output.
    last_completed: Option<usize>,
    /// Sum across finalised selected lengths. `None` when the sum overflowed.
    total: Option<C>,
    allocation_error: Option<TryReserveError>,
}

impl<C> DpRunOutcome<C> {
    fn ensure_complete(&self, min_length: usize, max_length: usize) -> Result<()> {
        if let Some(error) = &self.allocation_error {
            return Err(anyhow!(
                "could not allocate counting tables: {error}; lower --memory-limit to use smaller tables"
            ));
        }
        if self.last_completed != Some(max_length) {
            return Err(anyhow!(
                "requested length range {min_length}..={max_length} is incomplete"
            ));
        }
        if self.total.is_none() {
            return Err(anyhow!(
                "total for requested length range {min_length}..={max_length} does not fit in u128"
            ));
        }
        Ok(())
    }
}

fn run_dp_sequence<M: Mask, C: GlobalCount>(
    grid: &GridDefinition,
    n: usize,
    opts: RunOptions<'_>,
    mp: &MultiProgress,
) -> DpRunOutcome<C> {
    let RunOptions {
        min_length,
        max_length,
        memory_limit,
        quiet,
        human,
        ..
    } = opts;

    let block_pb = build_block_spinner(mp, n, grid.dimensions, quiet);
    let blocks: Vec<M> = compute_blocks(grid);
    if let Some(pb) = block_pb {
        pb.finish_and_clear();
    }

    let budget = resolve_memory_budget(memory_limit);
    let allowed = opts.visits.map(VisitFilters::masks::<M>);
    let unconstrained = is_unconstrained(&blocks) && allowed.is_none();
    let partitioned = allowed.is_some()
        || (!unconstrained
            && (dp_table_bytes(n, max_length) > budget
                || (C::ARBITRARY_PRECISION && !local_counts_fit(n, max_length))));
    let peak = allowed.as_ref().map_or_else(
        || {
            if unconstrained {
                0
            } else if partitioned {
                (0..=max_length)
                    .map(|length| count_plan_with::<C>(n, length, budget).table_bytes)
                    .max()
                    .unwrap_or(0)
            } else {
                dp_table_bytes(n, max_length)
            }
        },
        |allowed| filtered_table_bytes::<M, C>(n, &blocks, max_length, budget, allowed),
    );
    let mem_str = HumanBytes(peak).to_string();
    let count_pb = build_dp_bar(mp, n, max_length, &mem_str, quiet, partitioned);
    let mut printer = LengthPrinter::new(mp, min_length, max_length, human, count_pb.as_ref());

    let t1 = Instant::now();
    let progress = drive_dp::<M, C>(
        grid,
        &blocks,
        (max_length, budget),
        allowed.as_deref(),
        &mem_str,
        count_pb.as_ref(),
        &mut printer,
    );
    let last_completed = progress.last_completed;
    let overflow = progress.overflow;
    let elapsed = t1.elapsed();
    let cancelled = tty::is_cancelled();

    if let Some(pb) = count_pb.as_ref() {
        pb.disable_steady_tick();
        pb.finish_and_clear();
    }
    let entries = printer.finish();

    if overflow && let Some(last) = last_completed {
        print_overflow_warning(last);
    }

    let total = entries
        .iter()
        .try_fold(C::default(), |acc, (_, count)| acc.checked_add(count));
    let status = if cancelled {
        RunStatus::Interrupted
    } else if progress.allocation_error.is_some() {
        RunStatus::AllocationFailed
    } else if overflow {
        RunStatus::CountOverflow
    } else if total.is_none() {
        RunStatus::TotalOverflow
    } else {
        RunStatus::Complete
    };

    DpRunOutcome {
        entries,
        elapsed,
        status,
        last_completed,
        total,
        allocation_error: progress.allocation_error,
    }
}

fn print_overflow_warning(last_exact: usize) {
    let warn = style("warning:").yellow().bold();
    eprintln!(
        "{warn} counts past length {last_exact} do not fit in u128, run limited to --max-length {last_exact}",
    );
}

fn print_total_overflow_warning() {
    let warn = style("warning:").yellow().bold();
    eprintln!(
        "{warn} sum across selected lengths does not fit in u128, total omitted from the summary"
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
    mem_str: &str,
    quiet: bool,
    partitioned: bool,
) -> Option<ProgressBar> {
    let dp_ticks = dp_mask_ticks(n, effective);
    if quiet || dp_ticks == 0 {
        return None;
    }
    let pb = mp.add(if partitioned {
        ProgressBar::new_spinner()
    } else {
        ProgressBar::new(dp_ticks)
    });
    pb.set_style(if partitioned {
        spinner_style()
    } else {
        bar_style()
    });
    pb.set_prefix("Counting");
    pb.set_message(dp_progress_message(1, effective, n, mem_str));
    pb.enable_steady_tick(Duration::from_millis(80));
    Some(pb)
}

fn dp_progress_message(current: usize, effective: usize, n: usize, mem_str: &str) -> String {
    format!("length {current} of {effective}, {n} points, ~{mem_str}")
}

/// Progress increments are batched: `ProgressBar::inc` takes a lock, and the
/// DP can fire billions of mask events per run.
const TICK_BATCH: u64 = 1024;

struct CountProgress {
    last_completed: Option<usize>,
    overflow: bool,
    allocation_error: Option<TryReserveError>,
}

/// Drives the DP, forwarding each `LengthDone` to the printer and updating the
/// progress bar in lockstep. The closure breaks on SIGINT so the DP can yield
/// its partial state. Returns the last finalised length and whether the next
/// count overflowed `u128`.
fn drive_dp<M: Mask, C: GlobalCount>(
    grid: &GridDefinition,
    blocks: &[M],
    limits: (usize, u64),
    allowed: Option<&[M]>,
    mem_str: &str,
    count_pb: Option<&ProgressBar>,
    printer: &mut LengthPrinter<'_, C>,
) -> CountProgress {
    let (effective, budget) = limits;
    let n = grid.node_count();
    let mut last_emitted: Option<usize> = None;
    let mut overflow = false;
    let mut ticks: u64 = 0;
    let mut flushed: u64 = 0;

    let forward = |event| {
        match event {
            CountEvent::Mask => {
                if let Some(pb) = count_pb {
                    ticks = ticks.saturating_add(1);
                    if ticks - flushed >= TICK_BATCH {
                        pb.inc(ticks - flushed);
                        flushed = ticks;
                    }
                }
            }
            CountEvent::LengthDone { length, count } => {
                last_emitted = Some(length);
                printer.print(length, count);
                if let Some(pb) = count_pb {
                    pb.inc(ticks - flushed);
                    flushed = ticks;
                    let next = (length + 1).min(effective);
                    pb.set_message(dp_progress_message(next, effective, n, mem_str));
                }
            }
            CountEvent::Overflow => {
                overflow = true;
            }
        }
        if tty::is_cancelled() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let allocation_error = if let Some(allowed) = allowed {
        count_patterns_bounded_filtered(grid, blocks, effective, budget, allowed, forward)
    } else {
        count_patterns_bounded_with(grid, blocks, effective, budget, forward)
    }
    .err();

    CountProgress {
        last_completed: last_emitted,
        overflow,
        allocation_error,
    }
}

/// Prints a JSON report or the per-length table and `Total`/`Points` summary.
/// Totals cover finalized selected entries, including partial runs.
fn print_report<C: GlobalCount>(
    outcome: &DpRunOutcome<C>,
    grid: &GridDefinition,
    opts: RunOptions<'_>,
) -> Result<()> {
    if opts.json {
        println!(
            "{}",
            render_json(
                grid,
                &outcome.entries,
                LengthRange {
                    min_length: opts.min_length,
                    max_length: opts.max_length,
                },
                outcome.last_completed,
                outcome.total.as_ref(),
                outcome.status,
                opts.visits,
            )?
        );
        return Ok(());
    }
    let total_str = outcome
        .total
        .as_ref()
        .filter(|_| !outcome.entries.is_empty())
        .map(|t| format_count(t, opts.human));
    let points_str = grid.node_count().to_string();
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
    Ok(())
}

fn print_footer<C>(outcome: &DpRunOutcome<C>, opts: RunOptions<'_>) {
    let elapsed = outcome.elapsed;
    if outcome.status == RunStatus::Interrupted {
        let last = outcome
            .last_completed
            .map_or_else(String::new, |length| format!(" at length {length}"));
        let timing = if opts.quiet {
            String::new()
        } else {
            format!(" after {elapsed:.2?}")
        };
        eprintln!("  Interrupted{last}{timing}");
        return;
    }
    if !opts.quiet {
        match outcome.last_completed {
            Some(last) if last < opts.max_length => {
                eprintln!("  Counted up to length {last} in {elapsed:.2?}");
            }
            Some(_) => eprintln!("  Counted in {elapsed:.2?}"),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalized_zero_count_completes_the_selected_range() -> Result<()> {
        let mp = MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden());
        let mut printer = LengthPrinter::new(&mp, 2, 2, false, None);
        // Each move requires its unvisited destination, so length 2 has no patterns.
        let blocks = [0u32, 2, 1, 0];
        let grid = andlock::grid::build_grid_definition(&[2], 0).map_err(anyhow::Error::msg)?;
        let progress =
            drive_dp::<_, u128>(&grid, &blocks, (2, 64), None, "64 B", None, &mut printer);
        assert!(!progress.overflow);
        let outcome = DpRunOutcome {
            entries: printer.finish(),
            elapsed: Duration::ZERO,
            status: RunStatus::Complete,
            last_completed: progress.last_completed,
            total: Some(0),
            allocation_error: progress.allocation_error,
        };
        assert_eq!(outcome.entries, [(2, 0)]);
        outcome.ensure_complete(2, 2)
    }
}
