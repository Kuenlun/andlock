/*!
andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
Copyright (C) 2026  Juan Luis Leal Contreras (Kuenlun)

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    // Regression test for the bug where an unconstrained run with many
    // points was being clamped against the DP memory budget — even though
    // the closed-form fast path inside `count_patterns_dp` never allocates
    // the DP buffers. With 31 free points the DP estimate balloons to
    // ~143 GiB, but the run itself should be effectively free.
    #[test]
    fn unconstrained_run_skips_memory_clamp_at_max_n() {
        // A 1-byte budget is tighter than even the smallest DP layer, so
        // `effective_max_length(31, 31, 1)` would normally collapse to 0.
        let (effective, clamp) = resolve_memory_budget(31, 31, Some(1), true);
        assert_eq!(effective, 31, "unconstrained run must not clamp max_length");
        assert!(clamp.is_none(), "unconstrained run must not report a clamp");
    }

    // Sanity: with `unconstrained = false` the budget still clamps as before.
    #[test]
    fn constrained_run_still_respects_memory_clamp() {
        let (effective, clamp) = resolve_memory_budget(31, 31, Some(1), false);
        assert!(effective < 31, "tight budget must clamp constrained run");
        assert!(clamp.is_some(), "clamp metadata must be reported");
    }
}
