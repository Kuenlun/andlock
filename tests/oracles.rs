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

/// 32 points exceeds the `u32` mask's 31-point ceiling; the pipeline
/// must dispatch to the `u64` width through the `Width::U64` arm in
/// `run_pipeline`. The `--max-length 3` cap exercises the
/// `need_dp_next == true` branch inside `process_layer<u64>` (popcount
/// layer p = 1 needs to emit prefix/suffix sums for layer 2). The
/// 1×32 line is constrained, so the run goes through the layered DP
/// rather than the closed-form fast path.
#[test]
fn grid_1x32_engages_wider_mask_path() {
    let (total, counts) = run_total(&["grid", "1x32", "--max-length", "3"]);
    // Length 1: 32 singletons; length 2: 2 × (n − 1) = 62 ordered
    // adjacent pairs. Length 3 oracle (counted independently by the
    // unit-test path) is 120 — pinned via the parsed total.
    assert!(counts.iter().any(|&(len, c)| len == 1 && c == 32));
    assert!(counts.iter().any(|&(len, c)| len == 2 && c == 62));
    assert!(counts.iter().any(|&(len, c)| len == 3 && c == 120));
    assert_eq!(total, 1 + 32 + 62 + 120);
}

/// 64 points exceeds the `u64` mask's 63-point ceiling; the pipeline
/// must dispatch to the `u128` width through the `Width::U128` arm.
/// Symmetric counterpart to `grid_1x32_engages_wider_mask_path`.
/// Length-2 oracle: 2 × (64 − 1) = 126; length-3 oracle: 248.
#[test]
fn grid_1x64_engages_widest_mask_path() {
    let (total, counts) = run_total(&["grid", "1x64", "--max-length", "3"]);
    assert!(counts.iter().any(|&(len, c)| len == 1 && c == 64));
    assert!(counts.iter().any(|&(len, c)| len == 2 && c == 126));
    assert!(counts.iter().any(|&(len, c)| len == 3 && c == 248));
    assert_eq!(total, 1 + 64 + 126 + 248);
}

/// Unconstrained `u64` path: 32 free points (each on an orthogonal
/// axis) means no triple is collinear, the block matrix is all zero,
/// and `count_patterns_dp::<u64>` takes the closed-form falling-
/// factorial fast path. The point count of 32 forces the
/// `Width::U64` dispatch arm; the unconstrained branch covers the
/// `count_unconstrained` shortcut for that monomorphisation.
#[test]
fn grid_0_free_32_engages_u64_unconstrained_fast_path() {
    let (total, counts) = run_total(&["grid", "0", "-f", "32", "--max-length", "3"]);
    // P(32, 1) = 32, P(32, 2) = 992, P(32, 3) = 29 760.
    assert!(counts.iter().any(|&(len, c)| len == 1 && c == 32));
    assert!(counts.iter().any(|&(len, c)| len == 2 && c == 992));
    assert!(counts.iter().any(|&(len, c)| len == 3 && c == 29_760));
    assert_eq!(total, 1 + 32 + 992 + 29_760);
}

/// Symmetric unconstrained test for the `u128` path: 64 free points
/// dispatches to `Width::U128` and the all-zero block matrix routes
/// through the closed-form fast path inside the wider monomorphisation.
#[test]
fn grid_0_free_64_engages_u128_unconstrained_fast_path() {
    let (total, counts) = run_total(&["grid", "0", "-f", "64", "--max-length", "3"]);
    // P(64, 1) = 64, P(64, 2) = 4 032, P(64, 3) = 249 984.
    assert!(counts.iter().any(|&(len, c)| len == 1 && c == 64));
    assert!(counts.iter().any(|&(len, c)| len == 2 && c == 4_032));
    assert!(counts.iter().any(|&(len, c)| len == 3 && c == 249_984));
    assert_eq!(total, 1 + 64 + 4_032 + 249_984);
}

/// Constrained `u64` path with `--max-length 0`: forces
/// `count_patterns_dp::<u64>` past the all-zero shortcut and through
/// the early `return counts;` branch when `max_length == 0`.
#[test]
fn grid_1x32_max_length_0_at_u64_path() {
    let (total, counts) = run_total(&["grid", "1x32", "--max-length", "0"]);
    assert_eq!(total, 1);
    assert_eq!(counts, vec![(0, 1)]);
}

/// Constrained `u64` path with `--max-length 1`: covers the second
/// early-return inside `count_patterns_dp::<u64>` when
/// `max_length < 2`. The DP body never enters the popcount loop, so
/// only the empty and singleton patterns are reported.
#[test]
fn grid_1x32_max_length_1_at_u64_path() {
    let (total, counts) = run_total(&["grid", "1x32", "--max-length", "1"]);
    assert_eq!(total, 1 + 32);
    assert!(counts.iter().any(|&(len, c)| len == 0 && c == 1));
    assert!(counts.iter().any(|&(len, c)| len == 1 && c == 32));
}

/// Symmetric `--max-length 0` test for the `u128` width.
#[test]
fn grid_1x64_max_length_0_at_u128_path() {
    let (total, counts) = run_total(&["grid", "1x64", "--max-length", "0"]);
    assert_eq!(total, 1);
    assert_eq!(counts, vec![(0, 1)]);
}

/// Symmetric `--max-length 1` test for the `u128` width.
#[test]
fn grid_1x64_max_length_1_at_u128_path() {
    let (total, counts) = run_total(&["grid", "1x64", "--max-length", "1"]);
    assert_eq!(total, 1 + 64);
    assert!(counts.iter().any(|&(len, c)| len == 0 && c == 1));
    assert!(counts.iter().any(|&(len, c)| len == 1 && c == 64));
}

/// 2×32 grid (n = 64) with mixed collinearity: pairs sharing a column
/// run along a 32-point line (so every non-adjacent same-column pair
/// has at least one collinear blocker), while cross-column pairs
/// differ on the x axis by 1 and so admit no integer point on the open
/// segment between them.
///
/// Length-2 oracle, derived from the geometry:
///   * 4032 ordered pairs total (= 64 × 63);
///   * same-column blocked = 2 columns × (32 × 31 − 2 × 31) = 1860
///     (every column has 32 × 31 ordered pairs and 2 × 31 adjacent
///     ones; the rest are blocked by an intermediate point);
///   * the remaining 124 same-column adjacents plus 2 × 32 × 32 = 2048
///     cross-column pairs are unblocked, giving 4032 − 1860 = 2172
///     patterns of length 2.
///
/// This pins the `collinear == false` arm of `compute_blocks::<u128>`
/// — which the all-collinear `1×64` grid cannot reach — to an exact
/// number, so a future arithmetic regression in the wider mask path
/// fails the test instead of slipping through a loose bound.
#[test]
fn grid_2x32_engages_u128_with_mixed_collinearity() {
    let (total, counts) = run_total(&["grid", "2x32", "--max-length", "2"]);
    assert!(counts.iter().any(|&(len, c)| len == 0 && c == 1));
    assert!(counts.iter().any(|&(len, c)| len == 1 && c == 64));
    assert!(counts.iter().any(|&(len, c)| len == 2 && c == 2_172));
    assert_eq!(total, 1 + 64 + 2_172);
}
