// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Visit filters constrain ordered prefixes and remain reproducible in JSON.

use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

const VISITS_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/visits.json");

fn with_stdin(args: &[&str], input: &str) -> Result<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_andlock"))
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .context("child stdin")?
        .write_all(input.as_bytes())?;
    Ok(child.wait_with_output()?)
}

fn run(args: &[&str], visits: &str) -> Result<Output> {
    let mut arguments = args.to_vec();
    arguments.extend(["--visits", "-", "--json", "-q"]);
    with_stdin(&arguments, visits)
}

fn report(output: &Output) -> Result<Value> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn counts(report: &Value) -> Result<Vec<u128>> {
    report["counts"]
        .as_array()
        .context("count array")?
        .iter()
        .map(|entry| Ok(entry["count"].as_str().context("decimal count")?.parse()?))
        .collect()
}

#[test]
fn grouped_visits_normalize_and_define_the_default_range() -> Result<()> {
    let output = run(&["3x3"], "[[0,0],[1],[3,2,3]]")?;
    let report = report(&output)?;
    assert_eq!(report["visits"], json!([[0], [1], [2, 3]]));
    assert_eq!(
        report["requested_range"],
        json!({"min_length": 0, "max_length": 3})
    );
    assert_eq!(report["completed_range"], report["requested_range"]);
    assert_eq!(counts(&report)?, [1, 1, 1, 2]);
    assert_eq!(report["total"], "5");
    Ok(())
}

#[test]
fn exact_prefixes_and_empty_sets_keep_zero_counts_complete() -> Result<()> {
    for (visits, expected) in [
        ("[[0],[2],[1]]", vec![1, 1, 0, 0]),
        ("[[0],[],[1]]", vec![1, 1, 0, 0]),
        ("[[0],[0],[1]]", vec![1, 1, 0, 0]),
    ] {
        let output = run(&["3x3"], visits)?;
        let report = report(&output)?;
        assert_eq!(counts(&report)?, expected);
        assert_eq!(report["status"], "complete");
    }
    for (visits, expected) in [("[]", vec![1]), ("[[],[1],[2]]", vec![1, 0, 0, 0])] {
        let output = run(&["3x3", "--memory-limit", "0"], visits)?;
        let report = report(&output)?;
        assert_eq!(counts(&report)?, expected);
        assert_eq!(report["visits"], serde_json::from_str::<Value>(visits)?);
    }
    Ok(())
}

#[test]
fn free_nodes_still_obey_restrictions_on_future_visits() -> Result<()> {
    let output = run(&["--free-points", "4"], "[[0,1],[1,2],[0,2,3]]")?;
    assert_eq!(counts(&report(&output)?)?, [1, 2, 3, 5]);
    Ok(())
}

#[test]
fn explicit_ranges_apply_only_the_selected_filter_prefix() -> Result<()> {
    let output = run(
        &["3x3", "--min-length", "2", "--max-length", "2"],
        "[[0],[1],[2,3]]",
    )?;
    let report = report(&output)?;
    assert_eq!(report["counts"], json!([{"length": 2, "count": "1"}]));
    assert_eq!(report["visits"], json!([[0], [1], [2, 3]]));
    Ok(())
}

#[test]
fn invalid_visit_json_nodes_and_lengths_fail_before_counting() -> Result<()> {
    for visits in [
        "{}",
        "[true]",
        "[[-1]]",
        "[[1.5]]",
        "[[9]]",
        "[[],[],[],[],[],[],[],[],[],[]]",
    ] {
        let output = run(&["3x3"], visits)?;
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("visits"));
    }
    for args in [
        vec!["3x3", "--max-length", "4"],
        vec!["3x3", "--min-length", "4"],
        vec!["3x3", "--min-length", "3", "--max-length", "2"],
    ] {
        let output = run(&args, "[[0],[1],[2]]")?;
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
    }
    Ok(())
}

#[test]
fn grid_stdin_and_visit_file_can_be_combined() -> Result<()> {
    let grid = r#"{"dimensions":2,"points":[[0,0],[1,0],[2,0],[2,1]]}"#;
    let output = with_stdin(
        &["--file", "-", "--visits", VISITS_FILE, "--json", "-q"],
        grid,
    )?;
    let report = report(&output)?;
    assert_eq!(report["visits"], json!([[0], [1], [2, 3]]));
    assert_eq!(counts(&report)?, [1, 1, 1, 2]);
    Ok(())
}

#[test]
fn two_stdin_sources_are_rejected_without_waiting_for_input() -> Result<()> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_andlock"))
        .args(["--file", "-", "--visits", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let _stdin = child.stdin.take().context("child stdin")?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            bail!("conflicting stdin sources waited for input");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output()?;
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot both read from stdin"));
    Ok(())
}

#[test]
fn visit_filters_conflict_with_grid_export() -> Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_andlock"))
        .args(["3x3", "--visits", VISITS_FILE, "--export-json"])
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn memory_budgets_and_precision_preserve_filtered_json_results() -> Result<()> {
    let visits = "[[0,1,2,3,4,5,6,7,8],[0,1],[2,4,7],[3,5,8],[6,7]]";
    let expected = report(&run(&["3x3"], visits)?)?;
    for budget in ["0", "1", "64", "1024"] {
        for big in [false, true] {
            let mut args = vec!["3x3", "--memory-limit", budget];
            if big {
                args.push("--big-counts");
            }
            assert_eq!(report(&run(&args, visits)?)?, expected);
        }
    }
    Ok(())
}

#[test]
fn arbitrary_precision_counts_the_unrestricted_tail_after_a_fixed_prefix() -> Result<()> {
    let mut visits = vec![(0usize..40).collect::<Vec<_>>(); 40];
    visits[0] = vec![0];
    visits[1] = vec![1];
    let encoded = serde_json::to_string(&visits)?;
    let output = run(
        &[
            "--free-points",
            "40",
            "--big-counts",
            "--memory-limit",
            "0",
            "--min-length",
            "40",
        ],
        &encoded,
    )?;
    let report = report(&output)?;
    let expected = (1u128..=38)
        .fold(num_bigint::BigUint::from(1u128), |value, factor| {
            value * factor
        })
        .to_string();
    assert_eq!(report["counts"], json!([{"length":40,"count":expected}]));
    assert_eq!(report["total"], expected);
    assert_eq!(report["status"], "complete");
    Ok(())
}
