# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1](https://github.com/Kuenlun/andlock/compare/v0.2.0...v0.2.1) - 2026-04-19

### Added

- *(cli)* render terminal grid preview before pattern enumeration ([#10](https://github.com/Kuenlun/andlock/pull/10))
- introduce compact JSON export format with inline numeric arrays ([#8](https://github.com/Kuenlun/andlock/pull/8))
- introduce pattern-simplifier library with canonical-form normalization ([#5](https://github.com/Kuenlun/andlock/pull/5))

### Fixed

- support zero-sized grid dimensions ([#6](https://github.com/Kuenlun/andlock/pull/6))

### Other

- use closed-form formula for unconstrained pattern counting ([#9](https://github.com/Kuenlun/andlock/pull/9))

## [0.2.0](https://github.com/Kuenlun/andlock/compare/v0.1.0...v0.2.0) - 2026-04-18

### Added

- `grid` subcommand: accepts an `NxMx…` dimension spec (case-insensitive separators), `--free-points N` to add non-collinear orthogonal nodes, `--export-json` to serialise the `GridDefinition` to stdout without counting, and `--min-length`/`--max-length` to filter output and prune the DP early — reducing runtime from O(N²·2ᴺ) to O(N²·Σ C(N,k)) for tight caps ([#4](https://github.com/Kuenlun/andlock/pull/4))
- `file` subcommand: loads a `GridDefinition` from a JSON file path or `-` to read from stdin, with `--export-json` to re-serialise the loaded grid without counting ([#4](https://github.com/Kuenlun/andlock/pull/4))
- Composable pipeline support: `andlock grid "3x3" --export-json | andlock file -` ([#4](https://github.com/Kuenlun/andlock/pull/4))
- `-q`/`--quiet` flag on both subcommands to suppress all progress output ([#4](https://github.com/Kuenlun/andlock/pull/4))
- Progress bar via `indicatif` with elapsed timing footer; core solver decoupled from UI via `on_mask` callback ([#4](https://github.com/Kuenlun/andlock/pull/4))
- Human-readable I/O error messages via `anyhow`: file-not-found and permission-denied use locale-independent `ErrorKind` mappings; JSON parse errors include the file path and strip `serde_json`'s `Error(…)` wrapper ([#4](https://github.com/Kuenlun/andlock/pull/4))
- Input validation: reject `--min-length`/`--max-length` combined with `--export-json`; detect and report duplicate grid coordinates (with indices and coordinates) before any computation ([#4](https://github.com/Kuenlun/andlock/pull/4))

### Changed

- CLI restructured from a hard-coded execution path to a composable, self-documenting interface built on `clap` 4.6 ([#4](https://github.com/Kuenlun/andlock/pull/4))
- Progress and timing output redirected to stderr; stdout now contains only the numeric results table, making pipe-based scripting reliable ([#4](https://github.com/Kuenlun/andlock/pull/4))
- Output table separator width is now dynamic, matching the widest result line ([#4](https://github.com/Kuenlun/andlock/pull/4))
- Core logic split into focused modules: `cli`, `dp`, and `grid` ([#4](https://github.com/Kuenlun/andlock/pull/4))
- Help text purged of "DP" jargon; user-facing strings now use "counting patterns" and "block matrix" ([#4](https://github.com/Kuenlun/andlock/pull/4))

### Fixed

- Widen the pattern-count accumulator from `u64` to `u128` to prevent overflow for grids with n ≥ 21 points ([#4](https://github.com/Kuenlun/andlock/pull/4))
- Widen the DP table entries from `u64` to `u128` to prevent per-cell wrap-around for long paths at n ≥ 22, where per-path counts can reach (k−1)! ([#4](https://github.com/Kuenlun/andlock/pull/4))

### Other

- release v0.1.0 ([#2](https://github.com/Kuenlun/andlock/pull/2))

## [0.1.0](https://github.com/Kuenlun/andlock/releases/tag/v0.1.0) - 2026-04-17

### Added

- implement n-dimensional unlock pattern counter with bitmask DP ([#1](https://github.com/Kuenlun/andlock/pull/1))
- accept grid definition as JSON input via `serde` deserialization ([#1](https://github.com/Kuenlun/andlock/pull/1))
- validate input enforcing a 25-point ceiling and per-point dimension consistency ([#1](https://github.com/Kuenlun/andlock/pull/1))
- add test suite covering the canonical 3×3 Android grid, n-dimensional collinearity, and edge cases ([#1](https://github.com/Kuenlun/andlock/pull/1))

### Fixed

- add execute permission to `check-license-headers.sh` ([#1](https://github.com/Kuenlun/andlock/pull/1))

### Other

- update .gitignore to exclude .vscode and .claude directories
- add GitHub Actions workflows, pre-commit hook, and license header enforcement
- add package metadata and enforce strict Rust/Clippy linting and nightly toolchain
- update README to provide detailed explanation of program concept, rules, and computational complexity
- initialize Rust project with Cargo configuration and main file
- initial commit
