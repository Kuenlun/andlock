// User-facing error reporting.
//
// Unit tests already cover the `parse_dims` / `validate` error strings at
// the library level — these tests only verify that those messages survive
// the CLI wiring and reach **stderr** with a non-zero exit code.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use predicates::str::contains;

#[test]
fn unknown_dim_component_reports_named_component() {
    common::bin()
        .args(["grid", "abcd"])
        .assert()
        .failure()
        .stderr(contains("invalid dimension component 'abcd'"));
}

#[test]
fn negative_dim_component_reports_constraint() {
    common::bin()
        .args(["grid", "--", "-1x3"])
        .assert()
        .failure()
        .stderr(contains("must be >= 0"));
}

#[test]
fn empty_dim_component_is_rejected() {
    common::bin()
        .args(["grid", "3x"])
        .assert()
        .failure()
        .stderr(contains("invalid dimension component"));
}

#[test]
fn too_many_points_reports_max_supported() {
    common::bin()
        .args(["grid", "6x6"])
        .assert()
        .failure()
        .stderr(contains("36 points exceeds the supported maximum of 31"));
}

#[test]
fn min_length_above_max_length_reports_both_values() {
    common::bin()
        .args(["grid", "3x3", "--min-length", "5", "--max-length", "4"])
        .assert()
        .failure()
        .stderr(contains(
            "--min-length (5) must not exceed --max-length (4)",
        ));
}

#[test]
fn max_length_above_point_count_is_rejected() {
    common::bin()
        .args(["grid", "3x3", "--max-length", "100"])
        .assert()
        .failure()
        .stderr(contains(
            "--max-length (100) exceeds the number of points (9)",
        ));
}

#[test]
fn invalid_memory_size_is_rejected_by_clap() {
    common::bin()
        .args(["grid", "3x3", "--memory-limit", "invalid"])
        .assert()
        .code(2)
        .stderr(contains("invalid number in memory size"));
}

#[test]
fn missing_file_path_reports_kind_and_quoted_path() {
    common::bin()
        .args(["file", "definitely-does-not-exist.json"])
        .assert()
        .failure()
        .stderr(contains("not found"))
        .stderr(contains("definitely-does-not-exist.json"));
}

#[test]
fn malformed_json_is_rejected() {
    common::bin()
        .args(["file", "-"])
        .write_stdin("not json")
        .assert()
        .failure()
        .stderr(contains("failed to parse JSON"));
}

#[test]
fn json_without_dimensions_field_reports_missing_field() {
    common::bin()
        .args(["file", "-"])
        .write_stdin("{}")
        .assert()
        .failure()
        .stderr(contains("missing field"))
        .stderr(contains("dimensions"));
}

#[test]
fn json_with_duplicate_points_reports_indices() {
    common::bin()
        .args(["file", "-"])
        .write_stdin(r#"{"dimensions":2,"points":[[0,0],[0,0]]}"#)
        .assert()
        .failure()
        .stderr(contains("points 0 and 1 have the same coordinates"));
}

#[test]
fn json_with_dim_mismatch_reports_offending_point() {
    common::bin()
        .args(["file", "-"])
        .write_stdin(r#"{"dimensions":3,"points":[[0,0]]}"#)
        .assert()
        .failure()
        .stderr(contains("point 0 has 2 coordinate(s); expected 3"));
}

#[test]
fn json_with_too_many_points_reports_max_supported() {
    use std::fmt::Write as _;
    let mut json = String::from(r#"{"dimensions":1,"points":["#);
    for i in 0..32 {
        if i > 0 {
            json.push(',');
        }
        write!(json, "[{i}]").unwrap();
    }
    json.push_str("]}");
    common::bin()
        .args(["file", "-"])
        .write_stdin(json)
        .assert()
        .failure()
        .stderr(contains("32 points exceeds the supported maximum of 31"));
}
