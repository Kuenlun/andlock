// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};

use andlock::canonicalizer::canonicalize;
use andlock::grid::{GridDefinition, build_grid_definition, parse_dims};

use crate::pipeline::{RunOptions, run_pipeline};
use crate::preview::render_preview;

const TOP_EXAMPLES: &str = "\
Examples:
  andlock grid 3x3                  Count all patterns on the Android 3x3 grid
  andlock grid 4x4 --min-length 4   Count Android-style patterns on a 4x4 grid
  andlock file grid.json            Count patterns on a grid loaded from JSON

Run `andlock <command> --help` for command-specific options.";

const GRID_EXAMPLES: &str = "\
Examples:
  andlock grid 3x3
      Count all patterns on the standard Android 3x3 grid.

  andlock grid 4x4 --min-length 4 --max-length 9
      Count Android-style patterns (length 4-9) on a 4x4 grid.

  andlock grid 3x3 --free-points 1
      Add one isolated free point to the 3x3 grid.

  andlock grid 3x3 --export-json > grid.json
      Save the grid to JSON for reuse with `andlock file`.";

const FILE_EXAMPLES: &str = "\
Examples:
  andlock file grid.json
      Count patterns on a grid loaded from a file.

  andlock grid 3x3 --export-json | andlock file -
      Pipe a generated grid through stdin.

  andlock file grid.json --simplify --export-json
      Print the canonical form of a grid.";

/// Count Android-style unlock patterns on n-dimensional grids.
///
/// Use `andlock grid` to generate a rectangular grid on the fly, or
/// `andlock file` to load one from JSON. The empty (length-0) pattern
/// is included in the count unless --min-length excludes it.
#[derive(Parser)]
#[command(
    name = "andlock",
    version,
    after_long_help = TOP_EXAMPLES,
    styles = clap_cargo::style::CLAP_STYLING
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Count patterns on a generated rectangular grid.
    ///
    /// Builds the grid in memory from <DIMS>, runs the counter, and prints
    /// the count for each pattern length. 1D and 2D grids that fit ~40x20
    /// cells get an ASCII preview; larger or 3D+ grids skip it. Use
    /// --export-json to dump the grid for reuse with `andlock file`.
    #[command(after_long_help = GRID_EXAMPLES)]
    Grid {
        /// Axis sizes joined by 'x' (e.g. "3x3", "10", "2X3x2").
        ///
        /// Each component is a non-negative integer; no surrounding whitespace.
        dims: String,

        /// Add N isolated points not collinear with any grid pair.
        ///
        /// Each free point lives on its own extra dimension to guarantee
        /// non-collinearity. Total grid + free points must not exceed 127.
        #[arg(short = 'f', long, default_value_t = 0, value_name = "N")]
        free_points: usize,

        #[command(flatten)]
        range: RangeArgs,

        #[command(flatten)]
        memory: MemoryArgs,

        #[command(flatten)]
        output: OutputArgs,
    },
    /// Count patterns on a grid loaded from JSON.
    ///
    /// Loads a `GridDefinition` (0-127 points) from <PATH> and counts its
    /// patterns. 1D and 2D grids that fit ~40x20 cells get an ASCII preview.
    /// Pass `-` as <PATH> to read from stdin.
    #[command(after_long_help = FILE_EXAMPLES)]
    File {
        /// Path to a JSON `GridDefinition`, or `-` to read from stdin.
        path: PathBuf,

        /// Canonicalize the grid before exporting (requires --export-json).
        ///
        /// Anchors the centroid at the origin and divides each axis by its
        /// coordinate GCD.
        #[arg(long, requires = "export_json", help_heading = "Output")]
        simplify: bool,

        #[command(flatten)]
        range: RangeArgs,

        #[command(flatten)]
        memory: MemoryArgs,

        #[command(flatten)]
        output: OutputArgs,
    },
}

#[derive(Args, Copy, Clone)]
struct OutputArgs {
    /// Print the grid as JSON instead of counting.
    ///
    /// `grid` emits canonical form; `file` re-emits the loaded grid
    /// (combine with --simplify to canonicalize). Redirect with
    /// `> grid.json` to save.
    #[arg(long, help_heading = "Output")]
    export_json: bool,

    /// Suppress progress, timing, and the grid preview.
    ///
    /// Pattern counts are still printed to stdout.
    #[arg(short, long, help_heading = "Output")]
    quiet: bool,

    /// Group long counts with `_` separators (e.g. `140_704`).
    ///
    /// Off by default so the output stays pipe-safe. Uses Rust-style
    /// underscores rather than locale-dependent commas or spaces, so values
    /// can be pasted straight into Rust source.
    #[arg(long, help_heading = "Output")]
    human: bool,
}

