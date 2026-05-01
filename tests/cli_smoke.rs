// CLI surface and exit-code contract.
//
// These tests exercise the wiring between argv, clap, the subcommand
// dispatcher, and process termination. Algorithmic correctness lives in
// `oracles.rs`; here we only check that the binary reaches the right
// branch and exits with the documented code.

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
fn install_handler_failure_propagates_through_main() {
    // Other integration tests cover the Ok path when they spawn the binary;
    // this one forces the Err branch by pre-registering a dummy ctrlc
    // handler so the real `set_handler` returns `MultipleHandlers`. The
    // resulting error has to reach stderr through `main`'s `?`.
    common::bin()
        .env("ANDLOCK_FORCE_HANDLER_ERROR", "1")
        .args(["grid", "3x3", "--quiet"])
        .assert()
        .failure()
        .stderr(contains("handler"));
}

#[test]
fn sigint_handler_runs_cleanup_and_exits_with_sigint_code() {
    // Subprocess test for the SIGINT cleanup path: the debug-only escape
    // hatch invokes the registered handler synchronously so the cleanup
    // body executes and `process::exit` lands on the documented code.
    // 130 = 128 + SIGINT on Unix; 1 elsewhere (Cargo flags only the
    // STATUS_CONTROL_C_EXIT magic on Windows, so any non-zero is fine).
    let expected = if cfg!(unix) { 130 } else { 1 };
    common::bin()
        .env("ANDLOCK_FORCE_SIGINT_HANDLER", "1")
        .assert()
        .code(expected);
}

#[test]
fn pipeline_error_propagates_from_grid_subcommand() {
    // Grid arm: a failure inside `run_pipeline` must surface through
    // the `?` operator with the actionable scratch-alloc message
    // intact. The debug-only hatch hijacks `allocate_scratch` to
    // synthesize a real `TryReserveError`, exercising the full
    // failure path — `map_err` closure, `?`, and propagation up to
    // `cli::run` — exactly as a genuine OOM would.
    common::bin()
        .env("ANDLOCK_FORCE_PIPELINE_ERROR", "1")
        .args(["grid", "3x3", "--quiet"])
        .assert()
        .failure()
        .stderr(contains("could not allocate"))
        .stderr(contains("--max-length"));
}

#[test]
fn pipeline_error_propagates_from_file_subcommand() {
    // File arm: same propagation contract as the grid arm — the
    // failure must reach stderr and exit non-zero, not be swallowed.
    common::bin()
        .env("ANDLOCK_FORCE_PIPELINE_ERROR", "1")
        .args(["file", "-", "--quiet"])
        .write_stdin(r#"{"dimensions":2,"points":[[0,0]]}"#)
        .assert()
        .failure()
        .stderr(contains("could not allocate"));
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
