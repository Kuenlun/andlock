# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/Kuenlun/andlock/compare/v0.2.1...v0.3.0) - 2026-05-02

### Added

- Add `aarch64-unknown-linux-musl` and `aarch64-pc-windows-msvc` to the prebuilt release-binary matrix, broadening Linux ARM and Windows ARM coverage alongside the existing `x86_64-pc-windows-msvc`, `x86_64-apple-darwin`, and `aarch64-apple-darwin` targets ([#30](https://github.com/Kuenlun/andlock/pull/30))
- Add `--human` flag to the `grid` and `file` subcommands for `_`-grouped digits (every three digits) on the per-length, `Total`, and `Points` rows, matching Rust integer-literal syntax for locale-neutral output ([#27](https://github.com/Kuenlun/andlock/pull/27))
- Add `--memory-limit SIZE` to the `grid` and `file` subcommands, accepting case-insensitive K/M/G/T binary suffixes (e.g., `512M`, `2GiB`). When omitted, the budget defaults to 80% of available RAM detected via `sysinfo`, and `--max-length` is clamped to the largest cap that fits the resulting DP allocation, with a stderr warning listing the skipped lengths and the needed versus available bytes ([#25](https://github.com/Kuenlun/andlock/pull/25))
- Apply coloured `--help` output via clap's styling API, with bold-underline green section headers and usage, bold cyan flag literals, and cyan placeholders, automatically degrading on restricted terminals ([#19](https://github.com/Kuenlun/andlock/pull/19))
- Handle `Ctrl+C` cleanly via a shared `MultiProgress` draw target, restoring the stderr cursor and exiting with status 130 on Unix or 1 elsewhere to avoid Cargo's red `STATUS_CONTROL_C_EXIT` banner ([#15](https://github.com/Kuenlun/andlock/pull/15))
- Stream per-length counts live as they are finalized during DP traversal, replacing the batch end-of-run output with progressive feedback that arrives the moment each popcount layer completes ([#12](https://github.com/Kuenlun/andlock/pull/12))

### Changed

- **BREAKING:** Change `count_patterns_dp` to accept a pre-allocated `DpScratch` buffer as its first parameter, with all heap allocations hoisted into `DpScratch::allocate(n, blocks, max_length) -> Result<DpScratch, TryReserveError>` so callers surface allocation failures up front and the DP body itself stays infallible ([#32](https://github.com/Kuenlun/andlock/pull/32))
- Tighten the release profile with `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = "debuginfo"`, and `debug = false` for smaller, faster published binaries, plus per-target `RUSTFLAGS` setting `target-cpu=x86-64-v3` on x86_64 targets and `target-cpu=apple-m1` on `aarch64-apple-darwin` ([#30](https://github.com/Kuenlun/andlock/pull/30))
- Switch the Linux release binary from `x86_64-unknown-linux-gnu` to `x86_64-unknown-linux-musl` to ship a portable, statically linked artifact ([#30](https://github.com/Kuenlun/andlock/pull/30))
- Replace the per-row `Length N:` prefix with a `Len Count` two-column header rendered lazily on the first matching row, add a `Points N` row to the run summary so the grid size remains visible when `--max-length` truncates the per-length list, and right-align the per-length, `Total`, and `Points` rows to a single shared column edge sized from the widest formatted count ([#27](https://github.com/Kuenlun/andlock/pull/27))
- Split `--help` into a terse one-line `-h` view and a detailed `--help` view via clap's doc-comment two-tier convention, group flags under `Pattern length`, `Resources`, and `Output` headings, and move curated examples behind `--help` only via `after_long_help` ([#26](https://github.com/Kuenlun/andlock/pull/26))
- Replace the O(p) `colex_rank` walk inside the DP transition with O(1) prefix and suffix sums of the `BINOM` table, computing each destination-layer index as a constant-time three-term lookup precomputed once per source mask ([#24](https://github.com/Kuenlun/andlock/pull/24))
- Switch the DP table from a flat `2ⁿ × n × u128` layout to two adjacent popcount layers with packed endpoints (indexing each popcount-`p` mask into `p` `u128` slots via `popcount(mask & (bit - 1))`), cutting peak DP memory by roughly 6x without changing asymptotic cost so larger grids fit on modest hardware ([#20](https://github.com/Kuenlun/andlock/pull/20))
- **BREAKING:** Rename the public library module `andlock::dp` to `andlock::counter`, with all public DP types and functions now reachable under the new path ([#14](https://github.com/Kuenlun/andlock/pull/14))
- Raise the maximum supported point count from 25 to 31, with the `u32` visited-set bitmask now the limiting factor instead of the DP table size ([#14](https://github.com/Kuenlun/andlock/pull/14))
- **BREAKING:** Replace the `impl Fn()` mask-tick callback in `count_patterns_dp` with `F: FnMut(DpEvent)`, where `DpEvent` is a new public enum with `Mask` and `LengthDone { length, count }` variants threading both progress ticks and finalized per-length results through a single closure ([#12](https://github.com/Kuenlun/andlock/pull/12))

### Fixed

- Skip the memory clamp when the block matrix is fully unconstrained (all-zero blocks), since `count_patterns_dp` takes a closed-form fast path that allocates no DP buffers. Cases like `andlock grid 0 -f 31` no longer truncate `--max-length` against a phantom 143 GiB estimate ([#29](https://github.com/Kuenlun/andlock/pull/29))
- Warn on stderr instead of erroring when `--min-length` or `--max-length` is combined with `--export-json`, so scripts that pass a uniform flag set across subcommands no longer abort. The warning is suppressed under `--quiet`, and the JSON payload on stdout is unchanged ([#18](https://github.com/Kuenlun/andlock/pull/18))
- Correct the `--help` text on both subcommands so it matches actual runtime behaviour: `parse_dims` accepts both `x` and `X` as separators with no surrounding whitespace, each `--free-points` adds an extra non-collinear axis, `--quiet` also suppresses the ASCII grid preview, and the preview itself is only rendered for 1D/2D grids fitting roughly 40x20 cells ([#17](https://github.com/Kuenlun/andlock/pull/17))

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
