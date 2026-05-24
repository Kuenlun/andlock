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

/// Streams matching per-length rows above a DP progress anchor, widening the
/// count column in place as new values arrive. Rows are buffered silently
/// when no live anchor is available.
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
    header_bar: Option<ProgressBar>,
    row_bars: Vec<ProgressBar>,
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

    /// Records a `(length, count)` row, filtered by the range and `count != 0`.
    pub fn print(&mut self, length: usize, count: u128) {
        if length < self.min_length || length > self.max_length || count == 0 {
            return;
        }
        self.entries.push((length, count));
        if let Some(live) = self.live.as_mut() {
            if live.header_bar.is_none() {
                live.header_bar = Some(live.fresh_bar());
            }
            live.row_bars.push(live.fresh_bar());
            self.realign_live();
        }
    }

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

    fn render_lines(&self) -> Vec<String> {
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
            .map(String::len)
            .max()
            .unwrap_or(0)
            .max(COUNT_HEADER.len());
        let mut lines = Vec::with_capacity(self.entries.len() + 1);
        lines.push(format!("  Len  {COUNT_HEADER:>width$}"));
        for ((length, _), value) in self.entries.iter().zip(formatted.iter()) {
            lines.push(format!("  {length:>LEN_COL_WIDTH$}  {value:>width$}"));
        }
        lines
    }

    /// Tears down live bars and returns the collected rows for [`render_final`].
    pub fn finish(mut self) -> Vec<(usize, u128)> {
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

impl LivePrinter<'_> {
    fn fresh_bar(&self) -> ProgressBar {
        let bar = self.mp.insert_before(self.anchor, ProgressBar::new(0));
        bar.set_style(row_style());
        bar
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
    // Rows look like `<GUTTER><label or length><GAP><value>`. To make the
    // table column and the summary values share a right edge we grow the
    // count column so `value_w >= summary.len() + label.len() - LEN_COL_WIDTH`.
    let formatted: Vec<String> = entries
        .iter()
        .map(|(_, c)| format_count(*c, human))
        .collect();

    let summary_pad = |label: &str, value: &str| {
        value
            .len()
            .saturating_add(label.len())
            .saturating_sub(LEN_COL_WIDTH)
    };
    let mut value_w = formatted
        .iter()
        .map(String::len)
        .max()
        .unwrap_or(0)
        .max(COUNT_HEADER.len());
    if let Some(s) = total_str {
        value_w = value_w.max(summary_pad(TOTAL_LABEL, s));
    }
    value_w = value_w.max(summary_pad(POINTS_LABEL, points_str));

    let separator_width = GUTTER + LEN_COL_WIDTH + GAP + value_w;
    let summary_value_width = |label: &str| separator_width - (GUTTER + label.len() + GAP);

    let mut table = Vec::new();
    if !entries.is_empty() {
        table.reserve_exact(entries.len() + 1);
        table.push(format!("  Len  {COUNT_HEADER:>value_w$}"));
        for ((length, _), value) in entries.iter().zip(formatted.iter()) {
            table.push(format!("  {length:>LEN_COL_WIDTH$}  {value:>value_w$}"));
        }
    }

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
