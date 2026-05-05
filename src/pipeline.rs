/*!
andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
Copyright (C) 2026  Juan Luis Leal Contreras (Kuenlun)

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

//! End-to-end counting pipeline: builds the block matrix, allocates the
//! DP scratch buffers, drives the counter, and prints the table+summary
//! block once the run finishes. Bridges the lib crate's algorithmic
//! pieces with the CLI's progress region and table renderer.

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use indicatif::{HumanBytes, MultiProgress, ProgressBar, ProgressStyle};

use andlock::counter::{DpEvent, DpScratch, count_patterns_dp, dp_mask_ticks, dp_table_bytes};
use andlock::grid::{GridDefinition, compute_blocks};

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

/// Runs the end-to-end counting pipeline for a single grid: builds the
/// block matrix, resolves the active memory budget, allocates the DP
/// scratch, drives the counter, and prints the unified
/// table+summary+footer block.
///
/// # Errors
/// Returns an error if the DP scratch allocation fails (the budget
/// estimate is reported in the message so the user can adjust
/// `--max-length` or `--memory-limit`).
pub fn run_pipeline(grid: &GridDefinition, opts: RunOptions) -> Result<()> {
    let RunOptions {
        min_length,
        max_length,
        memory_limit,
        quiet,
        human,
    } = opts;

    let n = grid.points.len();
    let dim = grid.dimensions;
    let mp = tty::progress();

    let block_pb = build_block_spinner(mp, n, dim, quiet);
    let blocks = compute_blocks(grid);
    if let Some(pb) = block_pb {
        pb.finish_and_clear();
    }

    // The all-zero block matrix triggers the closed-form fast path inside
    // `count_patterns_dp`, which never allocates the DP buffers. Skipping
    // the memory clamp in that case avoids truncating the run to a length
    // it could trivially compute — e.g. `grid 0 -f 31` ran into the
    // 143 GiB DP estimate even though no DP would actually run.
    let unconstrained = blocks.iter().all(|&b| b == 0);
    let (effective, clamp) = resolve_memory_budget(n, max_length, memory_limit, unconstrained);

    let count_pb = build_dp_bar(mp, n, effective, quiet);
    let mut printer = LengthPrinter::new(mp, min_length, effective, human, count_pb.as_ref());

    let dp = DpInputs {
        n,
        blocks: &blocks,
        effective,
    };
    let t1 = Instant::now();
    let counts = drive_dp(dp, count_pb.as_ref(), &mut printer)?;
    let elapsed = t1.elapsed();

    // `finish` clears the live row/header bars in live mode (so the
    // region is empty before we paint the static block) and returns
    // the collected entries; `render_final` then produces a single
    // unified layout where the table and the `Total`/`Points` summary
    // rows share the same right-edge.
    let entries = printer.finish();
    if let Some(pb) = count_pb {
        pb.finish_and_clear();
    }

    print_report(ReportInputs {
        entries: &entries,
        counts: &counts,
        n,
        min_length,
        max_length,
        effective,
        human,
    });
    if !quiet {
        let outcome = match clamp {
            Some((needed, budget)) => RunOutcome::Clamped {
                effective,
                max_length,
                needed,
                budget,
            },
            None => RunOutcome::Complete,
        };
        print_footer(outcome, elapsed);
    }
    Ok(())
}

/// Inputs the counter needs from the pipeline: the grid size `n`, the
/// per-pair block matrix, and the resolved `effective` cap. Bundled so
/// `drive_dp`'s signature stays narrow as the counter grows new knobs.
#[derive(Copy, Clone)]
struct DpInputs<'a> {
    n: usize,
    blocks: &'a [u32],
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
    pb.set_message(format!("{n} points, ~{}", HumanBytes(mem_est)));
    pb.enable_steady_tick(Duration::from_millis(80));
    Some(pb)
}

/// Allocates the DP scratch and runs the counter, forwarding mask
/// ticks to `count_pb` and finalized lengths to `printer`.
fn drive_dp(
    dp: DpInputs<'_>,
    count_pb: Option<&ProgressBar>,
    printer: &mut LengthPrinter<'_>,
) -> Result<Vec<u128>> {
    let DpInputs {
        n,
        blocks,
        effective,
    } = dp;
    let mut scratch = allocate_scratch(n, blocks, effective).map_err(|e| {
        let needed = dp_table_bytes(n, effective);
        anyhow!(
            "could not allocate ~{} of RAM for the DP buffers: {e}. \
             Lower --max-length or pass --memory-limit to clamp the run to a smaller cap.",
            HumanBytes(needed)
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
            DpEvent::LengthDone { length, count } => printer.print(length, count),
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
/// describe it. Pairing the clamped lengths with the byte estimates in a
/// single variant prevents a partial state — a `Clamped` outcome is
/// guaranteed to carry both — so `print_footer` does not need a defensive
/// inner check.
#[derive(Copy, Clone)]
enum RunOutcome {
    Complete,
    Clamped {
        effective: usize,
        max_length: usize,
        needed: u64,
        budget: u64,
    },
}

/// Wraps [`DpScratch::allocate`] so subprocess tests can drive the
/// alloc-failure path through `drive_dp`, exercising the `map_err`
/// closure and the `?` propagation chain in the production binary.
/// In release builds this is a transparent forward; the debug-only
/// hatch substitutes hostile inputs that saturate the underlying
/// `try_reserve_exact` and surface a real `TryReserveError`.
fn allocate_scratch(
    n: usize,
    blocks: &[u32],
    effective: usize,
) -> Result<DpScratch, std::collections::TryReserveError> {
    #[cfg(debug_assertions)]
    if std::env::var_os("ANDLOCK_FORCE_PIPELINE_ERROR").is_some() {
        let hostile = vec![1u32; 64 * 64];
        return DpScratch::allocate(64, &hostile, 64);
    }
    DpScratch::allocate(n, blocks, effective)
}

/// Stderr footer: clamp explanation (when applicable) plus elapsed time.
fn print_footer(outcome: RunOutcome, elapsed: std::time::Duration) {
    match outcome {
        RunOutcome::Clamped {
            effective,
            max_length,
            needed,
            budget,
        } => {
            eprintln!(
                "  Lengths {}–{} skipped — need {}, only {} available",
                effective + 1,
                max_length,
                HumanBytes(needed),
                HumanBytes(budget),
            );
            eprintln!("  Computed 0–{effective} of 0–{max_length} in {elapsed:.2?}");
        }
        RunOutcome::Complete => {
            eprintln!("  Counted in {elapsed:.2?}");
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use indicatif::{MultiProgress, ProgressDrawTarget};

    /// `drive_dp` must surface allocator failures as an actionable
    /// error that names both the byte estimate and the user-facing
    /// remediation flags. We craft an `n` past the algorithm's normal
    /// bound so `dp_layer_capacity` saturates and the underlying
    /// `try_reserve_exact` rejects the request up front; the `?`
    /// returns before the inner counter is invoked, so its own
    /// preconditions are never asserted.
    #[test]
    fn drive_dp_propagates_allocator_failure_with_actionable_message() {
        let mp = MultiProgress::with_draw_target(ProgressDrawTarget::hidden());
        let mut printer = LengthPrinter::new(&mp, 0, 64, false, None);
        let blocks = vec![1u32; 64 * 64];
        let dp = DpInputs {
            n: 64,
            blocks: &blocks,
            effective: 64,
        };

        let err = drive_dp(dp, None, &mut printer).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("could not allocate"),
            "expected size-prefixed message, got: {msg}",
        );
        assert!(
            msg.contains("--max-length") && msg.contains("--memory-limit"),
            "expected remediation flags in: {msg}",
        );
    }
}
