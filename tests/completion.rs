// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Process-level completion, partial-result, and diagnostic contracts.

use std::process::{Command, Output};

use anyhow::Result;

fn run(args: &[&str]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_andlock"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
}

fn rows(text: &str) -> Vec<(usize, u128)> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let length = fields.next()?.parse().ok()?;
            let count = fields.next()?.parse().ok()?;
            fields.next().is_none().then_some((length, count))
        })
        .collect()
}

fn total(text: &str) -> Option<u128> {
    text.lines()
        .find_map(|line| line.trim_start().strip_prefix("Total")?.trim().parse().ok())
}

#[test]
fn zero_table_budget_preserves_the_requested_range() -> Result<()> {
    let reference = run(&["3x3", "--memory-limit", "1MiB", "-q"])?;
    for quiet in [false, true] {
        let mut args = vec!["3x3", "--memory-limit", "0"];
        if quiet {
            args.push("--quiet");
        }
        let output = run(&args)?;
        assert!(output.status.success());
        assert_eq!(output.stdout, reference.stdout);
        let stderr = std::str::from_utf8(&output.stderr)?;
        assert!(!stderr.contains("warning:"), "{stderr}");
        if quiet {
            assert!(stderr.is_empty(), "{stderr}");
        }
    }
    Ok(())
}

#[test]
fn zero_table_budget_counts_selected_long_lengths() -> Result<()> {
    let output = run(&["3x3", "--memory-limit", "0", "--min-length", "9", "-q"])?;
    let stdout = std::str::from_utf8(&output.stdout)?;
    assert!(output.status.success());
    assert_eq!(rows(stdout), [(9, 140_704)]);
    assert_eq!(total(stdout), Some(140_704));
    Ok(())
}

#[test]
fn count_overflow_preserves_every_exact_length_even_when_quiet() -> Result<()> {
    for quiet in [false, true] {
        let mut args = vec!["--free-points", "35"];
        if quiet {
            args.push("--quiet");
        }
        let output = run(&args)?;
        let stdout = std::str::from_utf8(&output.stdout)?;
        let stderr = std::str::from_utf8(&output.stderr)?;
        let expected: Vec<_> = (0..=30)
            .map(|length| (length, ((36 - length)..=35).map(|n| n as u128).product()))
            .collect();
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert_eq!(rows(stdout), expected);
        assert_eq!(total(stdout), Some(expected.iter().map(|(_, n)| n).sum()));
        assert!(
            stderr.contains("counts past length 30 do not fit in u128"),
            "{stderr}"
        );
        assert!(
            stderr.contains("length range 0..=35 is incomplete"),
            "{stderr}"
        );
    }
    Ok(())
}

#[test]
fn count_overflow_before_minimum_does_not_report_a_zero_total() -> Result<()> {
    let output = run(&["--free-points", "35", "--min-length", "31", "-q"])?;
    let stdout = std::str::from_utf8(&output.stdout)?;
    let stderr = std::str::from_utf8(&output.stderr)?;
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(rows(stdout).is_empty(), "{stdout}");
    assert_eq!(total(stdout), None);
    assert!(
        stderr.contains("counts past length 30 do not fit in u128"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn selected_range_total_overflow_fails_with_exact_rows_even_when_quiet() -> Result<()> {
    for quiet in [false, true] {
        let mut args = vec!["--free-points", "34", "--min-length", "33"];
        if quiet {
            args.push("--quiet");
        }
        let output = run(&args)?;
        let stdout = std::str::from_utf8(&output.stdout)?;
        let stderr = std::str::from_utf8(&output.stderr)?;
        let factorial: u128 = (1..=34).product();
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert_eq!(rows(stdout), [(33, factorial), (34, factorial)]);
        assert_eq!(total(stdout), None);
        assert!(
            stderr.contains("sum across selected lengths does not fit in u128"),
            "{stderr}"
        );
        assert!(
            stderr.contains("total for requested length range 33..=34"),
            "{stderr}"
        );
        assert!(!stderr.contains("is incomplete"), "{stderr}");
    }
    Ok(())
}

#[test]
fn excluded_lengths_do_not_overflow_the_selected_total() -> Result<()> {
    let output = run(&["--free-points", "34", "--min-length", "34", "-q"])?;
    let stdout = std::str::from_utf8(&output.stdout)?;
    let factorial: u128 = (1..=34).product();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    assert_eq!(rows(stdout), [(34, factorial)]);
    assert_eq!(total(stdout), Some(factorial));
    Ok(())
}

#[test]
fn empty_grid_and_zero_length_ranges_complete_with_no_scratch_budget() -> Result<()> {
    for args in [
        vec!["--free-points", "0", "--memory-limit", "0", "-q"],
        vec!["3x3", "--max-length", "0", "--memory-limit", "0", "-q"],
    ] {
        let output = run(&args)?;
        let stdout = std::str::from_utf8(&output.stdout)?;
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
        assert_eq!(rows(stdout), [(0, 1)]);
        assert_eq!(total(stdout), Some(1));
    }
    Ok(())
}

#[test]
fn successful_quiet_run_prints_only_counts_and_summary() -> Result<()> {
    let output = run(&["3x3", "--min-length", "4", "--max-length", "4", "-q"])?;
    let stdout = std::str::from_utf8(&output.stdout)?;
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    assert_eq!(rows(stdout), [(4, 1624)]);
    assert_eq!(total(stdout), Some(1624));
    Ok(())
}

#[test]
fn quiet_export_keeps_the_ignored_range_warning() -> Result<()> {
    let output = run(&["3x3", "--export-json", "--min-length", "4", "-q"])?;
    let stderr = std::str::from_utf8(&output.stderr)?;
    assert!(output.status.success(), "{stderr}");
    let grid: andlock::grid::GridDefinition = serde_json::from_slice(&output.stdout)?;
    assert_eq!(grid.node_count(), 9);
    assert!(
        stderr.contains("warning: --min-length and --max-length have no effect"),
        "{stderr}"
    );
    Ok(())
}
