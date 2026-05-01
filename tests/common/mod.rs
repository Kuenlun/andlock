// Shared helpers for the andlock integration test suite.
//
// Tests drive the binary through `assert_cmd`, then parse its stdout into
// structural data so assertions can target what the user sees rather than
// fragile snapshots. The reference oracles live here so any test can
// reuse a single source of truth.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc
)]

use assert_cmd::Command;

pub fn bin() -> Command {
    Command::cargo_bin("andlock").unwrap()
}

/// Parses every `Len  Count` table row out of `stdout` into `(length, count)`.
///
/// A row is any line whose first two whitespace-separated tokens both parse
/// as integers (decimal, with optional `_` digit grouping when `--human` is
/// active). The parser deliberately skips header, separator, summary, and
/// preview lines because they fail one of those two checks.
pub fn parse_counts(stdout: &str) -> Vec<(u32, u128)> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let Some(first) = parts.next() else { continue };
        let Some(second) = parts.next() else { continue };
        if parts.next().is_some() {
            continue;
        }
        let Ok(len) = first.parse::<u32>() else {
            continue;
        };
        let Ok(count) = second.replace('_', "").parse::<u128>() else {
            continue;
        };
        out.push((len, count));
    }
    out
}

/// Returns the value on the `Total` summary line, if present.
pub fn parse_total(stdout: &str) -> Option<u128> {
    parse_summary(stdout, "Total")
}

/// Returns the value on the `Points` summary line.
pub fn parse_points(stdout: &str) -> Option<u128> {
    parse_summary(stdout, "Points")
}

fn parse_summary(stdout: &str, label: &str) -> Option<u128> {
    for line in stdout.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(label) {
            return rest.trim().replace('_', "").parse::<u128>().ok();
        }
    }
    None
}

// Reference oracles — well-established totals re-used across groups.
pub const COUNT_3X3_FULL: u128 = 389_498;
pub const COUNT_3X3_LEN_4_TO_9: u128 = 389_112;
pub const COUNT_3X3_LEN_9: u128 = 140_704;
pub const COUNT_4X4_FULL: u128 = 4_350_069_824_957;
pub const COUNT_2X2_FULL: u128 = 65;
pub const COUNT_1X9_FULL: u128 = 1_014;
pub const COUNT_2X3X2_FULL: u128 = 926_184_729;
