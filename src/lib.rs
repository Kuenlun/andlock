// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Counting core: grid, block matrix, bitmask DP, and canonicalisation.

pub mod canonicalizer;
pub mod counter;
pub mod grid;
pub mod mask;
