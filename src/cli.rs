// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Command-line surface: argument parsing, grid loading, and dispatch into
//! the counting pipeline.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use clap::{Args, CommandFactory, Parser};
use clap_complete::Shell;
use serde::Serialize;
use serde_json::value::RawValue;

use andlock::canonicalizer::canonicalize;
use andlock::grid::{GridDefinition, build_grid_definition, parse_dims};

use crate::pipeline::{RunOptions, run_pipeline};
use crate::preview::render_for_terminal;

const EXAMPLES: &str = "\
Examples:
  andlock 3x3
      Count all patterns on the standard Android 3x3 grid.

  andlock 3x3 --min-length 4
      Count only patterns the Android lock screen accepts (4+ points).

  andlock 3x3x3 --max-length 5
      Count patterns of at most 5 points on a 3D cube.

  andlock 3x3 --free-points 1
      Add one isolated free point to the 3x3 grid.

  andlock --free-points 3
      Count patterns on a grid of 3 isolated free points (no base grid).

  andlock 3x3 --export-json > grid.json
      Save the canonical grid to JSON for reuse.

  andlock 3x3 --min-length 4 --json > counts.json
      Save exact counts and completion status as JSON.

  andlock --file grid.json
      Count patterns on a grid loaded from JSON (`-` reads stdin).

  andlock 3x3 --export-json | andlock --file -
      Pipe a generated grid back through stdin.

  andlock --file grid.json --simplify --export-json
      Print the canonical form of a loaded grid.";

/// Count Android-style unlock patterns on n-dimensional grids.
///
/// Generates a rectangular grid from <DIMS>, or loads one from JSON with
/// `--file`. The empty (length-0) pattern is counted unless `--min-length`
/// excludes it. 1D and 2D grids small enough to fit on screen get an ASCII
/// preview before the run.
#[derive(Parser)]
#[command(
    name = "andlock",
    version,
    after_long_help = EXAMPLES,
    styles = clap_cargo::style::CLAP_STYLING
)]
struct Cli {
    /// Axis sizes joined by 'x' (e.g. "3x3", "10", "2X3x2").
    ///
    /// Each component is a non-negative integer with no surrounding
    /// whitespace. Required unless `--file` or `--free-points` is given.
    #[arg(value_name = "DIMS", conflicts_with = "file")]
    dims: Option<String>,

    /// Load a JSON `GridDefinition` from <PATH>, or `-` to read stdin.
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,

    /// Print a shell completion script for <SHELL> to stdout.
    ///
    /// Source the output to enable tab completion. Supported values:
    /// bash, elvish, fish, powershell, zsh.
    #[arg(long, value_name = "SHELL", exclusive = true)]
    completions: Option<Shell>,

    /// Add N free points to the grid.
    ///
    /// Free points are abstract nodes without coordinates: they sit on
    /// no line and never block any move. Total grid + free points must
    /// not exceed 127. With no <DIMS>, builds a grid of N isolated nodes
    /// (N may be 0 for an explicitly empty grid).
    #[arg(short = 'f', long, value_name = "N", conflicts_with = "file")]
    free_points: Option<usize>,

    #[command(flatten)]
    range: RangeArgs,

    #[command(flatten)]
    memory: MemoryArgs,

    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Args, Copy, Clone)]
#[allow(clippy::struct_excessive_bools)]
struct OutputArgs {
    /// Print the grid as JSON instead of counting.
    ///
    /// Generated grids emit canonical form. Loaded grids are re-emitted
    /// verbatim unless `--simplify` is also passed. Redirect with
    /// `> grid.json` to save.
    #[arg(long, help_heading = "Output")]
    export_json: bool,

    /// Print counts and completion status as one JSON object.
    ///
    /// Includes the grid and requested/completed length ranges. Counts and
    /// totals are decimal strings to preserve u128 precision. Diagnostics
    /// remain on stderr, and incomplete runs keep their failure exit code.
    #[arg(long, conflicts_with_all = ["export_json", "human"], help_heading = "Output")]
    json: bool,

