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

use anyhow::{Result, anyhow};
use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand};

use andlock::canonicalizer::canonicalize;
use andlock::grid::{GridDefinition, build_grid_definition, parse_dims};

use crate::json_format::pretty_compact_json_value;
use crate::pipeline::{RunOptions, run_pipeline};
use crate::preview::render_preview;

// Cargo-like help colours: green headers, cyan flags. Degrades gracefully when NO_COLOR or non-TTY.
const STYLES: Styles = Styles::styled()
    .header(
        AnsiColor::Green
            .on_default()
            .effects(Effects::BOLD.insert(Effects::UNDERLINE)),
    )
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

// Examples shown only with `--help` (kept out of `-h` so the brief view stays scannable).

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
    styles = STYLES
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
        /// non-collinearity. Total grid + free points must not exceed 31.
        #[arg(short = 'f', long, default_value_t = 0, value_name = "N")]
        free_points: usize,

        /// Print the grid as JSON instead of counting.
        ///
        /// Generated grids are emitted in canonical form. Redirect with
        /// `> grid.json` to save.
        #[arg(long, help_heading = "Output")]
        export_json: bool,

        #[command(flatten)]
        range: RangeArgs,

        #[command(flatten)]
        memory: MemoryArgs,

        /// Suppress progress, timing, and the grid preview.
        ///
        /// Pattern counts are still printed to stdout.
        #[arg(short, long, help_heading = "Output")]
        quiet: bool,

        /// Group long counts with `_` separators (e.g. `140_704`).
        ///
        /// Off by default so the output stays pipe-safe and trivially
        /// machine-parseable. Uses Rust-style underscores rather than
        /// locale-dependent commas or spaces, so values can be pasted
        /// straight into Rust source or any tool that accepts digit
        /// grouping.
        #[arg(long, help_heading = "Output")]
        human: bool,
    },
    /// Count patterns on a grid loaded from JSON.
    ///
    /// Loads a `GridDefinition` (0-31 points) from <PATH> and counts its
    /// patterns. 1D and 2D grids that fit ~40x20 cells get an ASCII preview.
    /// Pass `-` as <PATH> to read from stdin.
    #[command(after_long_help = FILE_EXAMPLES)]
    File {
        /// Path to a JSON `GridDefinition`, or `-` to read from stdin.
        path: PathBuf,

        /// Print the loaded grid as JSON instead of counting.
        ///
        /// Re-emits the grid pretty-printed; combine with --simplify to
        /// canonicalize first.
        #[arg(long, help_heading = "Output")]
        export_json: bool,

        /// Canonicalize the grid before exporting (requires --export-json).
        ///
        /// Translates the grid to the origin and compresses unused axes.
        #[arg(long, requires = "export_json", help_heading = "Output")]
        simplify: bool,

        #[command(flatten)]
        range: RangeArgs,

        #[command(flatten)]
        memory: MemoryArgs,

        /// Suppress progress, timing, and the grid preview.
        ///
        /// Pattern counts are still printed to stdout.
        #[arg(short, long, help_heading = "Output")]
        quiet: bool,

        /// Group long counts with `_` separators (e.g. `140_704`).
        ///
        /// Off by default so the output stays pipe-safe and trivially
        /// machine-parseable. Uses Rust-style underscores rather than
        /// locale-dependent commas or spaces, so values can be pasted
        /// straight into Rust source or any tool that accepts digit
        /// grouping.
        #[arg(long, help_heading = "Output")]
        human: bool,
    },
}

#[derive(Args)]
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

#[derive(Args)]
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

/// Parses `--memory-limit` values like "1024", "512M", "2GiB". Suffixes use
/// binary units (KiB / MiB / …) and are case-insensitive; a bare number is
/// interpreted as raw bytes. Used as a `clap` `value_parser`, so the
/// returned `String` error is rendered into the standard CLI diagnostic.
fn parse_memory_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("memory size is empty".into());
    }
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num_str, suffix) = s.split_at(split);
    let num: u64 = num_str
        .parse()
        .map_err(|_| format!("invalid number in memory size: {s:?}"))?;
    let multiplier: u64 = match suffix.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "ki" | "kib" => 1024,
        "m" | "mb" | "mi" | "mib" => 1024u64.pow(2),
        "g" | "gb" | "gi" | "gib" => 1024u64.pow(3),
        "t" | "tb" | "ti" | "tib" => 1024u64.pow(4),
        other => {
            return Err(format!(
                "unknown memory size suffix {other:?} (expected one of B, K, M, G, T)"
            ));
        }
    };
    num.checked_mul(multiplier)
        .ok_or_else(|| format!("memory size overflows u64: {s:?}"))
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

