// CLI surface and exit-code contract.
//
// These tests exercise the wiring between argv, clap, the subcommand
// dispatcher, and process termination. Algorithmic correctness lives in
// `oracles.rs`; here we only check that the binary reaches the right
// branch and exits with the documented code.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use predicates::str::contains;

#[test]
fn version_prints_name_and_semver() {
    common::bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::starts_with("andlock "))
        .stdout(predicates::str::is_match(r"\d+\.\d+\.\d+").unwrap());
}

#[test]
fn top_level_help_lists_both_subcommands() {
    common::bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("grid"))
        .stdout(contains("file"));
}

#[test]
fn grid_help_lists_documented_flags() {
    common::bin()
        .args(["grid", "--help"])
        .assert()
        .success()
        .stdout(contains("--min-length"))
        .stdout(contains("--max-length"))
        .stdout(contains("--memory-limit"))
        .stdout(contains("--export-json"))
        .stdout(contains("--free-points"));
}

#[test]
fn file_help_lists_documented_flags() {
    common::bin()
        .args(["file", "--help"])
        .assert()
        .success()
        .stdout(contains("--simplify"))
        .stdout(contains("--export-json"))
        .stdout(contains("--memory-limit"));
}

#[test]
fn no_subcommand_fails_with_clap_exit_code() {
    common::bin().assert().code(2);
}

#[test]
fn missing_dims_argument_fails_with_clap_exit_code() {
    common::bin().arg("grid").assert().code(2);
}

#[test]
fn happy_path_3x3_exits_zero_with_non_empty_stdout() {
    let assert = common::bin()
        .args(["grid", "3x3", "--quiet"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(!stdout.trim().is_empty());
}
