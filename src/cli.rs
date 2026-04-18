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
use std::time::Instant;

use anyhow::{Result, anyhow};

use clap::{Parser, Subcommand};

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
    /// Length 0 (the empty/null pattern) is counted as a valid pattern.
    Grid {
        /// Axis sizes separated by 'x' (e.g. "3x3", "10", "2x3x2").
        dims: String,

        /// Append N extra isolated points not collinear with any grid pair (e.g. "3x3 -f 1" adds one free point to the standard 3×3 grid). Total grid + free points must not exceed 25.
        #[arg(short = 'f', long, default_value_t = 0)]
        free_points: usize,

        /// Emit the generated `GridDefinition` as pretty JSON to stdout instead of counting patterns (use `> file.json` to save).
        #[arg(long)]
        export_json: bool,

        /// Suppress progress and timing output (results still printed to stdout).
        #[arg(short, long)]
        quiet: bool,
    },
    /// Load a `GridDefinition` from a JSON file and count its patterns (0–25 points).
    /// Length 0 (the empty/null pattern) is counted as a valid pattern.
    File {
        /// Path to a JSON file containing a `GridDefinition`.
        path: PathBuf,

        /// Suppress progress and timing output (results still printed to stdout).
        #[arg(short, long)]
        quiet: bool,
    },
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

fn run_pipeline(grid: &GridDefinition, quiet: bool) {
    let n = grid.points.len();
    let dim = grid.dimensions;
    if !quiet {
        eprint!("Computing block matrix for {n} points in {dim}D...");
    }

    let t0 = Instant::now();
    let blocks = compute_blocks(grid);
    if !quiet {
        eprintln!("\nBlock matrix computed in {:?}\n", t0.elapsed());
        eprint!("Computing valid patterns for {n} points...");
    }

    let t1 = Instant::now();
    let counts = count_patterns_dp(n, &blocks);
    let elapsed = t1.elapsed();

    let total: u64 = counts.iter().sum();
    for (k, c) in counts.iter().enumerate() {
        if *c > 0 {
            if k == 0 {
                println!("  Length {k:>2}: {c}  (empty/null pattern)");
            } else {
                println!("  Length {k:>2}: {c}");
            }
        }
    }
    println!("───────────────────────────");
    println!("  Total: {total}");
    if !quiet {
        eprintln!("\n  Time:  {elapsed:?}");
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
            quiet,
        } => {
            let parsed = parse_dims(&dims).map_err(|e| anyhow!("{e}"))?;
            let grid = build_grid_definition(&parsed, free_points);

            if export_json {
                println!("{}", serde_json::to_string_pretty(&grid)?);
                return Ok(());
            }

            grid.validate().map_err(|e| anyhow!("{e}"))?;
            run_pipeline(&grid, quiet);
        }
        Command::File { path, quiet } => {
            let content = fs::read_to_string(&path).map_err(|e| {
                anyhow!(
                    "could not open file \"{}\": {}",
                    path.display(),
                    io_kind_str(e.kind())
                )
            })?;
            let grid: GridDefinition = serde_json::from_str(&content)
                .map_err(|e| anyhow!("failed to parse JSON file \"{}\": {}", path.display(), e))?;
            grid.validate().map_err(|e| anyhow!("{e}"))?;
            run_pipeline(&grid, quiet);
        }
    }

    Ok(())
}
