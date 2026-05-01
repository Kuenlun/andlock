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
use indicatif::{HumanBytes, MultiProgress, ProgressBar, ProgressStyle};

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

use crate::json_format::pretty_compact_json;
use crate::preview::render_preview;
use andlock::canonicalizer::canonicalize;
use andlock::counter::{
    DpEvent, count_patterns_dp, dp_mask_ticks, dp_table_bytes, effective_max_length,
};
use andlock::grid::{GridDefinition, build_grid_definition, compute_blocks, parse_dims};

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
    /// is clamped to the largest length that fits and a warning lists the
    /// skipped lengths.
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

/// One-shot probe of OS-reported available RAM, scaled down to leave
/// headroom for the OS and the rest of the process. Used as the implicit
/// `--memory-limit` when the flag is not passed: `Vec::try_reserve_exact`
/// only fails when virtual address space is exhausted (which on Windows
/// includes the pagefile), so we cannot rely on the allocator alone to
/// keep the run inside physical RAM. No polling — sampled once.
fn detect_memory_budget() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    // Reserve ~20% as headroom (kernel page cache, other processes, the
    // rest of this process). The factor is conservative on purpose:
    // overshooting the budget is the failure mode we are guarding
    // against.
    sys.available_memory().saturating_mul(4) / 5
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

/// Bar style used for each per-length row of the live count table: just
/// the message, no bar/spinner/percentage. Each row is its own
/// `ProgressBar` so we can rewrite them in place when a wider count
/// arrives and forces a column re-alignment.
fn row_style() -> ProgressStyle {
    ProgressStyle::with_template("{msg}").unwrap_or_else(|_| ProgressStyle::default_bar())
}

/// Renders a `u128` count for display. With `human = false` returns the
/// raw decimal so output stays pipe-safe and machine-parseable. With
/// `human = true` groups digits in threes with underscores
/// (e.g. `140_704`), matching Rust integer-literal syntax so values
/// remain locale-neutral and can be pasted directly into source.
fn format_count(count: u128, human: bool) -> String {
    let raw = count.to_string();
    if !human || raw.len() <= 3 {
        return raw;
    }
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len() + (raw.len() - 1) / 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push('_');
        }
        out.push(b as char);
    }
    out
}

/// Collects per-length count rows as the DP finalizes them and renders
/// the count column right-aligned to the widest value seen so far. The
/// header is added lazily on the first matching row so a fully filtered
/// or memory-clamped run never emits an orphan header.
///
/// When a live progress region is available (`anchor` is a visible DP
/// bar), each row is shown immediately as its own line in the
/// `MultiProgress`, and every prior row is rewritten on the fly so the
/// growing column stays aligned. With no live region (quiet runs,
/// non-TTY, or DP with zero ticks), rows are buffered silently. Once DP
/// is done, `finish` clears any live bars and returns the collected
/// entries; the caller paints the final aligned table+summary block via
/// [`render_final`], which may widen the count column further so the
/// `Total` / `Points` rows share the same right edge as the table.
struct LengthPrinter<'a> {
    min_length: usize,
    max_length: usize,
    human: bool,
    entries: Vec<(usize, u128)>,
    live: Option<LivePrinter<'a>>,
}

/// Live-mode state: one `ProgressBar` per displayed line (header + one
/// per row), all stacked above the DP `anchor` bar in a shared
/// `MultiProgress`. Updating a bar's message is what realigns its row.
struct LivePrinter<'a> {
    mp: &'a MultiProgress,
    anchor: &'a ProgressBar,
    header_bar: Option<ProgressBar>,
    row_bars: Vec<ProgressBar>,
}

impl<'a> LengthPrinter<'a> {
    fn new(
        min_length: usize,
        max_length: usize,
        human: bool,
        anchor: Option<&'a ProgressBar>,
    ) -> Self {
        let live = anchor.filter(|a| !a.is_hidden()).map(|anchor| LivePrinter {
            mp: crate::signal::progress(),
            anchor,
            header_bar: None,
            row_bars: Vec::new(),
        });
        Self {
            min_length,
            max_length,
            human,
            entries: Vec::new(),
            live,
        }
    }