/// Parses the command line and dispatches to the matching subcommand.
///
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
            memory,
            quiet,
            human,
        } => {
            let parsed = parse_dims(&dims).map_err(|e| anyhow!("{e}"))?;
            let grid = build_grid_definition(&parsed, free_points);

            if export_json {
                warn_ignored_range(&range, quiet);
                println!("{}", pretty_compact_json_value(&grid_as_value(&grid)));
                return Ok(());
            }

            grid.validate().map_err(|e| anyhow!("{e}"))?;
            let (min_length, max_length) = resolve_range(&range, grid.points.len())?;
            if !quiet && let Some(preview) = render_preview(&grid, Some(free_points)) {
                println!("{preview}");
                println!();
            }
            run_pipeline(
                &grid,
                RunOptions {
                    min_length,
                    max_length,
                    memory_limit: memory.memory_limit,
                    quiet,
                    human,
                },
            )?;
        }
        Command::File {
            path,
            export_json,
            simplify,
            range,
            memory,
            quiet,
            human,
        } => {
            let (content, src_label) = read_grid_source(&path)?;
            let grid: GridDefinition = serde_json::from_str(&content)
                .map_err(|e| anyhow!("failed to parse JSON from {src_label}: {e}"))?;

            if export_json {
                warn_ignored_range(&range, quiet);
                let out = if simplify { canonicalize(&grid) } else { grid };
                println!("{}", pretty_compact_json_value(&grid_as_value(&out)));
                return Ok(());
            }

            grid.validate().map_err(|e| anyhow!("{e}"))?;
            let (min_length, max_length) = resolve_range(&range, grid.points.len())?;
            if !quiet && let Some(preview) = render_preview(&grid, None) {
                println!("{preview}");
                println!();
            }
            run_pipeline(
                &grid,
                RunOptions {
                    min_length,
                    max_length,
                    memory_limit: memory.memory_limit,
                    quiet,
                    human,
                },
            )?;
        }
    }

    Ok(())
}

/// Converts a [`GridDefinition`] into a [`serde_json::Value`] without
/// going through the fallible [`serde_json::to_value`]. The raw fields
/// (`usize` and `Vec<Vec<i32>>`) all have infallible `Into<Value>`
/// impls, so this round-trips bit-for-bit identical JSON to the
/// `Serialize`-derived path while letting the export path skip the
/// `?` operator and its unreachable Err arm.
fn grid_as_value(grid: &GridDefinition) -> serde_json::Value {
    let points: Vec<serde_json::Value> = grid
        .points
        .iter()
        .map(|p| serde_json::Value::Array(p.iter().map(|&n| serde_json::Value::from(n)).collect()))
        .collect();
    let mut obj = serde_json::Map::with_capacity(2);
    obj.insert(
        "dimensions".to_owned(),
        serde_json::Value::from(grid.dimensions),
    );
    obj.insert("points".to_owned(), serde_json::Value::Array(points));
    serde_json::Value::Object(obj)
}

/// Reads the grid source: stdin when `path == "-"`, otherwise the file
/// at `path`. Returns the raw text and a human-readable label suitable
/// for embedding in error messages.
fn read_grid_source(path: &std::path::Path) -> Result<(String, String)> {
    let stdin_sentinel = std::path::Path::new("-");
    if path == stdin_sentinel {
        let text = io::read_to_string(io::stdin())
            .map_err(|e| anyhow!("could not read from stdin: {}", io_kind_str(e.kind())))?;
        Ok((text, "stdin".to_owned()))
    } else {
        let text = fs::read_to_string(path).map_err(|e| {
            anyhow!(
                "could not open file \"{}\": {}",
                path.display(),
                io_kind_str(e.kind())
            )
        })?;
        Ok((text, format!("\"{}\"", path.display())))
    }
}

