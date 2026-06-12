// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use std::io::{self, Write};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use indicatif::MultiProgress;

// 128 + SIGINT on Unix. Any non-zero code on Windows that does not collide
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
/// cancellation so the DP can surface partial results. A second press forces
/// an immediate exit.
///
/// # Errors
/// Surfaces the `ctrlc` error when a handler is already registered.
pub fn install_handler() -> anyhow::Result<()> {
    ctrlc::set_handler(|| {
        if CANCELLED.swap(true, Ordering::SeqCst) {
            let _ = progress().clear();
            let _ = console::Term::stderr().show_cursor();
            let _ = io::stderr().flush();
            std::process::exit(SIGINT_EXIT_CODE);
        }
        compensate_ctrl_c_echo();
    })?;
    Ok(())
}

/// Realigns the cursor after the kernel's `^C` echo on the first SIGINT so
/// indicatif's next clear lines up with the live bar. The echo lands at the
/// right edge of the bar's last row (indicatif pads to terminal width) and
/// auto-wraps onto a fresh row, leaving indicatif one row off. `ESC [ A`
/// undoes the wrap.
///
/// Skipped when stderr is not a terminal. The kernel only echoes `^C` to a
/// TTY, and writing a bare escape into a redirected file would just pollute
/// the captured output.
fn compensate_ctrl_c_echo() {
    if !console::Term::stderr().is_term() {
        return;
    }
    let mut err = io::stderr().lock();
    let _ = err.write_all(b"\x1b[A");
    let _ = err.flush();
}
