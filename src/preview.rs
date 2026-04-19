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

//! Terminal preview renderer for low-dimensional point patterns.
//!
//! Renders 0D, 1D, and 2D base grids using `●` and free points using `★`.
//! Silently returns `None` when the grid is too large or too high-dimensional
//! to display meaningfully.

use std::collections::HashSet;

use andlock::canonicalizer::canonicalize;
use andlock::grid::GridDefinition;

const MAX_DISPLAY_COLS: usize = 40;
const MAX_DISPLAY_ROWS: usize = 20;
const MARGIN: &str = "    ";

/// Scan trailing dimensions for the "exactly one value = 1, all others = 0"
/// signature that identifies free-point dimensions produced by `build_grid_definition`.
/// Stops at the first dim that does not match, so only a contiguous suffix is counted.
fn detect_free_dims(grid: &GridDefinition) -> usize {
    let mut count = 0;
    for d in (0..grid.dimensions).rev() {
        let (ones, non_zeros) = grid.points.iter().fold((0_usize, 0_usize), |(o, nz), p| {
            let v = p[d];
            if v == 1 {
                (o + 1, nz + 1)
            } else if v != 0 {
                (o, nz + 1)
            } else {
                (o, nz)
            }
        });
        if ones == 1 && non_zeros == 1 {
            count += 1;
        } else {
            break;
        }
    }
    count
}

/// Build the terminal preview string for `grid`, or `None` to skip silently.
///
/// `known_free_dims` must be set to the number of free-point dimensions when
/// the grid was produced by the `grid` subcommand (already canonical).  Pass
/// `None` for user-provided grids: a canonical copy is made internally for
/// display purposes without touching the original.
#[must_use]
pub fn render_preview(grid: &GridDefinition, known_free_dims: Option<usize>) -> Option<String> {
    let canonical_grid = known_free_dims.is_none().then(|| canonicalize(grid));
    let display_grid = canonical_grid.as_ref().unwrap_or(grid);

    let free_dims = known_free_dims.unwrap_or_else(|| detect_free_dims(display_grid));
    let base_dims = display_grid.dimensions.saturating_sub(free_dims);

    // Abort silently for 3D+ base grids
    if base_dims > 2 {
        return None;
    }

    // Partition base vs free points.
    // For generated grids (known_free_dims is Some): build_grid_definition always places
    // base points first and free points last, and canonicalize preserves that order.
    // We split positionally so that free points translated to all-zeros by canonicalize
    // (which happens when there are zero base points) are still counted as free.
    // For user-provided grids: identify base points structurally by all-zero free-dim slots.
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

    // 0D base (or degenerate empty base): render only the free-point stars
    if base_dims == 0 || base_points.is_empty() {
        if n_free == 0 {
            return None;
        }
        return Some(vec!["★"; n_free].join(" "));
    }

    // Unique x-coordinates (dim 0), ascending
    let xs: Vec<i32> = {
        let mut v: Vec<i32> = base_points.iter().map(|p| p[0]).collect();
        v.sort_unstable();
        v.dedup();
        v
    };

    // Unique y-coordinates (dim 1 for 2D; single dummy row for 1D), descending
    let ys: Vec<i32> = if base_dims >= 2 {
        let mut v: Vec<i32> = base_points.iter().map(|p| p[1]).collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        v.dedup();
        v
    } else {
        vec![0_i32]
    };

    // Bounding-box guard (in display-cell units)
    if xs.len() > MAX_DISPLAY_COLS || ys.len() > MAX_DISPLAY_ROWS {
        return None;
    }

    // Lookup set: (x, y) → base point present
    let point_set: HashSet<(i32, i32)> = if base_dims >= 2 {
        base_points.iter().map(|p| (p[0], p[1])).collect()
    } else {
        xs.iter().map(|&x| (x, 0_i32)).collect()
    };

    let grid_rows = ys.len();
    let mut rows: Vec<String> = ys.iter().map(|&y| render_row(&xs, &point_set, y)).collect();

    if n_free > 0 {
        attach_free_points(&mut rows, n_free, grid_rows);
    }

    Some(rows.join("\n"))
}

/// Render one horizontal grid row, emitting `●` for occupied cells and ` `
/// for empty cells, separated by single spaces.
fn render_row(xs: &[i32], point_set: &HashSet<(i32, i32)>, y: i32) -> String {
    // capacity: one char per cell + one space separator between cells
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
/// - If `n_free ≤ grid_rows`: one star per row, centered vertically.
/// - If `n_free > grid_rows`: stars fill column by column (top to bottom),
///   wrapping into additional columns to the right.
fn attach_free_points(rows: &mut [String], n_free: usize, grid_rows: usize) {
    if n_free <= grid_rows {
        let top_pad = (grid_rows - n_free) / 2;
        for row in rows.iter_mut().skip(top_pad).take(n_free) {
            row.push_str(MARGIN);
            row.push('★');
        }
    } else {
        let num_star_cols = n_free.div_ceil(grid_rows);
        for (r, row) in rows.iter_mut().enumerate().take(grid_rows) {
            let mut first_star = true;
            for c in 0..num_star_cols {
                // Column-major indexing: column c holds stars c*grid_rows .. (c+1)*grid_rows
                let star_idx = c * grid_rows + r;
                if star_idx >= n_free {
                    break;
                }
                if first_star {
                    row.push_str(MARGIN);
                    first_star = false;
                } else {
                    row.push(' ');
                }
                row.push('★');
            }
        }
    }
}
