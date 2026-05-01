// Mathematical invariants that must hold across arbitrary inputs.
//
// Survives algorithmic refactors because the assertion is a property, not
// a hard-coded number. C1/C2 in particular guard against bugs in
// coordinate handling that a square-grid oracle would never expose.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{bin, parse_counts, parse_total};

fn quiet_run(args: &[&str]) -> (Vec<(u32, u128)>, u128) {
    let assert = bin().args(args).arg("--quiet").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let counts = parse_counts(&stdout);
    let total = parse_total(&stdout).unwrap_or_else(|| panic!("missing Total in:\n{stdout}"));
    (counts, total)
}

#[test]
fn axis_swap_yields_identical_counts() {
    let (a, _) = quiet_run(&["grid", "2x4"]);
    let (b, _) = quiet_run(&["grid", "4x2"]);
    assert_eq!(a, b);
}

#[test]
fn three_axis_permutations_yield_identical_counts() {
    let (a, _) = quiet_run(&["grid", "2x3x2"]);
    let (b, _) = quiet_run(&["grid", "3x2x2"]);
    let (c, _) = quiet_run(&["grid", "2x2x3"]);
    assert_eq!(a, b);
    assert_eq!(b, c);
}

#[test]
fn total_equals_sum_of_per_length_counts() {
    for dims in ["2x3", "3x3", "1x9"] {
        let (counts, total) = quiet_run(&["grid", dims]);
        let summed: u128 = counts.iter().map(|&(_, c)| c).sum();
        assert_eq!(summed, total, "row sum != Total for {dims}");
    }
}

#[test]
fn length_filter_partitions_full_total() {
    let (_, full) = quiet_run(&["grid", "3x3"]);
    let (_, low) = quiet_run(&["grid", "3x3", "--max-length", "4"]);
    let (_, high) = quiet_run(&["grid", "3x3", "--min-length", "5"]);
    assert_eq!(low + high, full);
}

#[test]
fn min_length_one_excludes_only_the_empty_pattern() {
    let (_, with_empty) = quiet_run(&["grid", "3x3"]);
    let (_, without_empty) = quiet_run(&["grid", "3x3", "--min-length", "1"]);
    assert_eq!(with_empty, without_empty + 1);
}

#[test]
fn single_isolated_point_reports_total_two() {
    let (_, total) = quiet_run(&["grid", "1"]);
    assert_eq!(total, 2);

    // A degenerate 3D shape with a single node must agree.
    let (_, total) = quiet_run(&["grid", "1x1x1"]);
    assert_eq!(total, 2);
}
