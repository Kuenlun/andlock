// Streaming workflows advertised in `--help` examples.
//
// `file -` reads from stdin until EOF; the read path must agree with the
// regular file path on bytes the user wrote elsewhere.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::Write;

use common::{bin, parse_total};

#[test]
fn pipe_export_into_file_reads_back_identical_total() {
    let exported = bin()
        .args(["grid", "3x3", "--export-json"])
        .assert()
        .success();
    let json = String::from_utf8_lossy(&exported.get_output().stdout).into_owned();

    let counted = bin()
        .args(["file", "-", "--quiet"])
        .write_stdin(json)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&counted.get_output().stdout).into_owned();
    assert_eq!(parse_total(&stdout), Some(389_498));
}

#[test]
fn stdin_path_and_disk_path_agree() {
    let json = bin()
        .args(["grid", "3x3", "--export-json"])
        .assert()
        .success();
    let json = String::from_utf8_lossy(&json.get_output().stdout).into_owned();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grid.json");
    std::fs::File::create(&path)
        .unwrap()
        .write_all(json.as_bytes())
        .unwrap();

    let from_disk = bin()
        .arg("file")
        .arg(&path)
        .arg("--quiet")
        .assert()
        .success();
    let from_disk = String::from_utf8_lossy(&from_disk.get_output().stdout).into_owned();

    let from_stdin = bin()
        .args(["file", "-", "--quiet"])
        .write_stdin(json)
        .assert()
        .success();
    let from_stdin = String::from_utf8_lossy(&from_stdin.get_output().stdout).into_owned();

    assert_eq!(parse_total(&from_disk), parse_total(&from_stdin));
}
