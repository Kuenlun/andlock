// Numerical oracles at the binary boundary.
//
// Unit tests already pin the DP's per-length output for 3x3; these tests
// run the *binary*, parse its stdout, and verify the totals a user would
// see. Catches regressions in argv plumbing, length filtering, and the
// final summary line that unit tests cannot.

mod common;

use common::{
    COUNT_1X9_FULL, COUNT_2X2_FULL, COUNT_2X3X2_FULL, COUNT_3X3_FULL, COUNT_3X3_LEN_4_TO_9,
    COUNT_3X3_LEN_9, COUNT_4X4_FULL, bin, parse_counts, parse_total,
};

fn run_total(args: &[&str]) -> (u128, Vec<(u32, u128)>) {
    let assert = bin().args(args).arg("--quiet").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let counts = parse_counts(&stdout);
    let total = parse_total(&stdout);
    (
        total.unwrap_or_else(|| panic!("no Total line in:\n{stdout}")),
        counts,
    )
}

#[test]
fn grid_3x3_full_total_matches_oracle() {
    let (total, counts) = run_total(&["grid", "3x3"]);
    assert_eq!(total, COUNT_3X3_FULL);
    assert!(
        counts
            .iter()
            .any(|&(len, c)| len == 9 && c == COUNT_3X3_LEN_9)
    );
}

#[test]
fn grid_3x3_min_length_4_matches_android_total() {
    let (total, counts) = run_total(&["grid", "3x3", "--min-length", "4"]);
    assert_eq!(total, COUNT_3X3_LEN_4_TO_9);
    assert!(counts.iter().all(|&(len, _)| len >= 4));
}

#[test]
fn grid_3x3_min_4_max_9_brackets_pattern_lengths() {
    let (total, counts) = run_total(&["grid", "3x3", "--min-length", "4", "--max-length", "9"]);
    assert_eq!(total, COUNT_3X3_LEN_4_TO_9);
    assert!(counts.iter().all(|&(len, _)| (4..=9).contains(&len)));
}

#[test]
fn grid_4x4_full_total_matches_oracle() {
    let (total, _) = run_total(&["grid", "4x4"]);
    assert_eq!(total, COUNT_4X4_FULL);
}

#[test]
fn grid_2x2_full_total_matches_oracle() {
    let (total, _) = run_total(&["grid", "2x2"]);
    assert_eq!(total, COUNT_2X2_FULL);
}

#[test]
fn grid_1x9_full_total_matches_oracle() {
    let (total, _) = run_total(&["grid", "1x9"]);
    assert_eq!(total, COUNT_1X9_FULL);
}

#[test]
fn grid_zero_axis_collapses_to_empty_pattern_only() {
    let (total, counts) = run_total(&["grid", "0"]);
    assert_eq!(total, 1);
    assert_eq!(counts, vec![(0, 1)]);

    let (total, _) = run_total(&["grid", "0x3"]);
    assert_eq!(total, 1);
}

#[test]
fn grid_1x1_reports_two_patterns() {
    let (total, _) = run_total(&["grid", "1x1"]);
    assert_eq!(total, 2);
}

#[test]
fn grid_max_length_zero_matches_empty_pattern() {
    let (total, counts) = run_total(&["grid", "3x3", "--max-length", "0"]);
    assert_eq!(total, 1);
    assert_eq!(counts, vec![(0, 1)]);
}

#[test]
fn grid_min_max_9_isolates_length_9_count() {
    let (total, counts) = run_total(&["grid", "3x3", "--min-length", "9", "--max-length", "9"]);
    assert_eq!(total, COUNT_3X3_LEN_9);
    assert_eq!(counts, vec![(9, COUNT_3X3_LEN_9)]);
}

#[test]
fn grid_3d_mixed_case_separator_parses() {
    let (total, _) = run_total(&["grid", "2X3x2"]);
    assert_eq!(total, COUNT_2X3X2_FULL);
}
