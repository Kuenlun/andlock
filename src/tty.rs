// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use console::Term;
use indicatif::MultiProgress;

// 128 + SIGINT on Unix; on Windows any non-zero code that does not collide
// with Cargo's STATUS_CONTROL_C_EXIT (0xC000013A) banner.
#[cfg(unix)]
pub const SIGINT_EXIT_CODE: i32 = 130;
#[cfg(not(unix))]
pub const SIGINT_EXIT_CODE: i32 = 1;

static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Shared draw target so the Ctrl+C handler can clear every bar at once.
pub fn progress() -> &'static MultiProgress {
    static PROGRESS: OnceLock<MultiProgress> = OnceLock::new();
    PROGRESS.get_or_init(MultiProgress::new)
}

/// Whether SIGINT has been received at least once.
pub fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

/// Installs the process-wide Ctrl+C handler. First press flags cooperative
/// cancellation so the DP can surface partial results, a second press forces
/// an immediate exit.
///
/// When stderr is a TTY the first press also clears the current row so the
/// kernel's `^C` echo, which lands wherever the cursor sits, does not leave a
/// half overwritten progress bar behind: the next tick repaints it intact.
///
/// # Errors
/// Surfaces the `ctrlc` error when a handler is already registered.
pub fn install_handler() -> anyhow::Result<()> {
    ctrlc::set_handler(|| {
        if CANCELLED.swap(true, Ordering::SeqCst) {
            let _ = progress().clear();
            let _ = Term::stderr().show_cursor();
            std::process::exit(SIGINT_EXIT_CODE);
        }
        let term = Term::stderr();
        if term.is_term() {
            let _ = term.clear_line();
        }
    })?;
    Ok(())
}
