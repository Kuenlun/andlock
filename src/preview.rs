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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use andlock::grid::GridDefinition;

    fn grid(dimensions: usize, points: Vec<Vec<i32>>) -> GridDefinition {
        GridDefinition { dimensions, points }
    }

    // --- detect_free_dims ---

    #[test]
    fn detect_free_dims_returns_zero_for_plain_base_grid() {
        // No trailing dimension has the "exactly one 1" signature.
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![0, 1], vec![1, 1]]);
        assert_eq!(detect_free_dims(&g), 0);
    }

    #[test]
    fn detect_free_dims_counts_contiguous_trailing_unit_axes() {
        // Dims 2 and 3 each carry exactly one `1`; dims 0–1 do not → count = 2.
        let g = grid(
            4,
            vec![
                vec![0, 0, 0, 0],
                vec![1, 0, 0, 0],
                vec![0, 0, 1, 0],
                vec![0, 0, 0, 1],
            ],
        );
        assert_eq!(detect_free_dims(&g), 2);
    }

    #[test]
    fn detect_free_dims_stops_at_first_non_matching_dim() {
        // Dim 2 (last) matches; dim 1 has two `1`s → scan stops → count = 1.
        #[rustfmt::skip]
        let g = grid(
            3,
            vec![
                vec![0, 0, 0],
                vec![1, 0, 0],
                vec![0, 1, 0],
                vec![0, 1, 1],
            ],
        );
        assert_eq!(detect_free_dims(&g), 1);
    }

    // --- render_preview: None cases ---

    #[test]
    fn render_preview_returns_none_for_3d_base_grid() {
        #[rustfmt::skip]
        let g = grid(
            3,
            vec![
                vec![0, 0, 0],
                vec![1, 0, 0],
                vec![0, 1, 0],
                vec![0, 0, 1],
            ],
        );
        assert!(render_preview(&g, Some(0)).is_none());
    }

    #[test]
    fn render_preview_returns_none_for_empty_base_without_free_points() {
        assert!(render_preview(&grid(2, vec![]), Some(0)).is_none());
    }

    #[test]
    fn render_preview_returns_none_when_column_count_exceeds_limit() {
        // 41 distinct x-values exceeds MAX_DISPLAY_COLS (40).
        let points: Vec<Vec<i32>> = (0..=40_i32).map(|x| vec![x, 0]).collect();
        assert!(render_preview(&grid(2, points), Some(0)).is_none());
    }

    #[test]
    fn render_preview_returns_none_when_row_count_exceeds_limit() {
        // 21 distinct y-values exceeds MAX_DISPLAY_ROWS (20).
        let points: Vec<Vec<i32>> = (0..=20_i32).map(|y| vec![0, y]).collect();
        assert!(render_preview(&grid(2, points), Some(0)).is_none());
    }

    // --- render_preview: 0D base (stars only) ---

    #[test]
    fn render_preview_0d_base_with_free_points_shows_only_stars() {
        // No base dimensions; 3 free points → "★ ★ ★".
        let g = grid(3, vec![vec![1, 0, 0], vec![0, 1, 0], vec![0, 0, 1]]);
        assert_eq!(render_preview(&g, Some(3)).unwrap(), "★ ★ ★");
    }

    // --- render_preview: 1D base ---

    #[test]
    fn render_preview_single_node_1d_grid() {
        assert_eq!(
            render_preview(&grid(1, vec![vec![0]]), Some(0)).unwrap(),
            "●"
        );
    }

    #[test]
    fn render_preview_1d_three_node_line() {
        let g = grid(1, vec![vec![-1], vec![0], vec![1]]);
        assert_eq!(render_preview(&g, Some(0)).unwrap(), "● ● ●");
    }

    // --- render_preview: 2D base ---

    #[test]
    fn render_preview_2d_2x2_fully_filled() {
        // ys descending → top row y=1, bottom row y=0.
        let g = grid(2, vec![vec![0, 0], vec![0, 1], vec![1, 0], vec![1, 1]]);
        assert_eq!(render_preview(&g, Some(0)).unwrap(), "● ●\n● ●");
    }

    #[test]
    fn render_preview_2d_3x3_fully_filled() {
        let g = grid(
            2,
            vec![
                vec![-1, -1],
                vec![-1, 0],
                vec![-1, 1],
                vec![0, -1],
                vec![0, 0],
                vec![0, 1],
                vec![1, -1],
                vec![1, 0],
                vec![1, 1],
            ],
        );
        assert_eq!(render_preview(&g, Some(0)).unwrap(), "● ● ●\n● ● ●\n● ● ●");
    }

    #[test]
    fn render_preview_2d_sparse_grid_renders_gaps_as_spaces() {
        // Corners + centre of a 3×3; the four edge midpoints are absent.
        // xs=[-1,0,1], ys=[1,0,-1]:
        //   y= 1 → ●   ●
        //   y= 0 →   ●
        //   y=-1 → ●   ●
        let g = grid(
            2,
            vec![
                vec![-1, -1],
                vec![-1, 1],
                vec![0, 0],
                vec![1, -1],
                vec![1, 1],
            ],
        );
        let result = render_preview(&g, Some(0)).unwrap();
        let rows: Vec<&str> = result.split('\n').collect();
        assert_eq!(rows.len(), 3);
        assert!(rows[0].starts_with('●') && rows[0].ends_with('●'));
        assert!(rows[1].contains('●') && rows[1].starts_with(' '));
        assert!(rows[2].starts_with('●') && rows[2].ends_with('●'));
    }

    // --- render_preview: free-point attachment ---

    #[test]
    fn render_preview_single_free_point_appended_to_1d_grid() {
        // 1D base (3 nodes) + 1 free point → star appended to the sole row.
        let g = grid(2, vec![vec![-1, 0], vec![0, 0], vec![1, 0], vec![0, 1]]);
        let result = render_preview(&g, Some(1)).unwrap();
        assert!(result.contains("● ● ●"));
        assert!(result.ends_with('★'));
        assert_eq!(result.chars().filter(|&c| c == '★').count(), 1);
    }

    #[test]
    fn render_preview_free_point_centered_in_tall_grid() {
        // 2D base with 3 rows + 1 free point: top_pad=(3-1)/2=1 → star on middle row.
        let g = grid(
            3,
            vec![vec![0, 0, 0], vec![0, 1, 0], vec![0, 2, 0], vec![0, 0, 1]],
        );
        let rows: Vec<String> = render_preview(&g, Some(1))
            .unwrap()
            .split('\n')
            .map(String::from)
            .collect();
        assert_eq!(rows.len(), 3);
        assert!(!rows[0].contains('★'), "top row should not have a star");
        assert!(rows[1].contains('★'), "middle row should have the star");
        assert!(!rows[2].contains('★'), "bottom row should not have a star");
    }

    #[test]
    fn render_preview_excess_free_points_wrap_across_columns() {
        // 1D base (2 nodes → 1 grid row) + 4 free points:
        // num_star_cols=4, all stars land on the single row → no newline.
        let g = grid(
            6,
            vec![
                vec![0, 0, 0, 0, 0, 0],
                vec![1, 0, 0, 0, 0, 0],
                vec![0, 0, 1, 0, 0, 0],
                vec![0, 0, 0, 1, 0, 0],
                vec![0, 0, 0, 0, 1, 0],
                vec![0, 0, 0, 0, 0, 1],
            ],
        );
        let result = render_preview(&g, Some(4)).unwrap();
        assert_eq!(result.chars().filter(|&c| c == '★').count(), 4);
        assert!(
            !result.contains('\n'),
            "all stars should collapse onto one row"
        );
    }

    // --- attach_free_points: partial last column ---

    #[test]
    fn attach_free_points_partial_last_column_breaks_early() {
        // grid_rows=3, n_free=4 → num_star_cols=2.
        // Column 1: star_idx for rows 0,1,2 = 3,4,5.
        // Row 0: 3 < 4 → star. Row 1: 4 >= 4 → break. Row 2: never reached.
        // Expected star counts: row0=2, row1=1, row2=1.
        let mut rows = vec![String::new(), String::new(), String::new()];
        attach_free_points(&mut rows, 4, 3);
        assert_eq!(rows[0].chars().filter(|&c| c == '★').count(), 2);
        assert_eq!(rows[1].chars().filter(|&c| c == '★').count(), 1);
        assert_eq!(rows[2].chars().filter(|&c| c == '★').count(), 1);
    }

    // --- auto-detect path (known_free_dims = None) ---

    #[test]
    fn render_preview_auto_detects_no_free_dims_for_plain_grid() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![0, 1], vec![1, 1]]);
        assert!(render_preview(&g, None).is_some());
    }

    #[test]
    fn render_preview_auto_detect_matches_explicit_for_canonical_grid() {
        // For a canonical generated grid, the auto-detect path and the
        // explicit-zero-free-dims path must produce identical output.
        let g = andlock::grid::build_grid_definition(&[3, 3], 0);
        assert_eq!(render_preview(&g, None), render_preview(&g, Some(0)));
    }

    #[test]
    fn render_preview_auto_detect_matches_explicit_for_grid_with_free_dims() {
        // Dims 2 and 3 each carry exactly one `1` → detect_free_dims returns 2.
        // Auto-detect must use the structural filter (v == 0) to separate the two
        // base points from the two free points, matching the positional split done
        // by the explicit Some(2) path.
        let g = grid(
            4,
            vec![
                vec![0, 0, 0, 0],
                vec![1, 0, 0, 0],
                vec![0, 0, 1, 0],
                vec![0, 0, 0, 1],
            ],
        );
        assert_eq!(render_preview(&g, None), render_preview(&g, Some(2)));
    }
}
