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

/// Builds a [`ProgressStyle`] from a template string, falling back to
/// `fallback()` when the template fails to parse. Centralising the
/// fallback lets the call sites stay declarative — they describe the
/// template they want and a sensible default to drop to — and lets the
/// fallback branch be exercised directly with an invalid template
/// rather than relying on the impossible "valid template returns Err"
/// path. Both arms are part of the public contract: a future template
/// change must not silently lose styling.
pub fn style_or_default(template: &str, fallback: fn() -> ProgressStyle) -> ProgressStyle {
    ProgressStyle::with_template(template).unwrap_or_else(|_| fallback())
}

/// Bar style used for each per-length row of the live count table: just
/// the message, no bar/spinner/percentage. Each row is its own
/// `ProgressBar` so we can rewrite them in place when a wider count
/// arrives and forces a column re-alignment.
fn row_style() -> ProgressStyle {
    style_or_default("{msg}", ProgressStyle::default_bar)
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

    /// Records a `(length, count)` row, filtered by `min/max_length`
    /// and `count != 0`. In live mode the new row is appended above the
    /// anchor and every prior row is repainted so the count column stays
    /// right-aligned to the widest formatted value.
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
    use indicatif::ProgressDrawTarget;

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

    /// Test scaffolding: a `TermLike` that swallows every write but is
    /// not the `Hidden` variant, so `ProgressBar::is_hidden()` returns
    /// `false` and `LengthPrinter` enters its live-rendering branch.
    /// The shared `lines` buffer lets the assertions below prove the
    /// printer actually emitted progress traffic; `Clone` makes the
    /// shared handle cheap to box for `term_like_with_hz`.
    #[derive(Debug, Default, Clone)]
    struct CapturingTerm {
        lines: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl CapturingTerm {
        fn write_count(&self) -> usize {
            self.lines.lock().unwrap().len()
        }
    }

    impl indicatif::TermLike for CapturingTerm {
        fn width(&self) -> u16 {
            80
        }
        fn move_cursor_up(&self, _n: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn move_cursor_down(&self, _n: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn move_cursor_right(&self, _n: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn move_cursor_left(&self, _n: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn write_line(&self, s: &str) -> std::io::Result<()> {
            self.lines.lock().unwrap().push(s.to_owned());
            Ok(())
        }
        fn write_str(&self, s: &str) -> std::io::Result<()> {
            self.lines.lock().unwrap().push(s.to_owned());
            Ok(())
        }
        fn clear_line(&self) -> std::io::Result<()> {
            Ok(())
        }
        fn flush(&self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn live_multiprogress() -> (MultiProgress, CapturingTerm) {
        let term = CapturingTerm::default();
        let target = ProgressDrawTarget::term_like_with_hz(Box::new(term.clone()), 20);
        (MultiProgress::with_draw_target(target), term)
    }

    /// In live mode the printer must collect each matching length, lay
    /// out the rows above the anchor bar, and hand the entries back at
    /// `finish` time so the caller can paint the unified table block.
    /// We drive two prints to exercise both the first-row path that
    /// installs the header bar and the subsequent path that only
    /// realigns the existing rows.
    #[test]
    fn length_printer_streams_live_rows_and_returns_entries() {
        let (mp, term) = live_multiprogress();
        let anchor = mp.add(ProgressBar::new(16));
        assert!(
            !anchor.is_hidden(),
            "test draw target must not be the hidden variant",
        );

        let mut printer = LengthPrinter::new(&mp, 0, 9, false, Some(&anchor));

        // count == 0 and out-of-range lengths are filtered before the
        // entries vec is touched, regardless of live state.
        printer.print(0, 0);
        printer.print(10, 1);

        printer.print(0, 1);
        printer.print(9, 140_704);
        let writes_during = term.write_count();

        let entries = printer.finish();
        assert_eq!(entries, vec![(0, 1), (9, 140_704)]);
        assert!(
            writes_during > 0,
            "live mode should have emitted progress traffic before finish",
        );

        anchor.finish_and_clear();
    }

    /// `--human` widens the longest count, and every prior row must be
    /// repainted so the table stays right-aligned. We assert on the
    /// rendered lines directly via `render_lines` because the live bars
    /// only carry the formatted strings — that helper is the source of
    /// truth for what each row prints.
    #[test]
    fn length_printer_realigns_rows_when_a_wider_count_arrives() {
        let (mp, _term) = live_multiprogress();
        let anchor = mp.add(ProgressBar::new(16));

        let mut printer = LengthPrinter::new(&mp, 0, 9, true, Some(&anchor));
        printer.print(0, 1);
        let narrow = printer.render_lines();
        printer.print(9, 140_704);
        let widened = printer.render_lines();

        assert_eq!(narrow.len(), 2, "header + one row");
        assert_eq!(widened.len(), 3, "header + two rows");
        assert!(
            widened.iter().all(|line| line.ends_with("140_704")
                || line.ends_with("Count")
                || line.ends_with("      1")),
            "all rows must right-align to the widest formatted value: {widened:?}",
        );

        let _ = printer.finish();
        anchor.finish_and_clear();
    }

    /// `realign_live` is also reachable as a free-standing helper — the
    /// `&self` signature lets a future caller invoke it outside of a
    /// `print` call. The defensive `let-else` early-return guards against
    /// that case: when no live state is attached, the function returns
    /// without producing output or touching any bar.
    #[test]
    fn realign_live_is_a_noop_when_no_live_state_is_attached() {
        let mp = MultiProgress::with_draw_target(ProgressDrawTarget::hidden());
        let printer = LengthPrinter::new(&mp, 0, 4, false, None);
        // Direct call: no state, nothing to do, must not panic or write.
        printer.realign_live();
    }

    /// `realign_live` is well-defined when live state exists but no row
    /// has matched yet — `header_bar` is `None`, so the inner `if let
    /// (Some, Some)` does not match and the function falls through to
    /// the row-bars loop (also empty). Production callers serialise
    /// `print` and never reach this state, but the helper must not
    /// crash on it; the symmetric test above covers the no-live path.
    #[test]
    fn realign_live_skips_header_update_when_no_row_has_matched_yet() {
        let (mp, _term) = live_multiprogress();
        let anchor = mp.add(ProgressBar::new(16));
        let printer = LengthPrinter::new(&mp, 0, 5, false, Some(&anchor));
        // Live state set, no print() called → header_bar stays None.
        printer.realign_live();
        anchor.finish_and_clear();
    }

    /// Live mode tracks bars lazily; if every row got filtered out or
    /// no row finalised, `finish` must still tear down without
    /// dereferencing a missing `header_bar`. The `if let Some(bar)`
    /// guard handles the contract; the test exercises the False arm
    /// alongside the row-bars loop, both of which iterate over empty
    /// containers.
    #[test]
    fn finish_with_live_state_but_no_prints_returns_no_entries() {
        let (mp, _term) = live_multiprogress();
        let anchor = mp.add(ProgressBar::new(16));
        let printer = LengthPrinter::new(&mp, 0, 5, false, Some(&anchor));
        let entries = printer.finish();
        assert!(entries.is_empty());
        anchor.finish_and_clear();
    }

    /// Non-live mode is the production-bin shape: the printer just
    /// buffers entries and `finish` returns them. This test fixes the
    /// False arm of the `if let Some(live)` patterns in both `print`
    /// and `finish` that the live-mode tests above never traverse.
    #[test]
    fn length_printer_buffers_entries_silently_with_no_anchor() {
        let mp = MultiProgress::with_draw_target(ProgressDrawTarget::hidden());
        let mut printer = LengthPrinter::new(&mp, 0, 9, false, None);
        printer.print(0, 1);
        printer.print(9, 140_704);
        let entries = printer.finish();
        assert_eq!(entries, vec![(0, 1), (9, 140_704)]);
    }

    /// `render_lines` returns an empty vector before any row matches so
    /// callers do not paint an orphan header. This matches the buffered
    /// (non-live) shape of the printer, which the integration tests
    /// already exercise end-to-end.
    #[test]
    fn render_lines_returns_empty_vector_before_any_row_matches() {
        let mp = MultiProgress::with_draw_target(ProgressDrawTarget::hidden());
        let printer = LengthPrinter::new(&mp, 0, 4, false, None);
        assert!(printer.render_lines().is_empty());
    }

    /// `render_final` is the single place where the table+summary
    /// layout lives, so its empty-entries shape must produce no table
    /// rows at all. The summary still carries `Points`, and `Total`
    /// only appears when the caller passes one — covering the two
    /// branches of the `if let Some(s) = total_str` block.
    #[test]
    fn render_final_with_empty_entries_emits_only_summary() {
        let report = render_final(&[], false, None, "0");
        assert!(
            report.table.is_empty(),
            "no entries should produce no table rows",
        );
        assert_eq!(report.summary.len(), 1, "only Points is rendered");
        assert!(
            report.summary[0].trim_start().starts_with("Points"),
            "summary line must label Points: {:?}",
            report.summary[0],
        );
        assert!(report.separator_width >= "  Points  0".len());
    }

    /// With a `Total` provided the summary carries both rows, both
    /// right-aligned to the same column edge. This is the path the
    /// pipeline takes whenever the run was not memory-clamped.
    #[test]
    fn render_final_with_total_emits_total_then_points() {
        let report = render_final(&[(0, 1), (1, 9)], false, Some("10"), "9");
        assert_eq!(report.summary.len(), 2);
        assert!(report.summary[0].trim_start().starts_with("Total"));
        assert!(report.summary[1].trim_start().starts_with("Points"));
        assert_eq!(report.table.len(), 3, "header + two entry rows");
    }

    /// `style_or_default` returns the parsed style on a valid template
    /// — the happy path for every production caller.
    #[test]
    fn style_or_default_returns_parsed_style_for_valid_template() {
        let _ = style_or_default("{msg}", ProgressStyle::default_bar);
    }

    /// And falls back to the supplied default when the template is
    /// rejected. Pinning this branch in a test keeps the resilience
    /// guarantee from silently rotting: a future indicatif version
    /// that tightens template validation must still degrade
    /// gracefully rather than panic the binary.
    #[test]
    fn style_or_default_falls_back_when_template_is_invalid() {
        // Each candidate is a template indicatif's parser is documented
        // to reject; the test asserts up-front so a future relaxation
        // is caught loudly here rather than silently uncovering the
        // fallback branch elsewhere.
        let invalid_candidates = ["{", "{}", "{foo:!}", "{foo:bar}", "{foo:>9.bogus}"];
        let mut hit_fallback = false;
        for template in invalid_candidates {
            if ProgressStyle::with_template(template).is_err() {
                let _ = style_or_default(template, ProgressStyle::default_bar);
                hit_fallback = true;
            }
        }
        assert!(
            hit_fallback,
            "no candidate template failed to parse — update the list",
        );
    }
}
