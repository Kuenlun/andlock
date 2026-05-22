// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Memory-budget policy: keeps the DP layers off swap by clamping
//! `--max-length` against either an explicit `--memory-limit` or 80 % of the
//! OS-reported available RAM.

use andlock::counter::{dp_table_bytes, effective_max_length};

/// 80 % of OS-reported available RAM. The 20 % headroom keeps
/// `Vec::try_reserve_exact` from succeeding by paging into swap.
fn detect_memory_budget() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.available_memory().saturating_mul(4) / 5
}

/// Returns `(effective_max_length, Some((needed, budget)))` when the run is
/// clamped, or `(max_length, None)` when it fits. `unconstrained` skips the
/// clamp because the closed-form path allocates no DP buffer.
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
        (effective, Some((dp_table_bytes(n, max_length), budget)))
    } else {
        (effective, None)
    }
}