/// Emits the standard warning when `--export-json` is paired with a
/// length flag that the export path ignores. Suppressed in `--quiet`.
fn warn_ignored_range(range: &RangeArgs, quiet: bool) {
    if !quiet && (range.min_length.is_some() || range.max_length.is_some()) {
        eprintln!("warning: --min-length and --max-length have no effect with --export-json");
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn parse_memory_size_accepts_plain_bytes() {
        assert_eq!(parse_memory_size("1024"), Ok(1024));
        assert_eq!(parse_memory_size("0"), Ok(0));
        assert_eq!(parse_memory_size("  2048  "), Ok(2048));
    }

    #[test]
    fn parse_memory_size_accepts_binary_suffixes() {
        assert_eq!(parse_memory_size("1K"), Ok(1024));
        assert_eq!(parse_memory_size("1KiB"), Ok(1024));
        assert_eq!(parse_memory_size("1kb"), Ok(1024));
        assert_eq!(parse_memory_size("2M"), Ok(2 * 1024 * 1024));
        assert_eq!(parse_memory_size("1G"), Ok(1024 * 1024 * 1024));
        assert_eq!(parse_memory_size("1T"), Ok(1024u64.pow(4)));
    }

    #[test]
    fn parse_memory_size_rejects_bad_inputs() {
        assert!(parse_memory_size("").is_err());
        assert!(parse_memory_size("abc").is_err());
        assert!(parse_memory_size("1X").is_err());
        assert!(parse_memory_size("-1").is_err());
        // u64 overflow on the multiplier
        assert!(parse_memory_size("999999999999T").is_err());
    }

    /// Error-kind labels are part of the CLI's user-visible diagnostics,
    /// so the mapping must be exhaustive and stable for every kind
    /// `read_grid_source` may surface. The integration tests already
    /// pin the `NotFound` arm via a missing file; this table fixes the
    /// remaining arms (and the catch-all) at the unit level so a future
    /// reword is caught at compile time rather than as a regression in
    /// the rendered error text.
    #[test]
    fn io_kind_str_labels_every_documented_kind() {
        let cases: &[(io::ErrorKind, &str)] = &[
            (io::ErrorKind::NotFound, "not found"),
            (io::ErrorKind::PermissionDenied, "permission denied"),
            (io::ErrorKind::AlreadyExists, "already exists"),
            (io::ErrorKind::WouldBlock, "operation would block"),
            (io::ErrorKind::InvalidInput, "invalid input"),
            (io::ErrorKind::TimedOut, "timed out"),
            (io::ErrorKind::WriteZero, "write zero"),
            (io::ErrorKind::Interrupted, "interrupted"),
            (io::ErrorKind::ConnectionRefused, "connection refused"),
            (io::ErrorKind::ConnectionReset, "connection reset"),
            (io::ErrorKind::ConnectionAborted, "connection aborted"),
        ];
        for &(kind, expected) in cases {
            assert_eq!(io_kind_str(kind), expected, "kind = {kind:?}");
        }
    }

    /// Any unmapped kind must fall through to the generic label rather
    /// than panicking — `io::ErrorKind` is `#[non_exhaustive]`, so the
    /// catch-all is the only forward-compatible default. `Unsupported`
    /// is not in the explicit table above, so it exercises the `_` arm.
    #[test]
    fn io_kind_str_falls_back_to_generic_label_for_unmapped_kinds() {
        assert_eq!(io_kind_str(io::ErrorKind::Unsupported), "I/O error");
    }

    /// `grid_as_value` is the export path's bridge between an
    /// in-memory `GridDefinition` and `serde_json::Value`; its output
    /// must round-trip through serde so the JSON wire format stays
    /// identical to what the `Serialize` derive emits. We compare
    /// against `serde_json::to_value(&grid)` rather than a hand-built
    /// expectation so a future field addition fails the test loudly
    /// instead of silently diverging the two paths.
    #[test]
    fn grid_as_value_matches_serde_to_value_output() {
        let grid = GridDefinition {
            dimensions: 3,
            points: vec![vec![0, 1, 2], vec![3, 4, 5], vec![]],
        };
        let direct = grid_as_value(&grid);
        let via_serde = serde_json::to_value(&grid).unwrap();
        assert_eq!(direct, via_serde);
    }
}
