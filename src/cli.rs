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

use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sysinfo::System;

use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};

use clap::{Args, Parser, Subcommand};

use crate::json_format::pretty_compact_json;
use crate::preview::render_preview;
use andlock::canonicalizer::canonicalize;
use andlock::counter::{
    Algorithm, DfsEvent, DpEvent, choose_algorithm, count_patterns_dfs, count_patterns_dp,
};
use andlock::grid::{GridDefinition, build_grid_definition, compute_blocks, parse_dims};

#[derive(Parser)]
#[command(
    name = "andlock",
    version,
    about = "Count Android-style unlock patterns on n-dimensional nodes"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a rectangular grid on the fly and count its patterns.
    /// Length 0 (the empty/null pattern) is counted as a valid pattern unless `--min-length` excludes it.
    /// An ASCII preview is rendered for 1D/2D grids that fit ~40×20 cells; larger or 3D+ grids skip the preview (use `--export-json` to inspect coordinates).
    Grid {
        /// Axis sizes separated by 'x' (e.g. "3x3", "10", "2x3x2").
        dims: String,

        /// Append N extra isolated points not collinear with any grid pair (e.g. "3x3 -f 1" adds one free point to the standard 3×3 grid). Total grid + free points must not exceed 31.
        #[arg(short = 'f', long, default_value_t = 0)]
        free_points: usize,

        /// Emit the generated `GridDefinition` as pretty JSON to stdout instead of counting patterns (use `> file.json` to save). Generated grids are always emitted in canonical form.
        #[arg(long)]
        export_json: bool,

        #[command(flatten)]
        range: RangeArgs,

        /// Suppress progress and timing output (results still printed to stdout).
        #[arg(short, long)]
        quiet: bool,
    },
    /// Load a `GridDefinition` from a JSON file and count its patterns (0–31 points).
    /// Length 0 (the empty/null pattern) is counted as a valid pattern unless `--min-length` excludes it.
    /// An ASCII preview is rendered for 1D/2D grids that fit ~40×20 cells; larger or 3D+ grids skip the preview.
    /// Pass `-` as the path to read from stdin, enabling pipelines like:
    ///   andlock grid "3x3" --export-json | andlock file -
    File {
        /// Path to a JSON file containing a `GridDefinition`, or `-` to read from stdin.
        path: PathBuf,

        /// Re-emit the loaded `GridDefinition` as pretty-printed JSON to stdout instead of counting patterns.
        #[arg(long)]
        export_json: bool,

        /// Apply canonical-form simplification passes (translate to origin, compress axes) before exporting JSON. Only valid with `--export-json`.
        #[arg(long, requires = "export_json")]
        simplify: bool,

        #[command(flatten)]
        range: RangeArgs,

        /// Suppress progress and timing output (results still printed to stdout).
        #[arg(short, long)]
        quiet: bool,
    },
}

#[derive(Args)]
struct RangeArgs {
    /// Only include patterns with at least N points (e.g. `--min-length 4` matches Android's lock screen minimum). Defaults to 0, i.e. the empty pattern is shown.
    #[arg(long, value_name = "N")]
    min_length: Option<usize>,

    /// Only include patterns with at most N points. The algorithm prunes longer prefixes, so a tight cap exponentially reduces runtime. Defaults to the total point count.
    #[arg(long, value_name = "N")]
    max_length: Option<usize>,
}

fn resolve_range(range: &RangeArgs, n: usize) -> Result<(usize, usize)> {
    let min = range.min_length.unwrap_or(0);
    let max = range.max_length.unwrap_or(n);
    if max > n {
        return Err(anyhow!(
            "--max-length ({max}) exceeds the number of points ({n})"
        ));
    }
    if min > max {
        return Err(anyhow!(
            "--min-length ({min}) must not exceed --max-length ({max})"
        ));
    }
    Ok((min, max))
}

