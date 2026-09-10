// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Counting-table budget from an explicit limit or available memory.

/// Resolve a table budget, leaving headroom for the input, runtime and OS.
/// If available memory cannot be detected, use a conservative 512 MiB budget.
pub fn resolve_memory_budget(memory_limit: Option<u64>) -> u64 {
    memory_limit.unwrap_or_else(|| {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        let available = sys.available_memory();
        if available == 0 {
            512 << 20
        } else {
            available.saturating_mul(4) / 5
        }
    })
}