    fn print(&mut self, length: usize, count: u128) {
        if length < self.min_length || length > self.max_length || count == 0 {
            return;
        }
        self.entries.push((length, count));
        if let Some(live) = self.live.as_mut() {
            // First matching row also brings in the header line.
            if live.header_bar.is_none() {
                let bar = live.mp.insert_before(live.anchor, ProgressBar::new(0));
                bar.set_style(row_style());
                live.header_bar = Some(bar);
            }
            // Append a fresh bar just above the DP anchor so rows stack
            // top-to-bottom in the order they finalize.
            let bar = live.mp.insert_before(live.anchor, ProgressBar::new(0));
            bar.set_style(row_style());
            live.row_bars.push(bar);
            self.realign_live();
        }
    }

    /// Pushes the freshly recomputed (right-aligned) lines into every
    /// live bar, so older rows widen to match the new column when a
    /// longer count arrives.
    fn realign_live(&self) {
        let Some(live) = self.live.as_ref() else {
            return;
        };
        let lines = self.render_lines();
        if let (Some(bar), Some(header)) = (live.header_bar.as_ref(), lines.first()) {
            bar.set_message(header.clone());
        }
        for (bar, line) in live.row_bars.iter().zip(lines.iter().skip(1)) {
            bar.set_message(line.clone());
        }
    }

    /// Renders the header + per-length rows with the count column
    /// right-aligned to the widest formatted value (or to "Count" when
    /// every value is narrower). Returns an empty vector when no row
    /// has matched, so callers do not paint an orphan header.
    fn render_lines(&self) -> Vec<String> {
        const HEADER: &str = "Count";
        if self.entries.is_empty() {
            return Vec::new();
        }
        let formatted: Vec<String> = self
            .entries
            .iter()
            .map(|(_, c)| format_count(*c, self.human))
            .collect();
        let width = formatted
            .iter()
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(0)
            .max(HEADER.len());
        let mut lines = Vec::with_capacity(self.entries.len() + 1);
        lines.push(format!("  Len  {HEADER:>width$}"));
        for ((length, _), value) in self.entries.iter().zip(formatted.iter()) {
            lines.push(format!("  {length:>3}  {value:>width$}"));
        }
        lines
    }

    /// Tears down any live bars and hands back the collected entries so
    /// the caller can render the final, fully aligned table+summary
    /// block via [`render_final`] (the live bars used a narrower
    /// count column when the summary widens it; the static repaint
    /// replaces them with the unified layout).
    fn finish(mut self) -> Vec<(usize, u128)> {
        if let Some(live) = self.live.take() {
            if let Some(bar) = live.header_bar {
                bar.finish_and_clear();
            }
            for bar in live.row_bars {
                bar.finish_and_clear();
            }
        }
        self.entries
    }
}

/// Renders the per-length table and the trailing summary (`Total` and
/// `Points`) with a unified column layout: every value is right-aligned
/// to the same column edge so the table and summary share a separator
/// width. `total_str = None` skips the `Total` row (used when a memory
/// clamp truncated the run).
///
/// The count column grows as needed: beyond fitting the largest count
/// and the `Count` header, it is widened so `Total` / `Points` can
/// right-align their values to the table's right edge despite their
/// labels being wider than the length column.
fn render_final(
    entries: &[(usize, u128)],
    human: bool,
    total_str: Option<&str>,
    points_str: &str,
) -> (Vec<String>, Vec<String>, usize) {
    let formatted: Vec<String> = entries
        .iter()
        .map(|(_, c)| format_count(*c, human))
        .collect();

    // Length column is 3 wide; the count column grows so summary rows
    // (whose labels are wider than 3) can right-align their values to
    // the same edge as the table. For label `L` with value string `V`:
    //   row_width = 2 + 3 + 2 + value_w   (table)
    //             = 2 + L.len() + 2 + V.len()  (summary, exact)
    // so value_w >= V.len() + L.len() - 3.
    let mut value_w = formatted
        .iter()
        .map(String::len)
        .max()
        .unwrap_or(0)
        .max("Count".len());
    if let Some(s) = total_str {
        value_w = value_w.max(s.len() + "Total".len() - 3);
    }
    value_w = value_w.max(points_str.len() + "Points".len() - 3);

    let row_width = 2 + 3 + 2 + value_w;

    let mut table = Vec::new();
    if !entries.is_empty() {
        table.reserve_exact(entries.len() + 1);
        table.push(format!("  Len  {:>value_w$}", "Count"));
        for ((length, _), value) in entries.iter().zip(formatted.iter()) {
            table.push(format!("  {length:>3}  {value:>value_w$}"));
        }
    }

    let mut summary = Vec::new();
    if let Some(s) = total_str {
        let w = row_width - (2 + "Total".len() + 2);
        summary.push(format!("  Total  {s:>w$}"));
    }
    let w = row_width - (2 + "Points".len() + 2);
    summary.push(format!("  Points  {points_str:>w$}"));

    (table, summary, row_width)
}

