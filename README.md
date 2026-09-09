# andlock

[![CI](https://github.com/Kuenlun/andlock/actions/workflows/rust.yml/badge.svg?branch=master)](https://github.com/Kuenlun/andlock/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/Kuenlun/andlock/branch/master/graph/badge.svg)](https://codecov.io/gh/Kuenlun/andlock)
[![Crates.io](https://img.shields.io/crates/v/andlock.svg)](https://crates.io/crates/andlock)
[![Docs.rs](https://docs.rs/andlock/badge.svg)](https://docs.rs/andlock)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Count Android-style unlock patterns on n-dimensional grids.

The Android lock screen is a combinatorics problem in disguise: how many distinct paths can you draw on a 3x3 grid under the skip rules? andlock answers it — and the same question on any rectangular lattice or custom point set, in any number of dimensions, exactly.

## Install

From crates.io:

```bash
cargo install andlock --locked
```

Or grab a prebuilt binary from the [latest release](https://github.com/Kuenlun/andlock/releases/latest) (Linux, macOS, Windows on x86_64 and aarch64).

## Usage

```bash
# The classic question: patterns the Android lock screen accepts.
andlock 3x3 --min-length 4

# Any rectangular lattice, in any number of dimensions.
andlock 4x4
andlock 3x3x3
andlock 10              # ten points on a line

# Free points sit on no line and never block a move (short: -f).
andlock 3x3 --free-points 1

# Custom point sets from JSON ('-' reads stdin).
andlock --file grid.json
andlock 3x3 --export-json | andlock --file -

# Keep large runs bounded.
andlock 6x6 --max-length 8 --memory-limit 2GiB --human
```

`andlock 3x3 --min-length 4` previews the grid, then prints one row per pattern length:

```text
● ● ●
● ● ●
● ● ●

  Len     Count
    4      1624
    5      7152
    6     26016
    7     72912
    8    140704
    9    140704
───────────────
  Total  389112
  Points      9
```

Counts go to stdout; the preview, progress, and warnings go to stderr, so pipes stay clean. Run `andlock --help` for every option or `andlock --completions <SHELL>` for tab completion.

`andlock 3x3 --min-length 4 --json` emits one count report containing `grid`, `requested_range`, `completed_range`, `counts`, `total`, and `status`. Counts and totals are decimal strings, preserving full `u128` precision. Each count has `length` and `count` fields. Ranges have inclusive `min_length` and `max_length` fields.

Status is `complete`, `interrupted`, `count_overflow`, `total_overflow`, or `memory_limit`. Partial reports retain finalized counts and their subtotal. `completed_range` and `total` are `null` when no selected length finished. A total that overflows is also `null`. `--json` cannot be combined with `--human` or `--export-json`, which exports only the reusable grid definition.

## The rule

A pattern is an ordered sequence of distinct nodes. A move from A to B is legal only when every node lying strictly on the segment AB has already been visited: for any intermediate C = A + t·(B − A) with t ∈ (0, 1), C must appear earlier in the pattern. Moves with no intermediate node are always legal.

The empty pattern and any single node count as valid by convention.

## Grid JSON

`--file` loads (and `--export-json` emits) this shape:

```json
{
  "dimensions": 2,
  "points": [
    [0, 0],
    [2, 0],
    [4, 0]
  ],
  "free_points": 1
}
```

`points` holds integer coordinates with magnitude up to 2³⁰ − 1; `free_points` (optional, default 0) adds isolated nodes. At most 127 nodes in total. `--simplify` rewrites a loaded grid into canonical form — the points above become `[[-1, 0], [0, 0], [1, 0]]` — which never changes any count.

## Cost

Without the visibility rule the count over N nodes would be exactly `floor(e · N!)`. The rule prunes that set, but the result still grows like `O(N!)`. Past 6x6 you will want `--max-length` and `--memory-limit` to keep runs bounded; by default andlock caps its tables at 80 % of available RAM and reports the largest `--max-length` that fits.

Every count is exact: arithmetic runs in `u128` with overflow detection, and a run stops at the last length that fits rather than ever printing a wrapped value.

## How it counts

A layered bitmask dynamic program walks visited-sets grouped by population count: Gosper's hack enumerates each layer and colex ranking addresses states inside the two live layers, so memory tracks the largest binomial layers instead of a `2^N` table. The visited mask monomorphises to `u32`/`u64`/`u128`, whichever is the narrowest fit for the grid.

## Exit codes

`0` success · `1` runtime error · `2` usage error · `130` interrupted (Ctrl+C prints the partial table first).

An incomplete requested length range or a selected-range total that exceeds `u128` exits with `1`, preserving all finalised per-length counts. `--quiet` keeps warnings and errors visible.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option. Any contribution intentionally submitted for inclusion in andlock, as defined in the Apache-2.0 license, shall be dual-licensed as above without any additional terms or conditions.
