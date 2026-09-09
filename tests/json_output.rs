// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Structured output preserves exact counts and describes partial completion.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result};
use serde_json::{Value, json};

fn run(args: &[&str]) -> Result<(Output, Value)> {
    let output = Command::new(env!("CARGO_BIN_EXE_andlock"))
        .args(args)
        .arg("--json")
        .env("NO_COLOR", "1")
        .output()?;
    let report = serde_json::from_slice(&output.stdout)?;
    Ok((output, report))
}

#[test]
fn complete_report_is_one_json_object_with_exact_input_grid() -> Result<()> {
    let grid = json!({"dimensions": 2, "points": [[2, -3], [6, -3], [6, 5]], "free_points": 1});
    let mut child = Command::new(env!("CARGO_BIN_EXE_andlock"))
        .args([
            "--file",
            "-",
            "--min-length",
            "1",
            "--max-length",
            "2",
            "--json",
            "-q",
        ])
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
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(
        report,
        json!({
            "grid": grid,
            "requested_range": {"min_length": 1, "max_length": 2},
            "completed_range": {"min_length": 1, "max_length": 2},
            "counts": [{"length": 1, "count": "4"}, {"length": 2, "count": "12"}],
            "total": "16",
            "status": "complete"
        })
    );
    Ok(())
}

#[test]
fn quiet_changes_only_diagnostics_for_successful_json_counts() -> Result<()> {
    let (normal, normal_report) = run(&["3x3", "--min-length", "4"])?;
    let (quiet, quiet_report) = run(&["3x3", "--min-length", "4", "-q"])?;
    assert!(normal.status.success());
    assert!(quiet.status.success());
    assert!(!normal.stderr.is_empty());
    assert!(quiet.stderr.is_empty());
    assert_eq!(normal_report, quiet_report);
    assert_eq!(quiet_report["total"], "389112");
    assert_eq!(quiet_report["status"], "complete");
    Ok(())
}

#[test]
fn memory_limited_reports_distinguish_partial_and_unstarted_ranges() -> Result<()> {
    for minimum in ["0", "2"] {
        let (output, report) = run(&["3x3", "--min-length", minimum, "--memory-limit", "0", "-q"])?;
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(report["status"], "memory_limit");
        assert_eq!(
            report["requested_range"]["min_length"],
            minimum.parse::<usize>()?
        );
        assert_eq!(report["requested_range"]["max_length"], 9);
        if minimum == "0" {
            assert_eq!(
                report["completed_range"],
                json!({"min_length": 0, "max_length": 1})
            );
            assert_eq!(
                report["counts"],
                json!([{"length": 0, "count": "1"}, {"length": 1, "count": "9"}])
            );
            assert_eq!(report["total"], "10");
        } else {
            assert!(report["completed_range"].is_null());
            assert_eq!(report["counts"], json!([]));
            assert!(report["total"].is_null());
        }
        assert!(std::str::from_utf8(&output.stderr)?.contains("insufficient memory"));
    }
    Ok(())
}

#[test]
fn count_overflow_excludes_the_inexact_length() -> Result<()> {
    for minimum in ["0", "31"] {
        let (output, report) = run(&["--free-points", "35", "--min-length", minimum, "-q"])?;
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(report["status"], "count_overflow");
        let counts = report["counts"].as_array().context("counts array")?;
        if minimum == "0" {
            assert_eq!(counts.len(), 31);
            let last: u128 = (6..=35).product();
            assert_eq!(counts[30], json!({"length": 30, "count": last.to_string()}));
            assert_eq!(
                report["completed_range"],
                json!({"min_length": 0, "max_length": 30})
            );
            assert!(report["total"].is_string());
        } else {
            assert!(counts.is_empty());
            assert!(report["completed_range"].is_null());
            assert!(report["total"].is_null());
        }
        assert!(std::str::from_utf8(&output.stderr)?.contains("do not fit in u128"));
    }
    Ok(())
}

#[test]
fn total_overflow_preserves_full_precision_rows_and_completed_range() -> Result<()> {
    let (output, report) = run(&["--free-points", "34", "--min-length", "33", "-q"])?;
    let factorial: u128 = (1..=34).product();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["status"], "total_overflow");
    assert!(report["total"].is_null());
    assert_eq!(report["completed_range"], report["requested_range"]);
    assert_eq!(
        report["counts"],
        json!([
            {"length": 33, "count": factorial.to_string()},
            {"length": 34, "count": factorial.to_string()}
        ])
    );

    let (selected, selected_report) = run(&["--free-points", "34", "--min-length", "34", "-q"])?;
    assert!(selected.status.success());
    assert_eq!(selected_report["status"], "complete");
    assert_eq!(selected_report["total"], factorial.to_string());
    Ok(())
}

#[test]
fn empty_grid_contains_a_completed_empty_pattern() -> Result<()> {
    let (output, report) = run(&["--free-points", "0", "--memory-limit", "0", "-q"])?;
    assert!(output.status.success());
    assert_eq!(
        report,
        json!({
            "grid": {"dimensions": 0, "points": [], "free_points": 0},
            "requested_range": {"min_length": 0, "max_length": 0},
            "completed_range": {"min_length": 0, "max_length": 0},
            "counts": [{"length": 0, "count": "1"}],
            "total": "1",
            "status": "complete"
        })
    );
    Ok(())
}

#[test]
fn json_rejects_incompatible_output_modes() -> Result<()> {
    for option in ["--export-json", "--human"] {
        let output = Command::new(env!("CARGO_BIN_EXE_andlock"))
            .args(["3x3", "--json", option])
            .output()?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(std::str::from_utf8(&output.stderr)?.contains("cannot be used with"));
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn interrupted_json_keeps_finalized_counts_and_sigint_exit() -> Result<()> {
    use std::io::{BufRead, BufReader, Read};

    for minimum in ["0", "5"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_andlock"))
            .args([
                "4x4",
                "-f",
                "111",
                "--memory-limit",
                "32MiB",
                "--min-length",
                minimum,
                "--json",
                "-q",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stderr = BufReader::new(child.stderr.take().context("child stderr")?);
        let mut diagnostics = String::new();
        // This warning is emitted after handler installation and before the DP starts.
        while !diagnostics.contains("insufficient memory") {
            assert!(stderr.read_line(&mut diagnostics)? > 0, "{diagnostics}");
        }
        let signal = Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()?;
        assert!(signal.success());
        let output = child.wait_with_output()?;
        stderr.read_to_string(&mut diagnostics)?;
        assert_eq!(output.status.code(), Some(130), "{diagnostics}");
        let report: Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(report["status"], "interrupted");
        let counts = report["counts"].as_array().context("counts array")?;
        if minimum == "0" {
            assert!(!counts.is_empty());
            let mut sum = 0u128;
            for (length, entry) in counts.iter().enumerate() {
                assert_eq!(entry["length"], length);
                sum += entry["count"]
                    .as_str()
                    .context("decimal count")?
                    .parse::<u128>()?;
            }
            assert_eq!(report["completed_range"]["max_length"], counts.len() - 1);
            assert_eq!(report["total"], sum.to_string());
        } else {
            assert!(counts.is_empty());
            assert!(report["completed_range"].is_null());
            assert!(report["total"].is_null());
        }
        assert!(diagnostics.contains("Interrupted"));
        assert!(!diagnostics.contains("error:"));
    }
    Ok(())
}
