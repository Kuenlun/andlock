// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Terminal preview renderer for 0D / 1D / 2D base grids. Base nodes show as
//! `●`, free points as `★`, empty lattice cells as blanks. Returns `None`
//! for grids too large or too high-dimensional to display meaningfully.
//!
//! The base grid is canonicalised first — counting is invariant under
//! translation and per-axis scaling, so the preview is too — and then drawn
//! at true lattice positions: nodes render collinear exactly when the
//! counter treats them as collinear. The wider axis maps to the horizontal,
//! so equivalent shapes like `2x4` and `4x2` render identically.

use std::collections::HashSet;

use andlock::canonicalizer::canonicalize;
use andlock::grid::GridDefinition;

const MAX_DISPLAY_ROWS: usize = 20;
/// Width budget when stderr is not a terminal.
const DEFAULT_MAX_WIDTH: usize = 120;
/// Stars per row in a free-points-only block.
const FREE_ROW_WIDTH: usize = 10;
/// Gap between the base grid and an attached free-point block.
const MARGIN: &str = "    ";

/// Renders `grid` sized to the current terminal, or `None` to skip silently.
#[must_use]
pub fn render_for_terminal(grid: &GridDefinition) -> Option<String> {
    let max_width = console::Term::stderr()
        .size_checked()
        .map(|(_, cols)| usize::from(cols))
        .filter(|&cols| cols > 0)
        .unwrap_or(DEFAULT_MAX_WIDTH);
    render_preview(grid, max_width)
}

/// Builds the preview string for `grid` within `max_width` display columns,
/// or `None` to skip silently.
#[must_use]
pub fn render_preview(grid: &GridDefinition, max_width: usize) -> Option<String> {
    let n_free = grid.free_points;
    if grid.points.is_empty() {
        return (n_free > 0)
            .then(|| render_free_block(n_free, max_width))
            .flatten();
    }
    if grid.dimensions > 2 {
        return None;
    }

    let (cols, rows, point_set) = project(&canonicalize(grid));
    if rows > MAX_DISPLAY_ROWS || total_width(cols, rows, n_free) > max_width {
        return None;
    }

    let mut lines: Vec<String> = (0..rows)
        .map(|r| render_row(cols, &point_set, rows - 1 - r))
        .collect();
    if n_free > 0 {
        attach_free_points(&mut lines, n_free);
    }
    Some(lines.join("\n"))
}

/// Display width in characters of the full preview: the base grid plus the
/// free-point block [`attach_free_points`] appends. Saturates, so oversized
/// grids fail the width budget instead of wrapping.
fn total_width(cols: usize, rows: usize, n_free: usize) -> usize {
    let cell_width = |cells: usize| cells.saturating_mul(2).saturating_sub(1);
    let grid = cell_width(cols);
    if n_free == 0 {
        return grid;
    }
    let star_cols = if n_free <= rows {
        1
    } else {
        n_free.div_ceil(rows)
    };
    grid.saturating_add(MARGIN.len())
        .saturating_add(cell_width(star_cols))
}

/// Maps canonical base points onto display cells: per-axis minimum at zero,
/// wider span on the horizontal axis, `y` growing upwards.
fn project(grid: &GridDefinition) -> (usize, usize, HashSet<(usize, usize)>) {
    let (xs, ys): (Vec<i64>, Vec<i64>) = grid
        .points
        .iter()
        .map(|p| {
            let y = if grid.dimensions >= 2 { p[1] } else { 0 };
            (i64::from(p[0]), i64::from(y))
        })
        .unzip();
    let span = |values: &[i64]| {
        let min = values.iter().copied().min().unwrap_or(0);
        let max = values.iter().copied().max().unwrap_or(0);
        // Differences of i32 coordinates: always exact, never negative.
        (min, usize::try_from(max - min).unwrap_or(usize::MAX))
    };
    let (x_min, x_span) = span(&xs);
    let (y_min, y_span) = span(&ys);
    let ((h_min, h_span, hs), (v_min, v_span, vs)) = if y_span > x_span {
        ((y_min, y_span, &ys), (x_min, x_span, &xs))
    } else {
        ((x_min, x_span, &xs), (y_min, y_span, &ys))
    };

    let cells = hs
        .iter()
        .zip(vs)
        .map(|(&h, &v)| {
            (
                usize::try_from(h - h_min).unwrap_or(usize::MAX),
                usize::try_from(v - v_min).unwrap_or(usize::MAX),
            )
        })
        .collect();
    (h_span.saturating_add(1), v_span.saturating_add(1), cells)
}

/// Lays out `n` free points as rows of up to [`FREE_ROW_WIDTH`] stars each.
fn render_free_block(n: usize, max_width: usize) -> Option<String> {
    let cols = n.min(FREE_ROW_WIDTH);
    let rows = n.div_ceil(FREE_ROW_WIDTH);
    if rows > MAX_DISPLAY_ROWS || cols * 2 - 1 > max_width {
        return None;
    }
    let mut out = String::with_capacity(rows * cols * 4);
    for r in 0..rows {
        if r > 0 {
            out.push('\n');
        }
        let in_row = (n - r * FREE_ROW_WIDTH).min(FREE_ROW_WIDTH);
        for c in 0..in_row {
            if c > 0 {
                out.push(' ');
            }
            out.push('★');
        }
    }
    Some(out)
}

fn render_row(cols: usize, point_set: &HashSet<(usize, usize)>, y: usize) -> String {
    let mut row = String::with_capacity(cols * 4);
    for x in 0..cols {
        if x > 0 {
            row.push(' ');
        }
        row.push(if point_set.contains(&(x, y)) {
            '●'
        } else {
            ' '
        });
    }
    row
}

/// Appends a `★` block to the right of the grid rows.
///
/// `n_free <= rows.len()`: one star per row, centred vertically. Otherwise
/// stars fill column by column (top-to-bottom), wrapping into additional
/// columns on the right.
fn attach_free_points(rows: &mut [String], n_free: usize) {
    let grid_rows = rows.len();
    if n_free <= grid_rows {
        let top_pad = (grid_rows - n_free) / 2;
        for row in rows.iter_mut().skip(top_pad).take(n_free) {
            row.push_str(MARGIN);
            row.push('★');
        }
        return;
    }
    let num_star_cols = n_free.div_ceil(grid_rows);
    for (r, row) in rows.iter_mut().enumerate() {
        for c in 0..num_star_cols {
            let star_idx = c * grid_rows + r;
            if star_idx >= n_free {
                break;
            }
            row.push_str(if c == 0 { MARGIN } else { " " });
            row.push('★');
        }
    }
}
