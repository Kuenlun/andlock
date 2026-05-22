// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use std::io::{self, Write};
use std::sync::OnceLock;

use indicatif::MultiProgress;

// 128 + SIGINT on Unix; on Windows any non-zero code that does not collide
// with Cargo's STATUS_CONTROL_C_EXIT (0xC000013A) banner.
#[cfg(unix)]
const SIGINT_EXIT_CODE: i32 = 130;
#[cfg(not(unix))]
const SIGINT_EXIT_CODE: i32 = 1;

/// Shared draw target so the Ctrl+C handler can clear every bar at once.
pub fn progress() -> &'static MultiProgress {
    static PROGRESS: OnceLock<MultiProgress> = OnceLock::new();
    PROGRESS.get_or_init(MultiProgress::new)
}

/// Installs the process-wide Ctrl+C handler.
///
/// # Errors
/// Surfaces the `ctrlc` error when a handler is already registered.
pub fn install_handler() -> anyhow::Result<()> {
    ctrlc::set_handler(|| {
        let _ = progress().clear();
        let _ = console::Term::stderr().show_cursor();
        let _ = io::stderr().flush();
        std::process::exit(SIGINT_EXIT_CODE);
    })?;
    Ok(())
}
