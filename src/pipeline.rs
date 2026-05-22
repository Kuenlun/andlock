// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! End-to-end counting pipeline: builds the block matrix, allocates the
//! DP scratch buffers, drives the counter, and prints the table+summary
//! block once the run finishes. Bridges the lib crate's algorithmic
//! pieces with the CLI's progress region and table renderer.
//!
//! The mask width is picked once per run from `grid.points.len()` via
//! [`andlock::mask::smallest_for`]; the existing hot path on `u32`
//! (`n ≤ 31`) stays byte-identical to before, while wider grids extend
//! the same code through `u64` (`n ≤ 63`) or `u128` (`n ≤ 127`)
//! monomorphisations.

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

/// Knobs that drive a single counting run. Grouped to keep
/// [`run_pipeline`]'s signature stable as new flags land.
#[derive(Copy, Clone)]
pub struct RunOptions {
    pub min_length: usize,
    pub max_length: usize,
    pub memory_limit: Option<u64>,
    pub quiet: bool,
    pub human: bool,
}

/// Spinner style for short, indeterminate phases (e.g. building the
/// block matrix). Mirrors cargo's status-line layout: a 12-column
/// right-aligned bold-cyan verb (set via `set_prefix`), then a spinner
/// and the per-bar message.
fn spinner_style() -> ProgressStyle {
    style_or_default(
        "{prefix:>12.cyan.bold} {spinner} {wide_msg}",
        ProgressStyle::default_spinner,
    )
}

/// Determinate bar style for the DP progress: a 12-column right-aligned
/// bold-cyan verb prefix, a fixed 27-column bracketed bar drawn with
/// `=`/`>`/space, the per-bar message, and an ETA tail.
fn bar_style() -> ProgressStyle {
    style_or_default(
        "{prefix:>12.cyan.bold} [{bar:27}] {msg}  eta {eta}",
        ProgressStyle::default_bar,
    )
    .progress_chars("=> ")
}

/// Runs the end-to-end counting pipeline for a single grid.
///
/// Picks the smallest sufficient [`Mask`] width for the grid, builds the
/// block matrix, resolves the active memory budget, allocates the DP
/// scratch, drives the counter, and prints the unified
/// table+summary+footer block.
///
/// # Errors
/// Returns an error if the DP scratch allocation fails (the budget
/// estimate is reported in the message so the user can adjust
/// `--max-length` or `--memory-limit`).
///
/// # Panics
/// Panics if `grid.points.len()` exceeds [`mask::MAX_POINTS`]; callers
/// must run [`andlock::grid::GridDefinition::validate`] first (the CLI
/// front door does so unconditionally).
pub fn run_pipeline(grid: &GridDefinition, opts: RunOptions) -> Result<()> {
    let RunOptions {
        min_length,
        max_length,
        memory_limit: _,
        quiet,
        human,
    } = opts;

    let n = grid.points.len();
    let dim = grid.dimensions;
    let mp = tty::progress();

    let width = pick_width(n);

    let block_pb = build_block_spinner(mp, n, dim, quiet);

    let outcome = match width {
        Width::U32 => run_dp_sequence::<u32>(grid, n, opts, mp, block_pb.as_ref()),
        Width::U64 => run_dp_sequence::<u64>(grid, n, opts, mp, block_pb.as_ref()),
        Width::U128 => run_dp_sequence::<u128>(grid, n, opts, mp, block_pb.as_ref()),
    }?;

    print_report(ReportInputs {
        entries: &outcome.entries,
        counts: &outcome.counts,
        n,
        min_length,
        max_length,
        effective: outcome.effective,
        human,
    });
    if !quiet {
        let footer_outcome = match outcome.clamp {
            Some(_) => RunOutcome::Clamped {
                effective: outcome.effective,
            },
            None => RunOutcome::Complete,
        };
        print_footer(footer_outcome, outcome.elapsed);
    }
    Ok(())
}

