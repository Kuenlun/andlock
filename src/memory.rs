// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Memory-budget policy: probes OS-reported available RAM and clamps
//! `max_length` so the DP buffers fit. The `--memory-limit` parser lives
//! in `cli` next to the other clap value parsers; this module is only
//! the runtime budget logic.

use andlock::counter::{dp_table_bytes, effective_max_length};

/// One-shot probe of OS-reported available RAM, scaled down to leave
/// headroom for the OS and the rest of the process. Used as the implicit
/// `--memory-limit` when the flag is not passed: `Vec::try_reserve_exact`
/// only fails when virtual address space is exhausted (which on Windows
/// includes the pagefile), so we cannot rely on the allocator alone to
/// keep the run inside physical RAM. No polling — sampled once.
fn detect_memory_budget() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    // Reserve ~20% as headroom (kernel page cache, other processes, the
    // rest of this process). The factor is conservative on purpose:
    // overshooting the budget is the failure mode we are guarding
    // against.
    sys.available_memory().saturating_mul(4) / 5
}

/// Resolves the effective `max_length` cap against the active memory
/// budget. The budget comes from `--memory-limit` when present, otherwise
/// from a one-shot probe of OS-reported available RAM (see
/// [`detect_memory_budget`]).
///
/// `unconstrained` short-circuits the clamp: an all-zero block matrix
/// triggers the closed-form fast path inside `count_patterns_dp`, which
/// allocates no DP buffers, so no memory budget can ever justify
/// truncating the run.
///
/// Returns `(effective, Some((needed_bytes, budget_bytes)))` when the cap
/// is clamped, or `(max_length, None)` when it fits.
pub fn resolve_memory_budget(
    n: usize,
    max_length: usize,
    memory_limit: Option<u64>,
    unconstrained: bool,
) -> (usize, Option<(u64, u64)>) {
    if unconstrained {
        return (max_length, None);
    }
    let budget = memory_limit.unwrap_or_else(detect_memory_budget);
    let effective = effective_max_length(n, max_length, budget);
    if effective < max_length {
        let needed = dp_table_bytes(n, max_length);
        (effective, Some((needed, budget)))
    } else {
        (effective, None)
    }
}
