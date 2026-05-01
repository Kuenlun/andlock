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
use indicatif::{HumanBytes, ProgressBar};

use andlock::counter::{DpEvent, DpScratch, count_patterns_dp, dp_mask_ticks, dp_table_bytes};
use andlock::grid::{GridDefinition, compute_blocks};

use crate::memory::resolve_memory_budget;
use crate::output::{LengthPrinter, bar_style, format_count, render_final, spinner_style};

pub fn run_pipeline(
    grid: &GridDefinition,
    min_length: usize,
    max_length: usize,
    memory_limit: Option<u64>,
    quiet: bool,
    human: bool,
) -> Result<()> {
    let n = grid.points.len();
    let dim = grid.dimensions;

    // Block matrix
    let block_pb = if quiet {
        None
    } else {
        let pb = crate::tty::progress().add(ProgressBar::new_spinner());
        pb.set_style(spinner_style());
        pb.set_message(format!("Building block matrix ({n} points, {dim}D)"));
        pb.enable_steady_tick(Duration::from_millis(80));
        Some(pb)
    };

    let blocks = compute_blocks(grid);

    if let Some(ref pb) = block_pb {
        pb.finish_and_clear();
    }

    // The all-zero block matrix triggers the closed-form fast path inside
    // `count_patterns_dp`, which never allocates the DP buffers. Skipping
    // the memory clamp in that case avoids truncating the run to a length
    // it could trivially compute — e.g. `grid 0 -f 31` ran into the
    // 143 GiB DP estimate even though no DP would actually run.
    let unconstrained = blocks.iter().all(|&b| b == 0);
    let (effective, clamp) = resolve_memory_budget(n, max_length, memory_limit, unconstrained);

    // DP uses a single global bar with one tick per popcount-`p` bitmask
    // visited (`dp_mask_ticks(n, effective)` total). The bar is suppressed
    // when no ticks will fire — both for `--quiet` and for trivially small
    // caps (`effective < 2`) where the popcount loop never runs and an
    // empty bar would otherwise flash on screen.
    let dp_ticks = dp_mask_ticks(n, effective);
    let count_pb: Option<ProgressBar> = if quiet || dp_ticks == 0 {
        None
    } else {
        let mem_est = dp_table_bytes(n, effective);
        let pb = crate::tty::progress().add(ProgressBar::new(dp_ticks));
        pb.set_style(bar_style());
        pb.set_message(format!("{n} points, ~{}", HumanBytes(mem_est)));
        pb.enable_steady_tick(Duration::from_millis(80));
        Some(pb)
    };

    // Per-length lines are printed the moment they are finalized so the
    // user sees results live. The printer also retains them so we can
    // size the trailing separator to the widest row.
    let mut printer = LengthPrinter::new(min_length, effective, human, count_pb.as_ref());
    let t1 = Instant::now();
    let mut scratch = DpScratch::allocate(n, &blocks, effective).map_err(|e| {
        let needed = dp_table_bytes(n, effective);
        anyhow!(
            "could not allocate ~{} of RAM for the DP buffers: {e}. Lower --max-length or pass --memory-limit to clamp the run to a smaller cap.",
            HumanBytes(needed)
        )
    })?;
    let counts = count_patterns_dp(&mut scratch, n, &blocks, effective, |event| match event {
        DpEvent::Mask => {
            if let Some(ref pb) = count_pb {
                pb.inc(1);
            }
        }
        DpEvent::LengthDone { length, count } => printer.print(length, count),
    });
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

    // A clamped run omits the total so partial counts stand on their own;
    // the skip reason and elapsed time appear in the footer on stderr.
    // `Points` qualifies the count so the user does not have to derive it
    // from --max-length or the grid dimensions.
    let total_str = (effective >= max_length)
        .then(|| format_count(counts[min_length..=effective].iter().sum(), human));
    let points_str = n.to_string();
    let (table, summary, sep_width) =
        render_final(&entries, human, total_str.as_deref(), &points_str);

    for line in &table {
        println!("{line}");
    }
    println!("{}", "─".repeat(sep_width));
    for line in &summary {
        println!("{line}");
    }

    if !quiet {
        if effective < max_length {
            if let Some((needed, budget)) = clamp {
                eprintln!(
                    "  Lengths {}–{} skipped — need {}, only {} available",
                    effective + 1,
                    max_length,
                    HumanBytes(needed),
                    HumanBytes(budget),
                );
            }
            eprintln!("  Computed 0–{effective} of 0–{max_length} in {elapsed:.2?}");
        } else {
            eprintln!("  Counted in {elapsed:.2?}");
        }
    }
    Ok(())
}
