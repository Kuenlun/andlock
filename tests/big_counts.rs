// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Arbitrary-precision output and agreement with the fixed-width counting mode.

use std::process::{Command, Output};

use anyhow::{Context, Result};
use num_bigint::BigUint;
use serde_json::Value;

fn run(args: &[&str], big: bool) -> Result<(Output, Value)> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_andlock"));
    command.args(args).args(["--json", "--quiet"]);
    if big {
        command.arg("--big-counts");
    }
    let output = command.output()?;
    let report = serde_json::from_slice(&output.stdout)?;
    Ok((output, report))
}

#[test]
fn free_point_counts_and_totals_remain_exact_beyond_u128() -> Result<()> {
    for n in [35usize, 127] {
        let (output, report) = run(
            &["--free-points", &n.to_string(), "--memory-limit", "0"],
            true,
        )?;
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(output.stderr.is_empty());
        assert_eq!(report["status"], "complete");
        assert_eq!(report["completed_range"], report["requested_range"]);
        let counts = report["counts"].as_array().context("count array")?;
        assert_eq!(counts.len(), n + 1);
        let factorial: BigUint = (1..=n).map(BigUint::from).product();
        let mut total = BigUint::default();
        for (length, entry) in counts.iter().enumerate() {
            let denominator: BigUint = (1..=(n - length)).map(BigUint::from).product();
            let expected = &factorial / denominator;
            assert_eq!(entry["length"], length);
            assert_eq!(entry["count"], expected.to_string());
            total += expected;
        }
        assert_eq!(report["total"], total.to_string());
        assert!(factorial.bits() > 128);
    }
    Ok(())
}

#[test]
fn default_mode_retains_explicit_count_and_total_overflow() -> Result<()> {
    let (overflow, report) = run(&["--free-points", "35"], false)?;
    assert_eq!(overflow.status.code(), Some(1));
    assert_eq!(report["status"], "count_overflow");

    let args = ["--free-points", "34", "--min-length", "33"];
    let (limited, limited_report) = run(&args, false)?;
    let (exact, exact_report) = run(&args, true)?;
    assert_eq!(limited.status.code(), Some(1));
    assert_eq!(limited_report["status"], "total_overflow");
    assert!(exact.status.success());
    assert_eq!(exact_report["status"], "complete");
    assert_eq!(limited_report["counts"], exact_report["counts"]);
    let factorial: BigUint = (1u32..=34).map(BigUint::from).product();
    assert_eq!(exact_report["total"], (factorial * 2u32).to_string());
    Ok(())
}

#[test]
fn constrained_counts_match_default_across_partition_budgets() -> Result<()> {
    for grid in ["2x3", "3x3", "2x2x2"] {
        let (_, expected) = run(&[grid, "--memory-limit", "1MiB"], false)?;
        for budget in ["0", "1", "512", "1MiB"] {
            let (output, actual) = run(&[grid, "--memory-limit", budget], true)?;
            assert!(
                output.status.success(),
                "{grid}, {budget}: {:?}",
                output.stderr
            );
            assert_eq!(actual, expected, "{grid}, {budget}");
        }
    }
    Ok(())
}

#[test]
fn big_counts_support_human_text_output() -> Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_andlock"))
        .args([
            "--free-points",
            "35",
            "--min-length",
            "35",
            "--big-counts",
            "--human",
            "--quiet",
        ])
        .output()?;
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let text = std::str::from_utf8(&output.stdout)?;
    let row = text
        .lines()
        .find(|line| line.trim_start().starts_with("35 "))
        .context("length 35 row")?;
    let count = row.split_whitespace().nth(1).context("human count")?;
    assert!(count.contains('_'));
    let factorial: BigUint = (1u32..=35).map(BigUint::from).product();
    assert_eq!(count.replace('_', ""), factorial.to_string());
    Ok(())
}

#[test]
fn big_counts_reject_grid_export() -> Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_andlock"))
        .args(["3x3", "--big-counts", "--export-json"])
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(std::str::from_utf8(&output.stderr)?.contains("cannot be used with"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn arbitrary_precision_interruption_preserves_exact_partial_output() -> Result<()> {
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;

    let mut child = Command::new(env!("CARGO_BIN_EXE_andlock"))
        .args([
            "4x4",
            "-f",
            "111",
            "--memory-limit",
            "1KiB",
            "--big-counts",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stderr = BufReader::new(child.stderr.take().context("child stderr")?);
    let mut diagnostics = String::new();
    // Preview output follows handler installation and precedes counting.
    assert!(stderr.read_line(&mut diagnostics)? > 0);
    let signal = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()?;
    assert!(signal.success());
    let output = child.wait_with_output()?;
    stderr.read_to_string(&mut diagnostics)?;
    assert_eq!(output.status.code(), Some(130), "{diagnostics}");
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["status"], "interrupted");
    let counts = report["counts"].as_array().context("count array")?;
    assert!(!counts.is_empty());
    let mut total = BigUint::default();
    for (length, entry) in counts.iter().enumerate() {
        assert_eq!(entry["length"], length);
        let decimal = entry["count"].as_str().context("decimal count")?;
        total += BigUint::parse_bytes(decimal.as_bytes(), 10).context("valid decimal count")?;
    }
    assert_eq!(report["completed_range"]["max_length"], counts.len() - 1);
    assert_eq!(report["total"], total.to_string());
    assert!(diagnostics.contains("Interrupted"));
    assert!(!diagnostics.contains("error:"));
    Ok(())
}
