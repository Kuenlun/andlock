# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/Kuenlun/andlock/releases/tag/v0.1.0) - 2026-04-17

### Added

- implement n-dimensional unlock pattern counter with bitmask DP ([#1](https://github.com/Kuenlun/andlock/pull/1))
- accept grid definition as JSON input via `serde` deserialization
- validate input enforcing a 25-point ceiling and per-point dimension consistency
- add test suite covering the canonical 3×3 Android grid, n-dimensional collinearity, and edge cases

### Fixed

- add execute permission to `check-license-headers.sh`

### Other

- update .gitignore to exclude .vscode and .claude directories
- add GitHub Actions workflows, pre-commit hook, and license header enforcement
- add package metadata and enforce strict Rust/Clippy linting and nightly toolchain
- update README to provide detailed explanation of program concept, rules, and computational complexity
- initialize Rust project with Cargo configuration and main file
- initial commit
