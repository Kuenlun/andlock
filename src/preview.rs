// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Terminal preview renderer for 0D / 1D / 2D base grids. Base nodes show as
//! `●`, free points as `★`. Silently returns `None` for grids that are too
//! large or too high-dimensional to display meaningfully.

use std::collections::HashSet;

use andlock::canonicalizer::canonicalize;
use andlock::grid::GridDefinition;

const MAX_DISPLAY_COLS: usize = 40;
const MAX_DISPLAY_ROWS: usize = 20;
const MARGIN: &str = "    ";

/// Counts trailing dimensions matching the free-point signature emitted by
/// `build_grid_definition`: exactly one coordinate = 1, every other = 0.
fn detect_free_dims(grid: &GridDefinition) -> usize {
    (0..grid.dimensions)
        .rev()
        .take_while(|&d| {
            let mut ones = 0usize;
            let mut non_zero = 0usize;
            for p in &grid.points {
                match p[d] {
                    0 => {}
                    1 => {
                        ones += 1;
                        non_zero += 1;
                    }
                    _ => non_zero += 1,
                }
            }
            ones == 1 && non_zero == 1
        })
        .count()
}

/// Build the preview string for `grid`, or `None` to skip silently. Pass
/// `known_free_dims = Some(n)` when the grid was freshly built by the `grid`
/// subcommand (already canonical); pass `None` for user-provided grids.
#[must_use]
pub fn render_preview(grid: &GridDefinition, known_free_dims: Option<usize>) -> Option<String> {
    let canonical_grid = known_free_dims.is_none().then(|| canonicalize(grid));
    let display_grid = canonical_grid.as_ref().unwrap_or(grid);

    let free_dims = known_free_dims.unwrap_or_else(|| detect_free_dims(display_grid));
    let base_dims = display_grid.dimensions.saturating_sub(free_dims);
    if base_dims > 2 {
        return None;
    }

    // Generated grids preserve the base-then-free order, so a positional split
    // works even after canonicalisation zeroes the free-coordinate row. User
    // grids need the all-zero-tail filter instead.
    let n_base = display_grid.points.len().saturating_sub(free_dims);
    let base_points: Vec<&Vec<i32>> = if known_free_dims.is_some() {
        display_grid.points[..n_base].iter().collect()
    } else {
        display_grid
            .points
            .iter()
            .filter(|p| p[base_dims..].iter().all(|&v| v == 0))
            .collect()
    };
    let n_free = display_grid.points.len() - base_points.len();

    if base_dims == 0 || base_points.is_empty() {
        return (n_free > 0).then(|| vec!["★"; n_free].join(" "));
    }

    let xs = unique_sorted(base_points.iter().map(|p| p[0]), false);
    let ys = if base_dims >= 2 {
        unique_sorted(base_points.iter().map(|p| p[1]), true)
    } else {
        vec![0_i32]
    };

    if xs.len() > MAX_DISPLAY_COLS || ys.len() > MAX_DISPLAY_ROWS {
        return None;
    }

    let point_set: HashSet<(i32, i32)> = if base_dims >= 2 {
        base_points.iter().map(|p| (p[0], p[1])).collect()
    } else {
        xs.iter().map(|&x| (x, 0)).collect()
    };

    let mut rows: Vec<String> = ys.iter().map(|&y| render_row(&xs, &point_set, y)).collect();
    if n_free > 0 {
        attach_free_points(&mut rows, n_free);
    }
    Some(rows.join("\n"))
}

fn unique_sorted(values: impl Iterator<Item = i32>, descending: bool) -> Vec<i32> {
    let mut v: Vec<i32> = values.collect();
    if descending {
        v.sort_unstable_by(|a, b| b.cmp(a));
    } else {
        v.sort_unstable();
    }
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
