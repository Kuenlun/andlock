// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Terminal preview renderer for 0D / 1D / 2D base grids. Base nodes show as
//! `●`, free points as `★`. Silently returns `None` for grids that are too
//! large or too high-dimensional to display meaningfully.
//!
//! For 2D grids the axis with more unique values is mapped to the horizontal,
//! so equivalent shapes like `2x4` and `4x2` render in the same wide layout.

use std::collections::HashSet;

use andlock::grid::GridDefinition;

const MAX_DISPLAY_COLS: usize = 40;
const MAX_DISPLAY_ROWS: usize = 20;
const MARGIN: &str = "    ";

/// Build the preview string for `grid`, or `None` to skip silently.
#[must_use]
pub fn render_preview(grid: &GridDefinition) -> Option<String> {
    if grid.dimensions > 2 {
        return None;
    }
    let base_points = grid.points.as_slice();
    let n_free = grid.free_points;

    if grid.dimensions == 0 || base_points.is_empty() {
        return (n_free > 0).then(|| vec!["★"; n_free].join(" "));
    }

    let xs0 = unique_sorted(base_points.iter().map(|p| p[0]));
    let (xs, ys_asc, point_set): (Vec<i32>, Vec<i32>, HashSet<(i32, i32)>) = if grid.dimensions >= 2
    {
        let xs1 = unique_sorted(base_points.iter().map(|p| p[1]));
        let (h, v, xs, ys) = if xs1.len() > xs0.len() {
            (1, 0, xs1, xs0)
        } else {
            (0, 1, xs0, xs1)
        };
        let set = base_points.iter().map(|p| (p[h], p[v])).collect();
        (xs, ys, set)
    } else {
        let set = xs0.iter().map(|&x| (x, 0)).collect();
        (xs0, vec![0], set)
    };

    if xs.len() > MAX_DISPLAY_COLS || ys_asc.len() > MAX_DISPLAY_ROWS {
        return None;
    }

    let mut rows: Vec<String> = ys_asc
        .iter()
        .rev()
        .map(|&y| render_row(&xs, &point_set, y))
        .collect();
    if n_free > 0 {
        attach_free_points(&mut rows, n_free);
    }
    Some(rows.join("\n"))
}

fn unique_sorted(values: impl Iterator<Item = i32>) -> Vec<i32> {
    let mut v: Vec<i32> = values.collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn render_row(xs: &[i32], point_set: &HashSet<(i32, i32)>, y: i32) -> String {
    let mut row = String::with_capacity(xs.len() * 2);
    for (i, &x) in xs.iter().enumerate() {
        if i > 0 {
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

/// Append a `★` block to the right of the grid rows.
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
