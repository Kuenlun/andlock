# Performance checks

Build both revisions with the same Rust toolchain, release profile, and native CPU flags. Keep the executables under distinct names. Run measurements on the same idle Linux host, without concurrent builds or benchmarks.

```bash
cargo build --release --locked
cp target/release/andlock /tmp/andlock-baseline
# Switch to the candidate revision.
cargo build --release --locked
cp target/release/andlock /tmp/andlock-candidate
python3 benches/compare.py /tmp/andlock-baseline /tmp/andlock-candidate > comparison.json
```

The Python standard library provides Linux `wait4` peak RSS without adding a crate or unsafe Rust. Four fixed workloads include rectangles and free nodes, with compact table allocations of at least 28 MiB. A run is invalid if its peak RSS does not exceed the Python launcher peak. Each build gets one warmup and five measured runs per case, alternating order. The JSON report records executable hashes, sizes, raw samples, median elapsed time, and median peak RSS. Each run has a 30-second timeout, configurable with `--timeout-seconds`; a timeout kills the child’s process group and reaps the child. Counts must match byte for byte, every requested length must be present, and stderr must be empty.

Exit status is 1 for a regression, 2 for an invalid run, and 0 otherwise. Defaults allow 5% elapsed-time variation and no RSS or binary growth. Explicit `--max-time-growth-percent`, `--max-rss-growth-kib`, and `--max-binary-growth-bytes` limits are recorded in the report. Document the reason for intentional growth before setting a larger limit. RSS covers the whole process; allocator and runtime costs are included.

For isolated counting throughput, Criterion reuses scratch storage so allocation and teardown are outside the measured operation:

```bash
cargo bench --bench dp -- --save-baseline before
# Run the same benchmark harness against the candidate revision.
cargo bench --bench dp -- --baseline before
```

Fixed maximum lengths keep the work identical across revisions. The suite uses 20 equally sized samples per case, a 0.5-second warmup, and a 4-second measurement target. It covers storage-width transitions, terminal layers, free nodes, and all visited-mask widths. Keep Criterion baselines from different harnesses separate.

Run the comparison helper tests with `python3 -B benches/test_compare.py`.