/// Picks the dispatcher width for a grid of `n` points.
///
/// Delegates to [`mask::smallest_for`] for the ladder so the boundary
/// logic lives in exactly one place.
///
/// # Panics
/// Panics when `n > mask::MAX_POINTS`. Callers must run
/// [`andlock::grid::GridDefinition::validate`] first (the CLI front
/// door does so unconditionally), which rejects oversized grids with a
/// user-facing error before they reach this dispatcher — so the panic
/// is a defensive trip-wire rather than a reachable code path.
const fn pick_width(n: usize) -> Width {
    match mask::smallest_for(n) {
        Some(w) => w,
        None => panic!(
            "pick_width called with n past mask::MAX_POINTS — \
             GridDefinition::validate must run before run_pipeline"
        ),
    }
}

/// Bundles the per-run outputs the [`run_pipeline`] finalisation phase
/// needs. Built inside the width-dispatched [`run_dp_sequence`] so the
/// generic [`Mask`]-typed buffers stay encapsulated; the printing
/// pipeline operates on the `Vec<u128>` and `Vec<(usize, u128)>` views
/// the renderer accepts.
struct DpRunOutcome {
    counts: Vec<u128>,
    entries: Vec<(usize, u128)>,
    effective: usize,
    clamp: Option<(u64, u64)>,
    elapsed: Duration,
}

/// Width-specialised driver: builds the block matrix, finishes the
/// build spinner, runs the memory clamp + DP, and collects everything
/// the M-independent finalisation phase needs.
///
/// Generic over [`Mask`]; the runtime caller in [`run_pipeline`]
/// dispatches to this function once per supported width so each
/// monomorphisation stays specialised for its bitmask integer type.
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

    // The all-zero block matrix triggers the closed-form fast path inside
    // `count_patterns_dp`, which never allocates the DP buffers. Skipping
    // the memory clamp in that case avoids truncating the run to a length
    // it could trivially compute — e.g. `grid 0 -f 31` ran into the
    // 143 GiB DP estimate even though no DP would actually run.
    let unconstrained = blocks.iter().all(|&b| b == M::ZERO);
    let (effective, clamp) = resolve_memory_budget(n, max_length, memory_limit, unconstrained);

    if !quiet && let Some((needed, budget)) = clamp {
        print_clamp_warning(effective, needed, budget);
    }

    let count_pb = build_dp_bar(mp, n, effective, quiet);
    let mut printer = LengthPrinter::new(mp, min_length, effective, human, count_pb.as_ref());

    let dp = DpInputs {
        n,
        blocks: &blocks,
        effective,
    };
    let t1 = Instant::now();
    let counts = drive_dp::<M>(dp, count_pb.as_ref(), &mut printer)?;
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

/// Prints the memory-clamp warning to stderr at the start of the run so
/// the user learns the cap before the DP begins, rather than discovering
/// it only when the run finishes. Uses the bold-yellow `warning:` prefix
/// that rustc, clippy, and cargo all share, and names the equivalent
/// `--max-length` value inline so the user can re-run with the same cap
/// declared explicitly.
fn print_clamp_warning(effective: usize, needed: u64, budget: u64) {
    let warn = style("warning:").yellow().bold();
    eprintln!(
        "{warn} insufficient memory, run limited to --max-length {effective} \
         (need {}, only {} available)",
        HumanBytes(needed),
        HumanBytes(budget),
    );
}

/// Inputs the counter needs from the pipeline: the grid size `n`, the
/// per-pair block matrix, and the resolved `effective` cap. Bundled so
/// `drive_dp`'s signature stays narrow as the counter grows new knobs.
#[derive(Copy, Clone)]
struct DpInputs<'a, M: Mask> {
    n: usize,
    blocks: &'a [M],
    effective: usize,
}

/// Inputs needed to paint the final table+summary block: the run's
/// raw outputs (`entries`, `counts`) plus the layout/length context
/// the renderer formats them with.
#[derive(Copy, Clone)]
struct ReportInputs<'a> {
    entries: &'a [(usize, u128)],
    counts: &'a [u128],
    n: usize,
    min_length: usize,
    max_length: usize,
    effective: usize,
    human: bool,
}

/// Spinner shown while `compute_blocks` builds the block matrix. `None`
/// in quiet mode so the function still returns a uniform shape.
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

/// Determinate DP bar with one tick per popcount-`p` bitmask visited
/// (`dp_mask_ticks(n, effective)` total). Suppressed both for `--quiet`
/// and for trivially small caps (`effective < 2`) where no ticks fire
/// and an empty bar would otherwise flash on screen.
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

