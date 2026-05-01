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
    // Debug-only escape hatches so subprocess tests can exercise the
    // failure paths of `main`. Compiled out of release builds entirely.
    #[cfg(debug_assertions)]
    if std::env::var_os("ANDLOCK_FORCE_HANDLER_ERROR").is_some() {
        // Pre-register the real handler so the second `set_handler`
        // below fails with `MultipleHandlers` and propagates through
        // the `?`, exercising the actionable ctrlc-error path. The
        // unit test in this module already covers the same arm at
        // the library level; this hatch is defence-in-depth, pinning
        // that the production binary's `main` propagates the error
        // through `?` exactly as the unit test asserts the lower
        // level does.
        let _ = ctrlc::set_handler(handle_sigint);
    }
    #[cfg(debug_assertions)]
    if std::env::var_os("ANDLOCK_FORCE_SIGINT_HANDLER").is_some() {
        // Run the cleanup body the registered handler would normally
        // execute, then surface a normal error so `main` returns
        // through `lang_start`. That path triggers the C-runtime
        // atexit hooks that the LLVM coverage runtime needs to flush
        // profile data on Windows; calling `handle_sigint` directly
        // would short-circuit through `ExitProcess` and void the run.
        cleanup_for_sigint();
        anyhow::bail!("simulated sigint cleanup (ANDLOCK_FORCE_SIGINT_HANDLER set)");
    }
    ctrlc::set_handler(handle_sigint)?;
    Ok(())
}

/// Restores the terminal that the progress bars hijacked, then terminates
/// the process. `process::exit` skips destructors, so without the explicit
/// cleanup the shell prompt would land on top of a half-drawn frame with
/// an invisible caret.
///
/// Excluded from coverage instrumentation: this body is genuinely
/// unreachable from any portable test driver. Triggering SIGINT in a
/// subprocess requires `GenerateConsoleCtrlEvent` on Windows or
/// `libc::kill` on Unix — both `extern "C"`, both forbidden under this
/// crate's `unsafe_code = "forbid"`. Even if such a call were possible,
/// `std::process::exit` on Windows resolves to `ExitProcess`, which
/// bypasses the C-runtime atexit hooks the LLVM profile-write runtime
/// uses to flush coverage data, so any subprocess invocation would
/// silently void its own profile. The reachable cleanup logic lives
/// entirely in [`cleanup_for_sigint`], which the
/// `ANDLOCK_FORCE_SIGINT_HANDLER` hatch above exercises through the
/// normal `main`-return path.
#[cfg_attr(coverage_nightly, coverage(off))]
fn handle_sigint() {
    cleanup_for_sigint();
    std::process::exit(SIGINT_EXIT_CODE);
}

/// Cleanup the registered handler runs before terminating: clears every
/// progress bar, restores the cursor `enable_steady_tick` hid, and flushes
/// stderr so the shell prompt is drawn on a clean line.
fn cleanup_for_sigint() {
    let _ = progress().clear();
    let _ = console::Term::stderr().show_cursor();
    let _ = io::stderr().flush();
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    /// `ctrlc` allows exactly one handler per process. Calling
    /// [`install_handler`] a second time must surface the underlying
    /// `MultipleHandlers` error instead of silently overwriting; this
    /// exercises both the success arm of `set_handler` and the `?`
    /// propagation that surfaces a duplicate-registration error.
    ///
    /// This test must be the only one in the crate that invokes
    /// `install_handler`; otherwise the global handler state would
    /// leak across tests run in the same binary.
    #[test]
    fn install_handler_rejects_duplicate_registration() {
        install_handler().unwrap();
        let err = install_handler().unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.to_ascii_lowercase().contains("handler"),
            "expected ctrlc multiple-handler error, got: {msg}",
        );
    }
}