const fn io_kind_str(kind: io::ErrorKind) -> &'static str {
    match kind {
        io::ErrorKind::NotFound => "not found",
        io::ErrorKind::PermissionDenied => "permission denied",
        io::ErrorKind::AlreadyExists => "already exists",
        io::ErrorKind::WouldBlock => "operation would block",
        io::ErrorKind::InvalidInput => "invalid input",
        io::ErrorKind::TimedOut => "timed out",
        io::ErrorKind::WriteZero => "write zero",
        io::ErrorKind::Interrupted => "interrupted",
        io::ErrorKind::ConnectionRefused => "connection refused",
        io::ErrorKind::ConnectionReset => "connection reset",
        io::ErrorKind::ConnectionAborted => "connection aborted",
        _ => "I/O error",
    }
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("  {spinner:.dim} {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner())
}

fn bar_style() -> ProgressStyle {
    ProgressStyle::with_template("  {msg}  [{bar:40.cyan/dim}]  {percent}%  eta {eta}")
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("━━╌")
}

/// Per-pass bar for IDDFS. Each pass counts patterns of a single length, so
/// the `{eta}` reflects the completion of *that* length only — the template
/// spells this out to avoid confusion with a global ETA.
fn iddfs_bar_style() -> ProgressStyle {
    ProgressStyle::with_template("  {msg}  [{bar:40.cyan/dim}]  {percent}%  partial eta {eta}")
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("━━╌")
}

/// Returns 20 % of the memory the OS reports as currently available.
/// Falls back to 0 when the information cannot be obtained, which causes
/// the router to always choose DFS — the safe default.
fn available_memory_budget() -> u64 {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.available_memory() / 5
}

fn print_length(
    length: usize,
    count: u128,
    min_length: usize,
    max_length: usize,
    pb: Option<&ProgressBar>,
    lines: &mut Vec<String>,
) {
    if length >= min_length && length <= max_length && count > 0 {
        let line = format!("  Length {length:>2}: {count}");
        match pb {
            Some(pb) if !pb.is_hidden() => pb.println(&line),
            _ => println!("{line}"),
        }
        lines.push(line);
    }
}

fn run_iddfs(
    n: usize,
    blocks: &[u32],
    max_length: usize,
    min_length: usize,
    quiet: bool,
    lines: &mut Vec<String>,
) -> Vec<u128> {
    // One progress bar per pass: IDDFS counts a single length at a time, and
    // indicatif's `{eta}` on that bar is the ETA to finish *this* length —
    // rendered as "partial eta" by `iddfs_bar_style` so the user never mistakes
    // it for a global estimate.
    let mut pass_pb: Option<ProgressBar> = None;
    count_patterns_dfs(n, blocks, max_length, |event| match event {
        DfsEvent::PassStart { target, pair_total } => {
            if !quiet {
                let pb = crate::signal::progress().add(ProgressBar::new(pair_total));
                pb.set_style(iddfs_bar_style());
                pb.set_message(format!("Counting length {target} (IDDFS)"));
                pb.enable_steady_tick(Duration::from_millis(80));
                pass_pb = Some(pb);
            }
        }
        DfsEvent::PassTick { .. } => {
            if let Some(ref pb) = pass_pb {
                pb.inc(1);
            }
        }
        DfsEvent::LengthDone { length, count } => {
            if let Some(pb) = pass_pb.take() {
                pb.finish_and_clear();
            }
            print_length(length, count, min_length, max_length, None, lines);
        }
    })
}

