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

//! Count formatting and table rendering for the CLI's result output.
//! Hosts the live per-length printer streamed from the DP, and the
//! final unified table+summary block painted once the run finishes.

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Width of the `Len` column in the result table. The table layout math
/// in [`render_final`] derives every other width from this anchor.
const LEN_COL_WIDTH: usize = 3;

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
pub struct LengthPrinter<'a> {
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
    /// Builds a printer that streams matching length rows above `anchor`
    /// in `mp`, falling back to silent buffering when `anchor` is `None`
    /// or hidden (quiet runs, non-TTY).
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

    pub fn print(&mut self, length: usize, count: u128) {
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

/// Final unified report: the length table, the `Total`/`Points`
/// summary block, and the separator width that joins them visually.
pub struct RenderedReport {
    pub table: Vec<String>,
    pub summary: Vec<String>,
    pub separator_width: usize,
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
pub fn render_final(
    entries: &[(usize, u128)],
    human: bool,
    total_str: Option<&str>,
    points_str: &str,
) -> RenderedReport {
    // Each row is laid out as `<GUTTER><label/length><GAP><value>`, so a
    // table row is `GUTTER + LEN_COL_WIDTH + GAP + value_w` wide and a
    // summary row is `GUTTER + label.len() + GAP + value.len()`. The
    // count column is grown so both shapes share the same right edge:
    //     value_w >= value.len() + label.len() - LEN_COL_WIDTH
    const GUTTER: usize = 2;
    const GAP: usize = 2;
    const COUNT_HEADER: &str = "Count";
    const TOTAL_LABEL: &str = "Total";
    const POINTS_LABEL: &str = "Points";

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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

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