/// Resolves the effective `max_length` cap against the active memory
/// budget. The budget comes from `--memory-limit` when present, otherwise
/// from a one-shot probe of OS-reported available RAM (see
/// [`detect_memory_budget`]).
///
/// Returns `(effective, Some((needed_bytes, budget_bytes)))` when the cap
/// is clamped, or `(max_length, None)` when it fits.
fn resolve_memory_budget(
    n: usize,
    max_length: usize,
    memory_limit: Option<u64>,
) -> (usize, Option<(u64, u64)>) {
    let budget = memory_limit.unwrap_or_else(detect_memory_budget);
    let effective = effective_max_length(n, max_length, budget);
    if effective < max_length {
        let needed = dp_table_bytes(n, max_length);
        (effective, Some((needed, budget)))
    } else {
        (effective, None)
    }
}

fn run_pipeline(
    grid: &GridDefinition,
    min_length: usize,
    max_length: usize,
    memory_limit: Option<u64>,
    quiet: bool,
    human: bool,
) -> Result<()> {
    let n = grid.points.len();
    let dim = grid.dimensions;

    let (effective, clamp) = resolve_memory_budget(n, max_length, memory_limit);

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
        let pb = crate::signal::progress().add(ProgressBar::new(dp_ticks));
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
    let counts = count_patterns_dp(n, &blocks, effective, |event| match event {
        DpEvent::Mask => {
            if let Some(ref pb) = count_pb {
                pb.inc(1);
            }
        }
        DpEvent::LengthDone { length, count } => printer.print(length, count),
    })
    .map_err(|e| {
        let needed = dp_table_bytes(n, effective);
        anyhow!(
            "could not allocate ~{} of RAM for the DP buffers: {e}. Lower --max-length or pass --memory-limit to clamp the run to a smaller cap.",
            HumanBytes(needed)
        )
    })?;
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
            run_pipeline(
                &grid,
                min_length,
                max_length,
                memory.memory_limit,
                quiet,
                human,
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
            run_pipeline(
                &grid,
                min_length,
                max_length,
                memory.memory_limit,
                quiet,
                human,
            )?;
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
    fn format_count_raw_when_human_disabled() {
        assert_eq!(format_count(0, false), "0");
        assert_eq!(format_count(1_624, false), "1624");
        assert_eq!(format_count(140_704, false), "140704");
        assert_eq!(
            format_count(162_203_611_691_767_643, false),
            "162203611691767643",
        );
    }

    #[test]
    fn format_count_groups_with_underscores_when_human() {
        // Short numbers stay untouched so we never produce a leading `_`.
        assert_eq!(format_count(0, true), "0");
        assert_eq!(format_count(9, true), "9");
        assert_eq!(format_count(56, true), "56");
        assert_eq!(format_count(320, true), "320");
        // Group boundaries fall on multiples of three from the right.
        assert_eq!(format_count(1_624, true), "1_624");
        assert_eq!(format_count(140_704, true), "140_704");
        assert_eq!(format_count(1_000_000, true), "1_000_000");
        assert_eq!(
            format_count(162_203_611_691_767_643, true),
            "162_203_611_691_767_643",
        );
    }

    #[test]
    fn format_count_handles_u128_extremes() {
        assert_eq!(
            format_count(u128::MAX, true),
            "340_282_366_920_938_463_463_374_607_431_768_211_455",
        );
    }
}
