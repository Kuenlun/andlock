// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

// Allow `#[coverage(off)]` under `--cfg coverage_nightly` (nightly-only).
// Used on test modules and on the SIGINT handler in `tty`, whose body is
// genuinely unreachable from any portable test driver — see the
// justification in `tty::handle_sigint`.
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod cli;
mod json_format;
mod memory;
mod output;
mod pipeline;
mod preview;
mod tty;

fn main() -> anyhow::Result<()> {
    tty::install_handler()?;
    cli::run()
}
