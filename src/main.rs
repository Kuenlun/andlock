// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

mod cli;
mod memory;
mod output;
mod pipeline;
mod preview;
mod tty;

fn main() -> anyhow::Result<()> {
    tty::install_handler()?;
    cli::run()
}