    /// Canonicalize the loaded grid before exporting.
    ///
    /// Divides each axis by the GCD of its coordinate differences, then
    /// anchors the node closest to the centroid at the origin. Requires
    /// `--file` and `--export-json`.
    #[arg(
        long,
        requires = "file",
        requires = "export_json",
        help_heading = "Output"
    )]
    simplify: bool,

    /// Suppress progress, timing, and the grid preview.
    ///
    /// Pattern counts are still printed to stdout. Warnings and errors
    /// remain visible on stderr.
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
    /// Cap counting-table memory (e.g. 512M, 2GiB).
    ///
    /// Accepts bytes or binary K/M/G/T suffixes. If complete layers do not
    /// fit, count smaller prefix partitions without shortening the requested
    /// range. Smaller budgets can require substantially more time; zero uses
    /// traversal without counting tables. Input and output storage are extra.
    ///
    /// Defaults to 80% of available RAM, or 512 MiB if detection is unavailable.
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
    /// Defaults to 0 (the empty pattern is included). Use `--min-length 4`
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
        // Mention --max-length only when the user actually set it.
        return Err(if range.max_length.is_some() {
            anyhow!("--min-length ({min}) must not exceed --max-length ({max})")
        } else {
            anyhow!("--min-length ({min}) exceeds the number of points ({n})")
        });
    }
    Ok((min, max))
}

/// Parses the CLI, loads or builds the grid, and dispatches to the pipeline.
///
/// # Errors
/// Propagates parse, I/O, and validation errors to the caller.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        let name = cmd.get_name().to_owned();
        clap_complete::generate(shell, &mut cmd, name, &mut io::stdout());
        return Ok(());
    }
    let mut grid = match (cli.dims.as_deref(), cli.file.as_deref()) {
        (Some(dims), None) => {
            let parsed = parse_dims(dims).map_err(|e| anyhow!("{e}"))?;
            build_grid_definition(&parsed, cli.free_points.unwrap_or(0))
                .map_err(|e| anyhow!("{e}"))?
        }
        (None, Some(path)) => {
            let (content, src_label) = read_grid_source(path)?;
            serde_json::from_str(&content)
                .map_err(|e| anyhow!("failed to parse JSON from {src_label}: {e}"))?
        }
        (None, None) if cli.free_points.is_some() => GridDefinition {
            dimensions: 0,
            points: Vec::new(),
            free_points: cli.free_points.unwrap_or(0),
        },
        (None, None) => {
            return Err(anyhow!(
                "one of <DIMS>, --file, or --free-points is required"
            ));
        }
        (Some(_), Some(_)) => unreachable!("clap rejects <DIMS> together with --file"),
    };
    grid.validate().map_err(|e| anyhow!("{e}"))?;
    if cli.output.simplify {
        grid = canonicalize(&grid);
    }
    run_grid(&grid, cli.range, cli.memory, cli.output)
}

fn run_grid(
    grid: &GridDefinition,
    range: RangeArgs,
    memory: MemoryArgs,
    output: OutputArgs,
) -> Result<()> {
    let OutputArgs {
        export_json,
        quiet,
        human,
        json,
        ..
    } = output;

    if export_json {
        if range.min_length.is_some() || range.max_length.is_some() {
            eprintln!("warning: --min-length and --max-length have no effect with --export-json");
        }
        println!("{}", grid_to_json(grid)?);
        return Ok(());
    }

    let (min_length, max_length) = resolve_range(&range, grid.node_count())?;
    // The preview is decoration, like progress: it goes to
    // stderr so stdout carries nothing but the counts.
    if !quiet && let Some(preview) = render_for_terminal(grid) {
        eprintln!("{preview}");
        eprintln!();
    }
    run_pipeline(
        grid,
        RunOptions {
            min_length,
            max_length,
            memory_limit: memory.memory_limit,
            quiet,
            human,
            json,
        },
    )
}

/// Inline JSON layout: one coordinate vector per line, matching the format
/// `--file` consumes. `free_points` is omitted when zero so grids without
/// free points round-trip to the minimal representation.
fn grid_to_json(grid: &GridDefinition) -> Result<String> {
    #[derive(Serialize)]
    struct Export {
        dimensions: usize,
        points: Vec<Box<RawValue>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        free_points: Option<usize>,
    }

    let points = grid
        .points
        .iter()
        .map(|p| {
            let coords: Vec<String> = p.iter().map(i32::to_string).collect();
            RawValue::from_string(format!("[{}]", coords.join(", ")))
        })
        .collect::<Result<_, _>>()?;
    let export = Export {
        dimensions: grid.dimensions,
        points,
        free_points: (grid.free_points != 0).then_some(grid.free_points),
    };
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    export.serialize(&mut ser)?;
    Ok(String::from_utf8(buf)?)
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
