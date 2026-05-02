# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/Kuenlun/andlock/compare/v0.2.1...v0.3.0) - 2026-05-02

### Added

- *(cli)* unify run summary layout with --human and points line ([#27](https://github.com/Kuenlun/andlock/pull/27))
- *(cli)* [**breaking**] clamp max-length to memory budget via sysinfo auto-detection ([#25](https://github.com/Kuenlun/andlock/pull/25))
- *(cli)* [**breaking**] expose --algorithm and --memory-limit, drop sysinfo polling ([#22](https://github.com/Kuenlun/andlock/pull/22))
- *(signal)* handle Ctrl+C cleanly via shared MultiProgress ([#15](https://github.com/Kuenlun/andlock/pull/15))
- *(counter)* add memory-aware IDDFS counter and lift node limit to 31 ([#14](https://github.com/Kuenlun/andlock/pull/14))
- *(dp)* stream per-length counts as they are finalized during DP traversal ([#12](https://github.com/Kuenlun/andlock/pull/12))

### Fixed

- *(cli)* skip memory clamp when block matrix is unconstrained ([#29](https://github.com/Kuenlun/andlock/pull/29))
- *(cli)* warn instead of error when range flags combine with --export-json ([#18](https://github.com/Kuenlun/andlock/pull/18))

### Other

- enforce 100% coverage including branches in pre-commit hook ([#35](https://github.com/Kuenlun/andlock/pull/35))
- *(cli)* expand unit and subprocess coverage of the binary surface ([#34](https://github.com/Kuenlun/andlock/pull/34))
- *(cli)* split into memory, output, pipeline, and tty modules ([#33](https://github.com/Kuenlun/andlock/pull/33))
- *(counter)* hoist dp allocation and expand coverage tooling ([#32](https://github.com/Kuenlun/andlock/pull/32))
- *(cli)* add integration coverage for the binary surface ([#31](https://github.com/Kuenlun/andlock/pull/31))
- *(release)* optimize binaries and broaden ci target matrix ([#30](https://github.com/Kuenlun/andlock/pull/30))
- *(prompts)* persist commit and PR drafts to repo-root files ([#28](https://github.com/Kuenlun/andlock/pull/28))
- *(cli)* split help into terse `-h` and detailed `--help` views ([#26](https://github.com/Kuenlun/andlock/pull/26))
- *(counter)* replace O(p) colex_rank with O(1) prefix/suffix sums ([#24](https://github.com/Kuenlun/andlock/pull/24))
- [**breaking**] drop IDDFS and make DP the sole counting algorithm ([#23](https://github.com/Kuenlun/andlock/pull/23))
- *(counter)* assert DP monotonicity for 4x4 plus 5 free points ([#21](https://github.com/Kuenlun/andlock/pull/21))
- *(counter)* switch DP to layered storage with packed endpoints ([#20](https://github.com/Kuenlun/andlock/pull/20))
- *(prompts)* add commit-builder and changelog-update, harden pr-builder ([#16](https://github.com/Kuenlun/andlock/pull/16))
- *(cli)* apply coloured help output via clap styling API ([#19](https://github.com/Kuenlun/andlock/pull/19))
- *(cli)* align --help text with actual runtime behaviour for dims, free-points, quiet, and preview ([#17](https://github.com/Kuenlun/andlock/pull/17))

## [0.2.1](https://github.com/Kuenlun/andlock/compare/v0.2.0...v0.2.1) - 2026-04-19

### Added

- _(cli)_ Render a terminal grid preview using `●` (nodes) and `★` (free points) before pattern enumeration; silently skipped for 3D+ base grids or grids larger than 40×20 ([#10](https://github.com/Kuenlun/andlock/pull/10))
- Compact JSON export format: numeric arrays stay on one line while objects use multiline indentation, improving readability of `--export-json` output ([#8](https://github.com/Kuenlun/andlock/pull/8))
- `pattern-simplifier` library crate with canonical-form normalization (`translate_to_origin` + `compress_axes`) exposed as a reusable public API; `file --simplify --export-json` outputs canonical-form JSON ([#5](https://github.com/Kuenlun/andlock/pull/5))

### Changed

- Extract algorithmic modules (`dp`, `grid`, `canonicalizer`) into a library crate; `main.rs` is now a thin CLI entry point over `lib.rs` ([#5](https://github.com/Kuenlun/andlock/pull/5))
- `grid --export-json` now always outputs canonical-form JSON (origin-anchored, GCD-compressed axes); no `--simplify` flag needed ([#5](https://github.com/Kuenlun/andlock/pull/5))
- Use closed-form falling-factorial formula `P(n,k) = n!/(n-k)!` for unconstrained grids (zero block matrix), reducing pattern counting from O(n·2ⁿ) to O(n) ([#9](https://github.com/Kuenlun/andlock/pull/9))

### Fixed

- Support zero-sized grid dimensions (e.g. `"0x3"`, `"0x0x1"`): any dimension of 0 yields 0 grid points and only the empty pattern, matching NumPy array semantics ([#6](https://github.com/Kuenlun/andlock/pull/6))

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
