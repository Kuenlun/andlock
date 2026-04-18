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

use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};

use clap::{Args, Parser, Subcommand};

use crate::dp::count_patterns_dp;
use crate::grid::{GridDefinition, build_grid_definition, compute_blocks, parse_dims};

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
    Grid {
        /// Axis sizes separated by 'x' (e.g. "3x3", "10", "2x3x2").
        dims: String,

        /// Append N extra isolated points not collinear with any grid pair (e.g. "3x3 -f 1" adds one free point to the standard 3×3 grid). Total grid + free points must not exceed 25.
        #[arg(short = 'f', long, default_value_t = 0)]
        free_points: usize,

        /// Emit the generated `GridDefinition` as pretty JSON to stdout instead of counting patterns (use `> file.json` to save).
        #[arg(long)]
        export_json: bool,

        #[command(flatten)]
        range: RangeArgs,

        /// Suppress progress and timing output (results still printed to stdout).
        #[arg(short, long)]
        quiet: bool,
    },
    /// Load a `GridDefinition` from a JSON file and count its patterns (0–25 points).
    /// Length 0 (the empty/null pattern) is counted as a valid pattern unless `--min-length` excludes it.
    /// Pass `-` as the path to read from stdin, enabling pipelines like:
    ///   andlock grid "3x3" --export-json | andlock file -
    File {
        /// Path to a JSON file containing a `GridDefinition`, or `-` to read from stdin.
        path: PathBuf,

        /// Re-emit the loaded `GridDefinition` as pretty-printed JSON to stdout instead of counting patterns.
        #[arg(long)]
        export_json: bool,

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

fn run_pipeline(grid: &GridDefinition, min_length: usize, max_length: usize, quiet: bool) {
    let n = grid.points.len();
    let dim = grid.dimensions;

    // Block matrix
    let block_pb = if quiet {
        None
    } else {
        let pb = ProgressBar::new_spinner();
        pb.set_style(spinner_style());
        pb.set_message(format!("Building block matrix ({n} points, {dim}D)"));
        pb.enable_steady_tick(Duration::from_millis(80));
        Some(pb)
    };

    let blocks = compute_blocks(grid);

    if let Some(ref pb) = block_pb {
        pb.finish_and_clear();
    }

    // DP — total masks the outer loop will tick through: 2^n − 1
    let num_masks: u64 = (1u64 << n).saturating_sub(1);
    let dp_pb = if quiet {
        None
    } else {
        let pb = ProgressBar::new(num_masks);
        pb.set_style(bar_style());
        pb.set_message(format!("Counting patterns ({n} points)"));
        pb.enable_steady_tick(Duration::from_millis(80));
        Some(pb)
    };

    let t1 = Instant::now();
    let counts = count_patterns_dp(n, &blocks, max_length, || {
        if let Some(ref pb) = dp_pb {
            pb.inc(1);
        }
    });
    let elapsed = t1.elapsed();

    if let Some(pb) = dp_pb {
        pb.finish_and_clear();
    }

    let total: u128 = counts[min_length..=max_length].iter().sum();
    let mut lines: Vec<String> = Vec::new();
    for (k, c) in counts.iter().enumerate().skip(min_length) {
        if *c > 0 {
            lines.push(format!("  Length {k:>2}: {c}"));
        }
    }
    let total_line = format!("  Total: {total}");
    let sep_width = lines
        .iter()
        .chain(std::iter::once(&total_line))
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(27);
    for line in &lines {
        println!("{line}");
    }
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
                println!("{}", serde_json::to_string_pretty(&grid)?);
                return Ok(());
            }

            grid.validate().map_err(|e| anyhow!("{e}"))?;
            let (min_length, max_length) = resolve_range(&range, grid.points.len())?;
            run_pipeline(&grid, min_length, max_length, quiet);
        }
        Command::File {
            path,
            export_json,
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
                println!("{}", serde_json::to_string_pretty(&grid)?);
                return Ok(());
            }

            grid.validate().map_err(|e| anyhow!("{e}"))?;
            let (min_length, max_length) = resolve_range(&range, grid.points.len())?;
            run_pipeline(&grid, min_length, max_length, quiet);
        }
    }

    Ok(())
}
