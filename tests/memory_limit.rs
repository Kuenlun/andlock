// Memory-limit clamping behaviour.
//
// The clamp is the only resource-management feature in the binary; its UX
// is part of the public contract. These tests pin the truncation logic
// and the warning text without re-testing the byte-counting helpers
// covered in unit tests.

mod common;

use common::{bin, parse_counts, parse_total};

#[test]
fn zero_budget_keeps_only_the_empty_pattern() {
    let assert = bin()
        .args(["grid", "3x3", "--memory-limit", "0", "--quiet"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    let counts = parse_counts(&stdout);
    assert_eq!(counts, vec![(0, 1)]);
    // A clamped run omits the Total line so the partial result stands on its own.
    assert_eq!(parse_total(&stdout), None);
    // --quiet suppresses the "Lengths X–Y skipped" warning too.
    assert!(
        stderr.is_empty(),
        "stderr should be empty under --quiet, got: {stderr}"
    );
}

#[test]
fn tight_budget_emits_skip_warning_on_stderr() {
    let assert = bin()
        .args(["grid", "3x3", "--memory-limit", "0"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("Lengths 1") && stderr.contains("9 skipped"),
        "stderr should mention the skipped range, got: {stderr}"
    );
}

#[test]
fn truncated_run_still_exits_zero() {
    bin()
        .args(["grid", "3x3", "--memory-limit", "0"])
        .assert()
        .code(0);
}

#[test]
fn binary_unit_suffixes_all_parse() {
    for suffix in ["", "K", "KiB", "M", "MiB", "G", "GiB"] {
        let limit = format!("1{suffix}");
        bin()
            .args(["grid", "1", "--memory-limit", &limit, "--quiet"])
            .assert()
            .success();
    }
}

#[test]
fn generous_budget_matches_unconstrained_run() {
    let with_limit = bin()
        .args(["grid", "3x3", "--memory-limit", "1G", "--quiet"])
        .assert()
        .success();
    let with_limit_out = String::from_utf8_lossy(&with_limit.get_output().stdout).into_owned();

    let without_limit = bin().args(["grid", "3x3", "--quiet"]).assert().success();
    let without_limit_out =
        String::from_utf8_lossy(&without_limit.get_output().stdout).into_owned();

    assert_eq!(
        parse_counts(&with_limit_out),
        parse_counts(&without_limit_out)
    );
    assert_eq!(
        parse_total(&with_limit_out),
        parse_total(&without_limit_out)
    );
}
