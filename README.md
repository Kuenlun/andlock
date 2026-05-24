# andlock

[![CI](https://github.com/Kuenlun/andlock/actions/workflows/rust.yml/badge.svg?branch=master)](https://github.com/Kuenlun/andlock/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/Kuenlun/andlock/branch/master/graph/badge.svg)](https://codecov.io/gh/Kuenlun/andlock)
[![Crates.io](https://img.shields.io/crates/v/andlock.svg)](https://crates.io/crates/andlock)
[![Docs.rs](https://docs.rs/andlock/badge.svg)](https://docs.rs/andlock)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Count Android-style unlock patterns on n-dimensional grids.

The Android lock screen is a combinatorics problem in disguise: how many distinct paths can you draw on a 3x3 grid under the skip rules? andlock answers it, and the same question on any rectangular lattice or custom point set in any number of dimensions.

## Install

From crates.io:

```bash
cargo install andlock --locked
```

Or grab a prebuilt binary from the [latest release](https://github.com/Kuenlun/andlock/releases/latest) (Linux, macOS, Windows on x86_64 and aarch64).

## Usage

```bash
# Every valid Android pattern on the canonical 3x3 grid.
andlock 3x3 --min-length 4

# Add a free point that sits on no line and never blocks a move (short: -f).
andlock 3x3 --free-points 1

# Load a custom grid from JSON, or pipe one through stdin.
andlock --file grid.json
andlock 3x3 --export-json | andlock --file -

# Group counts with `_` separators and cap peak RAM.
andlock 6x6 --human --memory-limit 2GiB
```

Run `andlock --help` for every option, or `andlock --completions <SHELL>` to print a shell-completion script.

## The rule

A pattern is an ordered sequence of distinct nodes. A move from A to B is legal only when every node lying strictly on the segment AB has already been visited: for any intermediate C = A + t·(B − A) with t ∈ (0, 1), C must appear earlier in the pattern. Moves with no intermediate node are always legal.

The empty pattern and any single node count as valid by convention.

## Cost

Without the visibility rule the count over N nodes would be exactly `floor(e · N!)`. The rule prunes that set, but the result still grows like `O(N!)`. Past 6x6 you will want `--max-length` and `--memory-limit` to keep runs bounded.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option. Any contribution intentionally submitted for inclusion in andlock, as defined in the Apache-2.0 license, shall be dual-licensed as above without any additional terms or conditions.
