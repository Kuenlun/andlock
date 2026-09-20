// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

#![cfg_attr(
    test,
    expect(
        unused_crate_dependencies,
        reason = "Cargo supplies dev-dependencies used by integration tests and benchmarks, not this binary's unit tests."
    )
)]

//! Count valid unlock patterns and render completed results and diagnostics.

use std::process::ExitCode;

mod cli;
mod memory;
mod output;
mod pipeline;
mod preview;
mod tty;

#[expect(
    clippy::print_stderr,
    reason = "CLI startup failures are reported on stderr."
)]
fn main() -> ExitCode {
    let result = tty::install_handler().and_then(|()| cli::run());
    if tty::is_cancelled() {
        std::process::exit(tty::SIGINT_EXIT_CODE);
    }
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{} {err:#}", console::style("error:").red().bold());
            ExitCode::FAILURE
        }
    }
}
