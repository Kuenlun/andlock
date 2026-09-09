// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Counting core for Android-style unlock patterns on n-dimensional grids.
//!
//! A pattern is an ordered sequence of distinct nodes; a move is legal once
//! every node lying strictly on its open segment has already been visited.
//! [`grid`] models point sets and builds the per-move blocking masks,
//! [`counter`] counts patterns of every length with a layered bitmask DP
//! ([`mask`] picks the narrowest integer width for the visited set), and
//! [`canonicalizer`] normalises equivalent grids to a shared fixed point.
//!
//! Count the patterns the Android lock screen accepts (4+ points on 3x3):
//!
//! ```
//! use std::ops::ControlFlow;
//!
//! use andlock::counter::{DpEvent, DpScratch, count_patterns_dp};
//! use andlock::grid::{build_grid_definition, compute_blocks};
//!
//! let grid = build_grid_definition(&[3, 3], 0)?;
//! let n = grid.node_count();
//! let blocks: Vec<u32> = compute_blocks(&grid); // u32 fits n <= 31 nodes
//!
//! let mut scratch = DpScratch::allocate::<u32>(n, &blocks, n)?;
//! let mut android = 0u128;
//! count_patterns_dp(&mut scratch, n, &blocks, n, |event| {
//!     if let DpEvent::LengthDone { length, count } = event
//!         && length >= 4
//!     {
//!         android += count;
//!     }
//!     ControlFlow::Continue(())
//! });
//! assert_eq!(android, 389_112);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod canonicalizer;
pub mod counter;
pub mod grid;
pub mod mask;
pub mod search;
mod symmetry;
