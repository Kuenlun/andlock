// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! CLI previews preserve zero-dimensional grid counts.

#![expect(
    clippy::indexing_slicing,
    reason = "Test fixtures have fixed shapes; missing expected counts or JSON fields must fail the test."
)]

// Cargo shares package dependencies across its library, binary and test targets.
use {
    andlock as _, clap as _, clap_cargo as _, clap_complete as _, console as _, criterion as _,
    ctrlc as _, indicatif as _, num_bigint as _, parse_size as _, portable_pty as _, serde as _,
    sysinfo as _, vt100 as _,
};

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use serde_json::{Value, json};

#[test]
fn zero_dimensional_preview_preserves_counts() -> Result<()> {
    for free_points in [0_i32, 2_i32] {
        let grid = json!({"dimensions": 0_i32, "points": [[]], "free_points": free_points});
        let mut expected = None;
        for quiet in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_andlock"));
            let _command = command.args(["--file", "-", "--json"]);
            if quiet {
                let _command = command.arg("--quiet");
            }
            let mut child = command
                .env("NO_COLOR", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            child
                .stdin
                .take()
                .context("child stdin")?
                .write_all(grid.to_string().as_bytes())?;
            let output = child.wait_with_output()?;
            assert!(output.status.success(), "{:?}", output.stderr);
            let report: Value = serde_json::from_slice(&output.stdout)?;
            assert_eq!(report["status"], "complete");
            assert_eq!(
                report["total"],
                if free_points == 0_i32 { "2" } else { "16" }
            );
            if let Some(reference) = &expected {
                assert_eq!(&output.stdout, reference);
            }
            expected = Some(output.stdout);
            let diagnostics = std::str::from_utf8(&output.stderr)?;
            if quiet {
                assert!(diagnostics.is_empty());
            } else {
                assert!(diagnostics.contains('●'), "{diagnostics}");
            }
        }
    }
    Ok(())
}
