// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use std::process::ExitCode;

mod cli;
mod memory;
mod output;
mod pipeline;
mod preview;
mod tty;

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
