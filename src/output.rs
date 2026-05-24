// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Result rendering: the live per-length printer streamed from the DP and the
//! final unified table + summary block printed once the run finishes.

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

const LEN_COL_WIDTH: usize = 3;
const GUTTER: usize = 2;
const GAP: usize = 2;
const COUNT_HEADER: &str = "Count";
const TOTAL_LABEL: &str = "Total";
const POINTS_LABEL: &str = "Points";

/// `ProgressStyle::with_template(template)`, swapping in `fallback()` on
/// template parse failure.
pub fn style_or_default(template: &str, fallback: fn() -> ProgressStyle) -> ProgressStyle {
    ProgressStyle::with_template(template).unwrap_or_else(|_| fallback())
}

fn row_style() -> ProgressStyle {
    style_or_default("{msg}", ProgressStyle::default_bar)
}

/// Render a count for display. `human = true` groups digits with `_`
/// matching Rust integer-literal syntax (e.g. `140_704`).
pub fn format_count(count: u128, human: bool) -> String {
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

/// Streams matching per-length rows above a DP progress anchor as a single
/// multi-line bar, widening the count column in place as new values arrive.
/// Rows are buffered silently when no live anchor is available.
///
/// A single bar (rather than one per row) keeps the live table atomic from
/// indicatif's point of view: `MultiProgress::clear` always wipes it whole,
/// so terminal scroll cannot strand the top of the table in scrollback.
pub struct LengthPrinter<'a> {
    min_length: usize,
    max_length: usize,
    human: bool,
    entries: Vec<(usize, u128)>,
    live: Option<LivePrinter<'a>>,
}

struct LivePrinter<'a> {
    mp: &'a MultiProgress,
    anchor: &'a ProgressBar,
    bar: Option<ProgressBar>,
}

impl<'a> LengthPrinter<'a> {
    pub fn new(
        mp: &'a MultiProgress,
        min_length: usize,
        max_length: usize,
        human: bool,
        anchor: Option<&'a ProgressBar>,
    ) -> Self {
        let live = anchor.filter(|a| !a.is_hidden()).map(|anchor| LivePrinter {
            mp,
            anchor,
            bar: None,
        });
        Self {
            min_length,
            max_length,
            human,
            entries: Vec::new(),
            live,
        }
    }

    /// Records a `(length, count)` row, filtered by the range and `count != 0`.
    pub fn print(&mut self, length: usize, count: u128) {
        if length < self.min_length || length > self.max_length || count == 0 {
            return;
        }
        self.entries.push((length, count));
        self.refresh_live();
    }

    fn refresh_live(&mut self) {
        let Self {
            entries,
            human,
            live: Some(live),
            ..
        } = self
        else {
            return;
        };
        let bar = live.bar.get_or_insert_with(|| {
            let bar = live.mp.insert_before(live.anchor, ProgressBar::new(0));
            bar.set_style(row_style());
            bar
        });
        let formatted = format_counts(entries, *human);
        let width = column_width(&formatted);
        bar.set_message(render_table_rows(entries, &formatted, width).join("\n"));
    }

    /// Hides the live bar and returns the collected rows for [`render_final`].
    /// `finish_and_clear` skips the redraw an unfinished bar would otherwise
    /// fire from `Drop`, which would repaint the multi-line table after the
    /// caller's `MultiProgress::clear` and strand its top line in scrollback.
    pub fn finish(mut self) -> Vec<(usize, u128)> {
        if let Some(LivePrinter { bar: Some(bar), .. }) = self.live.take() {
            bar.finish_and_clear();
        }
        self.entries
    }
}

/// Final report: the per-length table, the `Total`/`Points` summary, and the
/// separator width that joins them visually.
pub struct RenderedReport {
    pub table: Vec<String>,
    pub summary: Vec<String>,
    pub separator_width: usize,
}

/// Lay out the table and summary block with every value right-aligned to a
/// shared column edge. `total_str = None` skips the `Total` row, used when a
/// memory clamp truncated the run.
pub fn render_final(
    entries: &[(usize, u128)],
    human: bool,
    total_str: Option<&str>,
    points_str: &str,
) -> RenderedReport {
    let formatted = format_counts(entries, human);
    // Grow the value column so summary labels share the right edge: each
    // summary row uses (label.len() - LEN_COL_WIDTH) extra slack vs a data row.
    let summary_pad = |label: &str, value: &str| {
        value
            .len()
            .saturating_add(label.len())
            .saturating_sub(LEN_COL_WIDTH)
    };
    let mut value_w = column_width(&formatted);
    if let Some(s) = total_str {
        value_w = value_w.max(summary_pad(TOTAL_LABEL, s));
    }
    value_w = value_w.max(summary_pad(POINTS_LABEL, points_str));

    let separator_width = GUTTER + LEN_COL_WIDTH + GAP + value_w;
    let summary_value_width = |label: &str| separator_width - (GUTTER + label.len() + GAP);

    let table = render_table_rows(entries, &formatted, value_w);

    let mut summary = Vec::new();
    if let Some(s) = total_str {
        let w = summary_value_width(TOTAL_LABEL);
        summary.push(format!("  {TOTAL_LABEL}  {s:>w$}"));
    }
    let w = summary_value_width(POINTS_LABEL);
    summary.push(format!("  {POINTS_LABEL}  {points_str:>w$}"));

    RenderedReport {
        table,
        summary,
        separator_width,
    }
}

fn format_counts(entries: &[(usize, u128)], human: bool) -> Vec<String> {
    entries
        .iter()
        .map(|(_, c)| format_count(*c, human))
        .collect()
}

/// Width of the value column: max of the formatted strings and the `Count`
/// header.
fn column_width(formatted: &[String]) -> usize {
    formatted
        .iter()
        .map(String::len)
        .max()
        .unwrap_or(0)
        .max(COUNT_HEADER.len())
}

fn render_table_rows(entries: &[(usize, u128)], formatted: &[String], width: usize) -> Vec<String> {
    if entries.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(entries.len() + 1);
    out.push(format!("  Len  {COUNT_HEADER:>width$}"));
    for ((length, _), value) in entries.iter().zip(formatted) {
        out.push(format!("  {length:>LEN_COL_WIDTH$}  {value:>width$}"));
    }
    out
}
