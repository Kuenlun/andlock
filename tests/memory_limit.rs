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
    // --quiet suppresses the "warning: insufficient memory …" line too.
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
        stderr.contains("warning:") && stderr.contains("insufficient memory"),
        "stderr should carry a standard `warning:` line, got: {stderr}"
    );
    assert!(
        stderr.contains("--max-length 0"),
        "warning should name the equivalent --max-length value inline, got: {stderr}"
    );
    assert!(
        stderr.contains("available"),
        "warning should report the budget shortfall, got: {stderr}"
    );
}

#[test]
fn skip_warning_appears_before_progress_region() {
    // The whole point of moving the warning to the top of the run is so
    // the user sees the cap before waiting on the DP. Pin that ordering
    // by asserting the warning line comes before the elapsed-time line.
    let assert = bin()
        .args(["grid", "3x3", "--memory-limit", "0"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    let warn_idx = stderr.find("warning:").expect("expected warning line");
    let footer_idx = stderr.find("Counted up to").expect("expected footer line");
    assert!(
        warn_idx < footer_idx,
        "warning must precede the elapsed-time footer, got: {stderr}"
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
fn min_length_filter_with_zero_budget_emits_only_the_summary_block() {
    // End-to-end: when `--memory-limit 0` clamps the effective max to 0
    // and `--min-length 1` excludes the empty pattern, every length is
    // filtered out. The CLI must still print the `Points` summary line,
    // omit both the table and the `Total` row, and exit successfully.
    let assert = bin()
        .args([
            "grid",
            "3x3",
            "--memory-limit",
            "0",
            "--min-length",
            "1",
            "--quiet",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        parse_counts(&stdout).is_empty(),
        "expected no length rows when every length is filtered out, got:\n{stdout}",
    );
    // No `Total` row (clamped run), but `Points` is always emitted.
    assert_eq!(parse_total(&stdout), None);
    assert!(
        stdout.lines().any(|l| l.trim_start().starts_with("Points")),
        "expected a Points summary line in:\n{stdout}",
    );
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

/// Tight budget on the u64 dispatch arm: a 32-point line with
/// `--memory-limit 0` clamps `--max-length` to 0 and emits the same
/// `warning:` line as the u32 path. This exercises the clamp branch
/// of `run_dp_sequence::<u64>` so the wider monomorphisation gets
/// the same coverage as `Width::U32`.
#[test]
fn tight_budget_emits_skip_warning_at_u64_path() {
    let assert = bin()
        .args(["grid", "1x32", "--memory-limit", "0"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("warning:") && stderr.contains("insufficient memory"),
        "stderr should carry the standard warning at u64 width: {stderr}",
    );
}

/// Symmetric clamp test for the u128 dispatch arm.
#[test]
fn tight_budget_emits_skip_warning_at_u128_path() {
    let assert = bin()
        .args(["grid", "1x64", "--memory-limit", "0"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("warning:") && stderr.contains("insufficient memory"),
        "stderr should carry the standard warning at u128 width: {stderr}",
    );
}

/// Non-quiet run on the u64 dispatch arm at an effective length that
/// still triggers the DP progress bar (`effective >= 2`). Exercises
/// the `Some(pb)` arm of every progress-bar guard inside
/// `run_dp_sequence::<u64>` — the build spinner, the count bar, and
/// their symmetric `finish_and_clear` cleanup paths — so the wider
/// monomorphisation matches `Width::U32`'s branch coverage.
#[test]
fn non_quiet_run_at_u64_path_drives_progress_region() {
    bin()
        .args(["grid", "1x32", "--max-length", "3"])
        .assert()
        .success();
}

/// Symmetric non-quiet run on the u128 dispatch arm.
#[test]
fn non_quiet_run_at_u128_path_drives_progress_region() {
    bin()
        .args(["grid", "1x64", "--max-length", "3"])
        .assert()
        .success();
}
