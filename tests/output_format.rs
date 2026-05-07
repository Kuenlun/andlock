// Output-format stability for downstream consumers.
//
// Anyone scripting around `andlock` depends on these guarantees: counts go
// to stdout, diagnostics to stderr, `--human` is opt-in, exit codes follow
// the documented convention.

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

    assert!(!stdout.contains("warning:"));
    assert!(stderr.contains("warning:") && stderr.contains("insufficient memory"));
    // The clamped counts the user does see still land on stdout.
    assert!(parse_counts(&stdout).iter().any(|&(len, _)| len == 0));
}

#[test]
fn exit_codes_match_documented_convention() {
    // 0 — success.
    bin().args(["grid", "3x3", "--quiet"]).assert().code(0);
    // 1 — runtime error (validation here is a runtime, not clap, error).
    // 12x12 = 144 points exceeds the 127-point ceiling.
    bin().args(["grid", "12x12"]).assert().code(1);
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

#[test]
fn file_subcommand_renders_preview_when_not_quiet() {
    // The File arm's preview branch is symmetric to the Grid arm's:
    // when stdout is interactive and `render_preview` produces output,
    // the binary prints it before the table. We feed a 3×3 grid via
    // stdin and assert on the preview glyph the renderer emits.
    let assert = bin()
        .args(["file", "-"])
        .write_stdin(
            r#"{"dimensions":2,"points":[[0,0],[1,0],[2,0],[0,1],[1,1],[2,1],[0,2],[1,2],[2,2]]}"#,
        )
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        stdout.contains('●'),
        "file subcommand should emit the grid preview when not --quiet:\n{stdout}"
    );
}

#[test]
fn export_json_paired_with_length_flag_warns_on_stderr() {
    // `--export-json` ignores `--min-length` and `--max-length`; the
    // user-visible warning is part of the documented contract for
    // accidental flag pairing. `--quiet` suppresses it (already pinned
    // by `memory_limit::zero_budget_keeps_only_the_empty_pattern`).
    let assert = bin()
        .args(["grid", "3x3", "--export-json", "--min-length", "4"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("--min-length and --max-length have no effect with --export-json"),
        "expected warning on stderr, got: {stderr}",
    );
}

#[test]
fn export_json_quiet_suppresses_ignored_range_warning() {
    // Symmetric guard for the suppression branch of the same warning.
    let assert = bin()
        .args([
            "grid",
            "3x3",
            "--export-json",
            "--max-length",
            "4",
            "--quiet",
        ])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.is_empty(),
        "--quiet must suppress the ignored-range warning, got: {stderr}"
    );
}

#[test]
fn export_json_with_only_max_length_still_warns() {
    // The ignored-range warning fires whenever either length flag is set,
    // not only when `--min-length` is. Without this case the `||` short-
    // circuits on the `min_length` branch and the `max_length` arm of the
    // condition is never evaluated to true.
    let assert = bin()
        .args(["grid", "3x3", "--export-json", "--max-length", "4"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("--min-length and --max-length have no effect with --export-json"),
        "expected warning on stderr when only --max-length is set, got: {stderr}",
    );
}

#[test]
fn grid_subcommand_skips_preview_for_high_dimensional_grids() {
    // 3D base grids fall outside `render_preview`'s supported shapes, so
    // the helper returns `None`. Without `--quiet` the binary must still
    // succeed and emit no preview glyph — exercising the False arm of
    // the `let Some(preview)` guard in the Grid subcommand.
    let assert = bin().args(["grid", "2x2x2"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        !stdout.contains('●') && !stdout.contains('★'),
        "3D grid must skip the preview block:\n{stdout}",
    );
}

#[test]
fn file_subcommand_skips_preview_for_high_dimensional_grids() {
    // Mirror of the Grid case for the File arm: a 3D grid loaded from
    // JSON without `--quiet` must succeed and produce no preview.
    let assert = bin()
        .args(["file", "-"])
        .write_stdin(
            r#"{"dimensions":3,"points":[[0,0,0],[1,0,0],[0,1,0],[0,0,1],[1,1,0],[1,0,1],[0,1,1],[1,1,1]]}"#,
        )
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        !stdout.contains('●') && !stdout.contains('★'),
        "3D grid loaded from JSON must skip the preview:\n{stdout}",
    );
}