#[derive(Args, Copy, Clone)]
struct MemoryArgs {
    /// Cap peak RAM allocation (e.g. 512M, 2GiB).
    ///
    /// Accepts plain bytes ("1024") or values with K/M/G/T suffixes (binary
    /// units; 1 KiB = 1024 B). When the run would allocate more, --max-length
    /// is clamped to the largest length that fits and a `warning:` line
    /// reports the equivalent --max-length value alongside the budget
    /// shortfall.
    ///
    /// Defaults to ~80% of the OS-reported available RAM, sampled once at
    /// startup. The default guards against the DP silently growing into
    /// pagefile/swap.
    #[arg(
        long,
        value_name = "SIZE",
        value_parser = parse_memory_size,
        help_heading = "Resources",
    )]
    memory_limit: Option<u64>,
}

#[derive(Args, Copy, Clone)]
struct RangeArgs {
    /// Skip patterns shorter than N points.
    ///
    /// Defaults to 0 (the empty pattern is included). Use --min-length 4
    /// to match Android's lock-screen minimum.
    #[arg(long, value_name = "N", help_heading = "Pattern length")]
    min_length: Option<usize>,

    /// Skip patterns longer than N points.
    ///
    /// Defaults to the total point count. A tighter cap reduces runtime
    /// because the counter prunes longer prefixes.
    #[arg(long, value_name = "N", help_heading = "Pattern length")]
    max_length: Option<usize>,
}

fn parse_memory_size(s: &str) -> Result<u64, parse_size::Error> {
    parse_size::Config::new().with_binary().parse_size(s)
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

/// # Errors
/// Propagates parse, I/O, and validation errors to the caller.
pub fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Grid {
            dims,
            free_points,
            range,
            memory,
            output,
        } => {
            let parsed = parse_dims(&dims).map_err(|e| anyhow!("{e}"))?;
            let grid = build_grid_definition(&parsed, free_points);
            run_grid(&grid, Some(free_points), range, memory, output)
        }
        Command::File {
            path,
            simplify,
            range,
            memory,
            output,
        } => {
            let (content, src_label) = read_grid_source(&path)?;
            let grid: GridDefinition = serde_json::from_str(&content)
                .map_err(|e| anyhow!("failed to parse JSON from {src_label}: {e}"))?;
            let grid = if output.export_json && simplify {
                canonicalize(&grid)
            } else {
                grid
            };
            run_grid(&grid, None, range, memory, output)
        }
    }
}

/// `known_free_dims` is `Some` only for grids freshly built by `grid`, so the
/// preview can place free-point stars without re-detecting them.
fn run_grid(
    grid: &GridDefinition,
    known_free_dims: Option<usize>,
    range: RangeArgs,
    memory: MemoryArgs,
    output: OutputArgs,
) -> Result<()> {
    let OutputArgs {
        export_json,
        quiet,
        human,
    } = output;

    if export_json {
        if !quiet && (range.min_length.is_some() || range.max_length.is_some()) {
            eprintln!("warning: --min-length and --max-length have no effect with --export-json");
        }
        println!("{}", grid_to_json(grid));
        return Ok(());
    }

    grid.validate().map_err(|e| anyhow!("{e}"))?;
    let (min_length, max_length) = resolve_range(&range, grid.points.len())?;
    if !quiet && let Some(preview) = render_preview(grid, known_free_dims) {
        println!("{preview}");
        println!();
    }
    run_pipeline(
        grid,
        RunOptions {
            min_length,
            max_length,
            memory_limit: memory.memory_limit,
            quiet,
            human,
        },
    )
}

/// Inline JSON layout: one coordinate vector per line, matching the format
/// `andlock file` consumes.
fn grid_to_json(grid: &GridDefinition) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "{{\n  \"dimensions\": {},\n  \"points\": [",
        grid.dimensions
    );
    for (i, p) in grid.points.iter().enumerate() {
        s.push_str(if i == 0 { "\n    [" } else { ",\n    [" });
        for (j, c) in p.iter().enumerate() {
            if j > 0 {
                s.push_str(", ");
            }
            let _ = write!(s, "{c}");
        }
        s.push(']');
    }
    if !grid.points.is_empty() {
        s.push_str("\n  ");
    }
    s.push_str("]\n}");
    s
}

/// Returns the file contents and a label suitable for error messages
/// (`stdin` for `-`, or a quoted path).
fn read_grid_source(path: &Path) -> Result<(String, String)> {
    if path == Path::new("-") {
        let text = io::read_to_string(io::stdin())
            .map_err(|e| anyhow!("could not read from stdin: {e}"))?;
        Ok((text, "stdin".to_owned()))
    } else {
        let text = fs::read_to_string(path)
            .map_err(|e| anyhow!("could not open file \"{}\": {e}", path.display()))?;
        Ok((text, format!("\"{}\"", path.display())))
    }
}
