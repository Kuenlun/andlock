/*!
andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
Copyright (C) 2026  Juan Luis Leal Contreras (Kuenlun)

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use std::io::{self, Write};
use std::sync::OnceLock;

use indicatif::MultiProgress;

// Conventional 128 + SIGINT on Unix; on Windows there is no such convention,
// so we pick a non-zero code that still lets shell scripts detect cancellation
// (`if ! andlock …; then handle_cancel; fi`) without surfacing a red error in
// `cargo run` — Cargo only flags the magic STATUS_CONTROL_C_EXIT (0xC000013A).
#[cfg(unix)]
const SIGINT_EXIT_CODE: i32 = 130;
#[cfg(not(unix))]
const SIGINT_EXIT_CODE: i32 = 1;

/// Shared draw target so the Ctrl+C handler can clear all bars at once.
pub fn progress() -> &'static MultiProgress {
    static PROGRESS: OnceLock<MultiProgress> = OnceLock::new();
    PROGRESS.get_or_init(MultiProgress::new)
}

/// Installs the process-wide Ctrl+C handler that clears every active
/// progress bar and restores the terminal cursor before exiting with
/// `SIGINT_EXIT_CODE`. Must be called once at startup, before any bar
/// is created.
///
/// # Errors
/// Returns the underlying `ctrlc` error if a handler is already
/// registered for this process.
pub fn install_handler() -> anyhow::Result<()> {
    // Debug-only escape hatch so a subprocess test can exercise `main`'s `?`
    // Err-propagation path. Compiled out of release builds entirely.
    #[cfg(debug_assertions)]
    if std::env::var_os("ANDLOCK_FORCE_HANDLER_ERROR").is_some() {
        anyhow::bail!("simulated handler error (ANDLOCK_FORCE_HANDLER_ERROR set)");
    }
    ctrlc::set_handler(|| {
        // Clear every active bar and restore the cursor that `enable_steady_tick`
        // hid: `process::exit` skips destructors, so without this the shell
        // prompt lands on top of the last frame with an invisible caret.
        let _ = progress().clear();
        let _ = console::Term::stderr().show_cursor();
        let _ = io::stderr().flush();
        std::process::exit(SIGINT_EXIT_CODE);
    })?;
    Ok(())
}
