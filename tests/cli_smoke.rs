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
fn sigint_hatch_runs_cleanup_then_propagates_through_main() {
    // The debug hatch executes the cleanup body and bails through
    // `main`'s `?` instead of calling `process::exit(SIGINT_EXIT_CODE)`
    // — see the coverage note on `tty::handle_sigint`. So the
    // observable contract from this subprocess is anyhow's standard
    // failure path: non-zero exit plus the cleanup marker on stderr.
    // The real 128+SIGINT exit code belongs to `handle_sigint`, which
    // no portable test driver can trigger.
    common::bin()
        .env("ANDLOCK_FORCE_SIGINT_HANDLER", "1")
        .assert()
        .failure()
        .stderr(contains("simulated sigint cleanup"));
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
fn pipeline_error_propagates_at_u64_grid_width() {
    // The debug-only `ANDLOCK_FORCE_PIPELINE_ERROR` hatch lives inside
    // `allocate_scratch::<M>` and exists in every monomorphisation;
    // exercising it with a u64-width grid (`1x32` → 32 points) covers
    // the hatch in the `Width::U64` branch of the dispatcher.
    common::bin()
        .env("ANDLOCK_FORCE_PIPELINE_ERROR", "1")
        .args(["grid", "1x32", "--quiet"])
        .assert()
        .failure()
        .stderr(contains("could not allocate"))
        .stderr(contains("--max-length"));
}

#[test]
fn pipeline_error_propagates_at_u128_grid_width() {
    // Symmetric u128-width hatch test (`1x64` → 64 points) — covers
    // `allocate_scratch::<u128>`'s debug short-circuit so the third
    // monomorphisation has its early-return reached at least once.
    common::bin()
        .env("ANDLOCK_FORCE_PIPELINE_ERROR", "1")
        .args(["grid", "1x64", "--quiet"])
        .assert()
        .failure()
        .stderr(contains("could not allocate"))
        .stderr(contains("--max-length"));
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