fn run_pipeline(grid: &GridDefinition, min_length: usize, max_length: usize, quiet: bool) {
    let n = grid.points.len();
    let dim = grid.dimensions;

    // Block matrix
    let block_pb = if quiet {
        None
    } else {
        let pb = crate::signal::progress().add(ProgressBar::new_spinner());
        pb.set_style(spinner_style());
        pb.set_message(format!("Building block matrix ({n} points, {dim}D)"));
        pb.enable_steady_tick(Duration::from_millis(80));
        Some(pb)
    };

    let blocks = compute_blocks(grid);

    if let Some(ref pb) = block_pb {
        pb.finish_and_clear();
    }

    let algorithm = choose_algorithm(n, available_memory_budget());

    // DP uses a single global bar (one tick per bitmask, 2ⁿ − 1 total).
    // IDDFS manages one bar per pass inside its event closure instead.
    let count_pb: Option<ProgressBar> = if quiet || matches!(algorithm, Algorithm::Dfs) {
        None
    } else {
        let pb = crate::signal::progress().add(ProgressBar::new((1u64 << n).saturating_sub(1)));
        pb.set_style(bar_style());
        pb.set_message(format!("Counting patterns ({n} points, DP)"));
        pb.enable_steady_tick(Duration::from_millis(80));
        Some(pb)
    };

    // Per-length lines are printed the moment they are finalized so the user
    // sees results live. We also keep them in `lines` to size the separator.
    let mut lines: Vec<String> = Vec::new();
    let t1 = Instant::now();
    let counts = match algorithm {
        Algorithm::Dp => count_patterns_dp(n, &blocks, max_length, |event| match event {
            DpEvent::Mask => {
                if let Some(ref pb) = count_pb {
                    pb.inc(1);
                }
            }
            DpEvent::LengthDone { length, count } => {
                print_length(
                    length,
                    count,
                    min_length,
                    max_length,
                    count_pb.as_ref(),
                    &mut lines,
                );
            }
        }),
        // IDDFS emits final per-length counts one pass at a time.
        Algorithm::Dfs => run_iddfs(n, &blocks, max_length, min_length, quiet, &mut lines),
    };
    let elapsed = t1.elapsed();

    if let Some(pb) = count_pb {
        pb.finish_and_clear();
    }

    let total: u128 = counts[min_length..=max_length].iter().sum();
    let total_line = format!("  Total: {total}");
    let sep_width = lines
        .iter()
        .chain(std::iter::once(&total_line))
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(27);
    println!("{}", "─".repeat(sep_width));
    println!("{total_line}");
    if !quiet {
        eprintln!("  [Finished] Patterns counted in {elapsed:.2?}");
    }
}

/// # Errors
/// Propagates parse, I/O, and validation errors to the caller.
pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Grid {
            dims,
            free_points,
            export_json,
            range,
            quiet,
        } => {
            let parsed = parse_dims(&dims).map_err(|e| anyhow!("{e}"))?;
            let grid = build_grid_definition(&parsed, free_points);

            if export_json {
                if range.min_length.is_some() || range.max_length.is_some() {
                    return Err(anyhow!(
                        "--min-length and --max-length have no effect with --export-json"
                    ));
                }
                println!("{}", pretty_compact_json(&grid)?);
                return Ok(());
            }

            grid.validate().map_err(|e| anyhow!("{e}"))?;
            let (min_length, max_length) = resolve_range(&range, grid.points.len())?;
            if !quiet && let Some(preview) = render_preview(&grid, Some(free_points)) {
                println!("{preview}");
                println!();
            }
            run_pipeline(&grid, min_length, max_length, quiet);
        }
        Command::File {
            path,
            export_json,
            simplify,
            range,
            quiet,
        } => {
            let stdin_sentinel = std::path::Path::new("-");
            let (content, src_label) = if path == stdin_sentinel {
                let text = io::read_to_string(io::stdin())
                    .map_err(|e| anyhow!("could not read from stdin: {}", io_kind_str(e.kind())))?;
                (text, "stdin".to_owned())
            } else {
                let text = fs::read_to_string(&path).map_err(|e| {
                    anyhow!(
                        "could not open file \"{}\": {}",
                        path.display(),
                        io_kind_str(e.kind())
                    )
                })?;
                (text, format!("\"{}\"", path.display()))
            };
            let grid: GridDefinition = serde_json::from_str(&content)
                .map_err(|e| anyhow!("failed to parse JSON from {src_label}: {e}"))?;

            if export_json {
                if range.min_length.is_some() || range.max_length.is_some() {
                    return Err(anyhow!(
                        "--min-length and --max-length have no effect with --export-json"
                    ));
                }
                let out = if simplify { canonicalize(&grid) } else { grid };
                println!("{}", pretty_compact_json(&out)?);
                return Ok(());
            }

            grid.validate().map_err(|e| anyhow!("{e}"))?;
            let (min_length, max_length) = resolve_range(&range, grid.points.len())?;
            if !quiet && let Some(preview) = render_preview(&grid, None) {
                println!("{preview}");
                println!();
            }
            run_pipeline(&grid, min_length, max_length, quiet);
        }
    }

    Ok(())
}