/// Renders the DP bar's message, foregrounding the three numbers the
/// user cares about most while a long run is in flight: the length
/// currently being computed, the cap the run will reach, and the total
/// number of points the grid carries. The peak DP allocation trails the
/// length info so the user can see at a glance how much memory the
/// chosen cap commits to.
fn dp_progress_message(current: usize, effective: usize, n: usize, mem_bytes: u64) -> String {
    format!(
        "length {current} of {effective}, {n} points, ~{}",
        HumanBytes(mem_bytes),
    )
}

/// Allocates the DP scratch and runs the counter, forwarding mask
/// ticks to `count_pb` and finalized lengths to `printer`. Each
/// `LengthDone` also rewrites the bar's message so the "length X of Y"
/// counter advances in lock-step with the underlying DP.
fn drive_dp<M: Mask>(
    dp: DpInputs<'_, M>,
    count_pb: Option<&ProgressBar>,
    printer: &mut LengthPrinter<'_>,
) -> Result<Vec<u128>> {
    let DpInputs {
        n,
        blocks,
        effective,
    } = dp;
    let mem_est = dp_table_bytes(n, effective);
    let mut scratch = allocate_scratch::<M>(n, blocks, effective).map_err(|e| {
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

/// Paints the unified table + separator + summary block on stdout.
/// A clamped run omits the `Total` row so partial counts stand on
/// their own; the skip reason and elapsed time appear in the footer
/// on stderr. `Points` qualifies the count so the user does not have
/// to derive it from `--max-length` or the grid dimensions.
fn print_report(report: ReportInputs<'_>) {
    let ReportInputs {
        entries,
        counts,
        n,
        min_length,
        max_length,
        effective,
        human,
    } = report;
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

/// Outcome of a finished run, paired with the data the footer needs to
/// describe it. The `Clamped` variant carries the effective cap so the
/// footer can re-state it as a closing reminder of the partial nature
/// of the run, mirroring the up-front `warning:` line.
#[derive(Copy, Clone)]
enum RunOutcome {
    Complete,
    Clamped { effective: usize },
}

/// Wraps [`DpScratch::allocate`] so subprocess tests can drive the
/// alloc-failure path through `drive_dp`, exercising the `map_err`
/// closure and the `?` propagation chain in the production binary.
/// In release builds this is a transparent forward; the debug-only
/// hatch substitutes hostile inputs that saturate the underlying
/// `try_reserve_exact` and surface a real `TryReserveError`.
///
/// The hostile inputs are M-typed so each width's monomorphisation of
/// [`DpScratch::allocate`] is exercised honestly — `allocate_scratch::<u64>`
/// drives `DpScratch::allocate::<u64>`, not the `u32` instantiation.
fn allocate_scratch<M: Mask>(
    n: usize,
    blocks: &[M],
    effective: usize,
) -> Result<DpScratch, std::collections::TryReserveError> {
    #[cfg(debug_assertions)]
    if std::env::var_os("ANDLOCK_FORCE_PIPELINE_ERROR").is_some() {
        // `HOSTILE_N = 128` sits one past the widest mask ceiling, so
        // `dp_layer_capacity(128, 128)` saturates regardless of `M` and
        // the underlying `try_reserve_exact` rejects the request. The
        // alloc step short-circuits before any `n <= MAX_POINTS` assert
        // inside `count_patterns_dp` would fire.
        const HOSTILE_N: usize = 128;
        let mut hostile = vec![M::ZERO; HOSTILE_N * HOSTILE_N];
        // Defeat the all-zero shortcut inside `DpScratch::allocate` so
        // `dp_layer_capacity` actually runs.
        hostile[0] = M::bit(0);
        return DpScratch::allocate::<M>(HOSTILE_N, &hostile, HOSTILE_N);
    }
    DpScratch::allocate::<M>(n, blocks, effective)
}

/// Stderr footer: clamp explanation (when applicable) plus elapsed time.
fn print_footer(outcome: RunOutcome, elapsed: std::time::Duration) {
    match outcome {
        RunOutcome::Clamped { effective } => {
            eprintln!("  Counted up to length {effective} in {elapsed:.2?}");
        }
        RunOutcome::Complete => {
            eprintln!("  Counted in {elapsed:.2?}");
        }
    }
}
