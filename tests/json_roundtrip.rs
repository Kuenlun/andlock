// Grid ↔ JSON round-trip contract.
//
// Locks the JSON wire format and the canonicalisation pipeline at the
// binary boundary: bytes printed by `grid --export-json` must round-trip
// back through `file -` and produce identical counts.

mod common;

use std::io::Write;

use common::{bin, parse_counts, parse_total};

fn export_json(args_after_grid: &[&str]) -> String {
    let mut cmd = bin();
    cmd.arg("grid");
    cmd.args(args_after_grid);
    cmd.arg("--export-json");
    let out = cmd.assert().success();
    String::from_utf8_lossy(&out.get_output().stdout).into_owned()
}

fn count_from_stdin(json: &str) -> (Vec<(u32, u128)>, u128) {
    let assert = bin()
        .args(["file", "-", "--quiet"])
        .write_stdin(json.to_owned())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let counts = parse_counts(&stdout);
    let total = parse_total(&stdout).unwrap_or_else(|| panic!("missing Total in:\n{stdout}"));
    (counts, total)
}

fn count_from_grid(args_after_grid: &[&str]) -> (Vec<(u32, u128)>, u128) {
    let mut cmd = bin();
    cmd.arg("grid");
    cmd.args(args_after_grid);
    cmd.arg("--quiet");
    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let counts = parse_counts(&stdout);
    let total = parse_total(&stdout).unwrap_or_else(|| panic!("missing Total in:\n{stdout}"));
    (counts, total)
}

#[test]
fn export_then_file_yields_identical_counts() {
    let json = export_json(&["3x3"]);
    let (json_counts, json_total) = count_from_stdin(&json);
    let (grid_counts, grid_total) = count_from_grid(&["3x3"]);
    assert_eq!(json_counts, grid_counts);
    assert_eq!(json_total, grid_total);
}

#[test]
fn save_then_load_yields_identical_counts() {
    let json = export_json(&["3x3"]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grid.json");
    std::fs::File::create(&path)
        .unwrap()
        .write_all(json.as_bytes())
        .unwrap();

    let assert = bin()
        .arg("file")
        .arg(&path)
        .arg("--quiet")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let total = parse_total(&stdout).unwrap();

    let (_, grid_total) = count_from_grid(&["3x3"]);
    assert_eq!(total, grid_total);
}

#[test]
fn simplify_is_idempotent_through_json() {
    // Start with a translated, scaled 3x3 so simplify has work to do.
    let raw = r#"{
        "dimensions": 2,
        "points": [
            [10, 20], [13, 20], [16, 20],
            [10, 23], [13, 23], [16, 23],
            [10, 26], [13, 26], [16, 26]
        ]
    }"#;

    let once = bin()
        .args(["file", "-", "--export-json", "--simplify"])
        .write_stdin(raw.to_owned())
        .assert()
        .success();
    let once_out = String::from_utf8_lossy(&once.get_output().stdout).into_owned();

    let twice = bin()
        .args(["file", "-", "--export-json", "--simplify"])
        .write_stdin(once_out.clone())
        .assert()
        .success();
    let twice_out = String::from_utf8_lossy(&twice.get_output().stdout).into_owned();

    assert_eq!(once_out, twice_out);
}

#[test]
fn simplify_preserves_counts() {
    let raw = r#"{
        "dimensions": 2,
        "points": [
            [10, 20], [13, 20], [16, 20],
            [10, 23], [13, 23], [16, 23],
            [10, 26], [13, 26], [16, 26]
        ]
    }"#;

    let raw_counts = count_from_stdin(raw);

    let simplified = bin()
        .args(["file", "-", "--export-json", "--simplify"])
        .write_stdin(raw.to_owned())
        .assert()
        .success();
    let simplified_json = String::from_utf8_lossy(&simplified.get_output().stdout).into_owned();

    let simplified_counts = count_from_stdin(&simplified_json);
    assert_eq!(raw_counts, simplified_counts);
}

#[test]
fn simplify_requires_export_json_flag() {
    let raw = r#"{"dimensions":2,"points":[[0,0]]}"#;
    bin()
        .args(["file", "-", "--simplify"])
        .write_stdin(raw.to_owned())
        .assert()
        .code(2);
}

#[test]
fn export_json_is_byte_stable_across_runs() {
    let a = export_json(&["3x3"]);
    let b = export_json(&["3x3"]);
    assert_eq!(a, b);
}
