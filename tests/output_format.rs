// Output-format stability for downstream consumers.
//
// Anyone scripting around `andlock` depends on these guarantees: counts go
// to stdout, diagnostics to stderr, `--human` is opt-in, exit codes follow
// the documented convention.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{bin, parse_counts, parse_points, parse_total};

#[test]
fn quiet_strips_preview_and_timing_but_keeps_counts() {
    let assert = bin().args(["grid", "3x3", "--quiet"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    // No preview glyph, no timing line.
    assert!(!stdout.contains('●'));
    assert!(!stderr.contains("Counted in"));
    assert!(
        stderr.is_empty(),
        "unexpected stderr in quiet mode: {stderr}"
    );

    let counts = parse_counts(&stdout);
    assert_eq!(counts.len(), 10, "expected one row per length 0..=9");
}

#[test]
fn quiet_and_default_agree_on_counts() {
    let default_assert = bin().args(["grid", "3x3"]).assert().success();
    let default_counts = parse_counts(&String::from_utf8_lossy(
        &default_assert.get_output().stdout,
    ));

    let quiet_assert = bin().args(["grid", "3x3", "--quiet"]).assert().success();
    let quiet_counts = parse_counts(&String::from_utf8_lossy(&quiet_assert.get_output().stdout));

    assert_eq!(default_counts, quiet_counts);
}

#[test]
fn human_groups_long_counts_with_underscores() {
    let assert = bin()
        .args(["grid", "3x3", "--quiet", "--human"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        stdout.contains("140_704"),
        "expected underscore grouping in:\n{stdout}"
    );
    // Short counts must stay un-grouped.
    assert!(stdout.contains(" 56"));
    assert!(!stdout.contains("0_56"));
}

#[test]
fn human_is_off_by_default_for_pipe_safety() {
    let assert = bin().args(["grid", "3x3", "--quiet"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        !stdout.contains('_'),
        "default output must not group digits:\n{stdout}"
    );
}

#[test]
fn quiet_human_combination_still_parses() {
    let assert = bin()
        .args(["grid", "3x3", "--quiet", "--human"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let total = parse_total(&stdout).unwrap();
    assert_eq!(total, 389_498);
}

#[test]
fn points_line_matches_actual_point_count() {
    let assert = bin().args(["grid", "3x3", "--quiet"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert_eq!(parse_points(&stdout), Some(9));

    let assert = bin()
        .args(["grid", "3x3", "--free-points", "2", "--quiet"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert_eq!(parse_points(&stdout), Some(11));
}

#[test]
fn counts_go_to_stdout_warnings_to_stderr() {
    let assert = bin()
        .args(["grid", "3x3", "--memory-limit", "0"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    assert!(!stdout.contains("skipped"));
    assert!(stderr.contains("skipped"));
    // The clamped counts the user does see still land on stdout.
    assert!(parse_counts(&stdout).iter().any(|&(len, _)| len == 0));
}

#[test]
fn exit_codes_match_documented_convention() {
    // 0 — success.
    bin().args(["grid", "3x3", "--quiet"]).assert().code(0);
    // 1 — runtime error (validation here is a runtime, not clap, error).
    bin().args(["grid", "6x6"]).assert().code(1);
    // 2 — clap parse error.
    bin().args(["grid", "3x3", "--bogus-flag"]).assert().code(2);
}

#[test]
fn output_ends_with_single_trailing_newline() {
    let assert = bin().args(["grid", "3x3", "--quiet"]).assert().success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    assert!(stdout.ends_with('\n'), "stdout must end with a newline");
    assert!(
        !stdout.ends_with("\n\n"),
        "stdout must not end with a blank line"
    );
}
