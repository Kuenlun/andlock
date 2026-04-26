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

use clap::ValueEnum;
use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand};

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

use crate::json_format::pretty_compact_json;
use crate::preview::render_preview;
use andlock::canonicalizer::canonicalize;
use andlock::counter::{
    Algorithm, DfsEvent, DpEvent, choose_algorithm, count_patterns_dfs, count_patterns_dp,
    dp_mask_ticks, dp_table_bytes,
};
use andlock::grid::{GridDefinition, build_grid_definition, compute_blocks, parse_dims};

/// Default memory budget consulted by `--algorithm auto` when the user does
/// not pass `--memory-limit`. 1 GiB lets the layered DP run every
/// supported `n ≤ 24` problem (the 4×4 + 8 free-points monotonicity test
/// peaks at ~990 MiB) and falls back to DFS for `n ≥ 25`, where the peak
/// layer pair grows past 2 GiB. The DP no longer allocates a `2ⁿ`
/// mask→index lookup, so this budget is now driven entirely by the DP
/// layers themselves.
const DEFAULT_MEMORY_LIMIT: &str = "1G";

#[derive(Parser)]
#[command(
    name = "andlock",
    version,
    about = "Count Android-style unlock patterns on n-dimensional nodes",
    styles = STYLES
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
        /// Axis sizes separated by 'x' or 'X', with no surrounding whitespace (e.g. "3x3", "10", "2X3x2"). Each component must be a non-negative integer.
        dims: String,

        /// Append N extra isolated points not collinear with any grid pair (e.g. "3x3 -f 1" adds one free point to the standard 3×3 grid). Each free point lives on its own extra dimension to guarantee non-collinearity. Total grid + free points must not exceed 31.
        #[arg(short = 'f', long, default_value_t = 0)]
        free_points: usize,

        /// Emit the generated `GridDefinition` as pretty JSON to stdout instead of counting patterns (use `> file.json` to save). Generated grids are always emitted in canonical form.
        #[arg(long)]
        export_json: bool,

        #[command(flatten)]
        range: RangeArgs,

        #[command(flatten)]
        engine: EngineArgs,

        /// Suppress progress, timing output, and the ASCII grid preview (results still printed to stdout).
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

        #[command(flatten)]
        engine: EngineArgs,

        /// Suppress progress, timing output, and the ASCII grid preview (results still printed to stdout).
        #[arg(short, long)]
        quiet: bool,
    },
}

/// User-selectable counting algorithm. `Auto` defers to [`choose_algorithm`];
/// the other two variants force the underlying counter regardless of the
/// memory estimate (useful for benchmarks and reproducibility).
#[derive(Clone, Copy, Debug, ValueEnum)]
enum AlgorithmChoice {
    /// Pick DP if its estimated peak fits in `--memory-limit`, else DFS.
    Auto,
    /// Force the bitmask DP. Will allocate the full table even if it exceeds the limit.
    Dp,
    /// Force IDDFS. O(n) memory at the cost of substantially more CPU work.
    Dfs,
}

#[derive(Args)]
struct EngineArgs {
    /// Counting algorithm: `auto` (default) routes by estimated DP memory; `dp` forces the bitmask DP; `dfs` forces IDDFS. Forcing `dp` past the memory budget can swap or OOM on big grids — use deliberately.
    #[arg(long, value_enum, default_value_t = AlgorithmChoice::Auto)]
    algorithm: AlgorithmChoice,

    /// Memory budget for `--algorithm auto`. Accepts plain bytes ("1024") or values with K/M/G/T suffixes ("512M", "1G", "2GiB"); suffixes use binary units (1 KiB = 1024 B). Ignored when `--algorithm` is `dp` or `dfs`.
    #[arg(
        long,
        value_name = "SIZE",
        default_value = DEFAULT_MEMORY_LIMIT,
        value_parser = parse_memory_size,
    )]
    memory_limit: u64,
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

/// Parses `--memory-limit` values like "1024", "512M", "2GiB". Suffixes use
/// binary units (KiB / MiB / …) and are case-insensitive; a bare number is
/// interpreted as raw bytes. Used as a `clap` `value_parser`, so the returned
/// `String` error is rendered into the standard CLI diagnostic.
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

