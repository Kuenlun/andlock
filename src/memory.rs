// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Memory-budget policy: keeps the DP layers off swap by clamping
//! `--max-length` against either an explicit `--memory-limit` or 80 % of the
//! OS-reported available RAM.

use andlock::counter::{dp_table_bytes, effective_max_length};

/// 80 % of OS-reported available RAM. The 20 % headroom keeps
/// `Vec::try_reserve_exact` from being satisfied via swap. `None` when the
/// platform reports nothing — better no implicit clamp than clamping every
/// run to length zero on a machine whose RAM sysinfo cannot see.
fn detect_memory_budget() -> Option<u64> {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let available = sys.available_memory();
    (available > 0).then(|| available.saturating_mul(4) / 5)
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
    let Some(budget) = memory_limit.or_else(detect_memory_budget) else {
        return (max_length, None);
    };
    let effective = effective_max_length(n, max_length, budget);
    if effective < max_length {
        (effective, Some((dp_table_bytes(n, max_length), budget)))
    } else {
        (effective, None)
    }
}
