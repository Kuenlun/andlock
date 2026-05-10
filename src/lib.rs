// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Core library for the `andlock` binary. Exposes the algorithmic pieces —
//! grid construction, block-matrix derivation, the dynamic-programming
//! counter and the geometric simplification passes — as independent
//! modules so they can be reused and tested outside the CLI wrapper.

// Allow `#[coverage(off)]` on test modules under `--cfg coverage_nightly` (nightly-only).
#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]

pub mod canonicalizer;
pub mod counter;
pub mod grid;
pub mod mask;