/// Renders a byte count in the largest binary unit ≥ 1, with one decimal
/// place. Pure integer arithmetic so the project's `clippy::pedantic` ban
/// on precision-loss casts holds.
fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    const TIB: u64 = GIB * 1024;
    if bytes >= TIB {
        let whole = bytes / TIB;
        let frac = (bytes % TIB) * 10 / TIB;
        format!("{whole}.{frac} TiB")
    } else if bytes >= GIB {
        let whole = bytes / GIB;
        let frac = (bytes % GIB) * 10 / GIB;
        format!("{whole}.{frac} GiB")
    } else if bytes >= MIB {
        let whole = bytes / MIB;
        let frac = (bytes % MIB) * 10 / MIB;
        format!("{whole}.{frac} MiB")
    } else if bytes >= KIB {
        let whole = bytes / KIB;
        let frac = (bytes % KIB) * 10 / KIB;
        format!("{whole}.{frac} KiB")
    } else {
        format!("{bytes} B")
    }
}

/// Resolves the user's `--algorithm` choice into the concrete [`Algorithm`]
/// the counter will run, given the problem size and budget. When `quiet` is
/// `false`, prints a one-line diagnostic explaining the decision.
fn resolve_algorithm(
    choice: AlgorithmChoice,
    n: usize,
    max_length: usize,
    memory_limit: u64,
    quiet: bool,
) -> Algorithm {
    let algorithm = match choice {
        AlgorithmChoice::Auto => choose_algorithm(n, max_length, memory_limit),
        AlgorithmChoice::Dp => Algorithm::Dp,
        AlgorithmChoice::Dfs => Algorithm::Dfs,
    };
    if !quiet {
        let line = match (choice, algorithm) {
            (AlgorithmChoice::Auto, Algorithm::Dp) => "  [Auto] Selected DP".to_owned(),
            // Phrased as a delta over the budget so the message stays
            // unambiguous even when the estimate and budget round to
            // identical strings (e.g. both "1.0 GiB" when DP overshoots by
            // a few KiB) — and tells the user exactly how much to raise
            // `--memory-limit` if they want DP after all.
            (AlgorithmChoice::Auto, Algorithm::Dfs) => {
                let overflow =
                    format_bytes(dp_table_bytes(n, max_length).saturating_sub(memory_limit));
                let limit_fmt = format_bytes(memory_limit);
                format!(
                    "  [Auto] Selected DFS: DP would need {overflow} more than the \
                     {limit_fmt} budget (raise --memory-limit or pass --algorithm dp)"
                )
            }
            (AlgorithmChoice::Dp, _) => "  [Forced] DP".to_owned(),
            (AlgorithmChoice::Dfs, _) => "  [Forced] DFS".to_owned(),
        };
        eprintln!("{line}");
    }
    algorithm
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

fn run_pipeline(
    grid: &GridDefinition,
    min_length: usize,
    max_length: usize,
    engine: &EngineArgs,
    quiet: bool,
) {
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

    let algorithm = resolve_algorithm(engine.algorithm, n, max_length, engine.memory_limit, quiet);

    // DP uses a single global bar with one tick per popcount-`p` bitmask
    // visited (`dp_mask_ticks(n, max_length)` total). The bar is suppressed
    // when no ticks will fire — both for `--quiet` and for trivially small
    // caps (`max_length < 2`) where the popcount loop never runs and an
    // empty bar would otherwise flash on screen.
    // IDDFS manages one bar per pass inside its event closure instead.
    let dp_ticks = dp_mask_ticks(n, max_length);
    let count_pb: Option<ProgressBar> =
        if quiet || matches!(algorithm, Algorithm::Dfs) || dp_ticks == 0 {
            None
        } else {
            let pb = crate::signal::progress().add(ProgressBar::new(dp_ticks));
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
            engine,
            quiet,
        } => {
            let parsed = parse_dims(&dims).map_err(|e| anyhow!("{e}"))?;
            let grid = build_grid_definition(&parsed, free_points);

            if export_json {
                if !quiet && (range.min_length.is_some() || range.max_length.is_some()) {
                    eprintln!(
                        "warning: --min-length and --max-length have no effect with --export-json"
                    );
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
            run_pipeline(&grid, min_length, max_length, &engine, quiet);
        }
        Command::File {
            path,
            export_json,
            simplify,
            range,
            engine,
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
                if !quiet && (range.min_length.is_some() || range.max_length.is_some()) {
                    eprintln!(
                        "warning: --min-length and --max-length have no effect with --export-json"
                    );
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
            run_pipeline(&grid, min_length, max_length, &engine, quiet);
        }
    }

    Ok(())
}

#[cfg(test)]
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

    #[test]
    fn parse_memory_size_default_constant_round_trips() {
        // The CLI's default budget string must parse cleanly.
        assert_eq!(
            parse_memory_size(DEFAULT_MEMORY_LIMIT),
            Ok(1024 * 1024 * 1024)
        );
    }

    #[test]
    fn format_bytes_picks_appropriate_unit() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GiB");
        assert_eq!(format_bytes(1536), "1.5 KiB");
    }
}
