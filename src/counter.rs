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

use crate::grid::MAX_POINTS;

/// Progress event emitted by [`count_patterns_dp`] during execution.
///
/// A streaming caller uses `Mask` to advance a per-mask progress bar and
/// `LengthDone` to print the count of patterns of a given length as soon as it
/// is fully known — before the whole DP finishes.
pub enum DpEvent {
    /// One outer-loop mask has been processed (fired `2ⁿ − 1` times during the
    /// constrained DP; not fired at all on the unconstrained fast path).
    Mask,
    /// `counts[length]` has received its last contribution and is now final.
    LengthDone { length: usize, count: u128 },
}

/// Progress / streaming event emitted by [`count_patterns_dfs`] during
/// execution.
///
/// Unlike [`DpEvent`], per-length counts cannot be finalized mid-run: every
/// `counts[k]` keeps accumulating contributions from new starting subtrees
/// until the very last one completes. `Progress` therefore exposes the
/// *running* tally so callers can render live partial totals, and
/// `LengthDone` is emitted once per length at the very end (mirroring the DP
/// event so both counters share the same output plumbing).
pub enum DfsEvent<'a> {
    /// A top-level `(start, second)` pair has been fully explored. Fires
    /// exactly `n * (n − 1)` times during the constrained search; not fired
    /// at all on the unconstrained fast path. `counts` is the current partial
    /// tally — valid only for the duration of the callback.
    Progress { counts: &'a [u128] },
    /// Final count for `length`, emitted once per entry of the returned
    /// vector after all DFS work is done.
    LengthDone { length: usize, count: u128 },
}

/// Number of [`DfsEvent::Progress`] ticks [`count_patterns_dfs`] fires during
/// a constrained run — `n · (n − 1)`, clamped against overflow. Callers use
/// it to size a progress bar.
#[must_use]
pub const fn dfs_progress_ticks(n: usize) -> u64 {
    let n = n as u64;
    n.saturating_mul(n.saturating_sub(1))
}

/// Which counting algorithm to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    /// Bottom-up bitmask DP — fast but allocates a `2ⁿ × n × 16`-byte table.
    Dp,
    /// Explicit DFS — slow but uses only `O(n)` memory.
    Dfs,
}

/// Returns the number of bytes the DP table would occupy for `n` nodes.
///
/// Formula: `2ⁿ × n × 16` (each of the `2ⁿ × n` entries is a `u128`).
/// Uses saturating arithmetic so callers can safely pass any `n`.
#[must_use]
pub fn dp_table_bytes(n: usize) -> u64 {
    (1u64 << n.min(63))
        .saturating_mul(n as u64)
        .saturating_mul(16)
}

/// Returns [`Algorithm::Dp`] if the DP table fits within `memory_budget`
/// bytes, otherwise [`Algorithm::Dfs`].
#[must_use]
pub fn choose_algorithm(n: usize, memory_budget: u64) -> Algorithm {
    if dp_table_bytes(n) <= memory_budget {
        Algorithm::Dp
    } else {
        Algorithm::Dfs
    }
}

/// Counts every valid pattern, routing to DP or DFS based on `algorithm`.
///
/// The `on_tick` callback is invoked once per outer-loop step: once per
/// bitmask for DP (up to `2ⁿ − 1` calls) or once per `(start, second)`
/// top-level pair for DFS (up to `n · (n − 1)` calls). Per-length streaming
/// is not available through this interface; use [`count_patterns_dp`] or
/// [`count_patterns_dfs`] directly to observe [`DpEvent::LengthDone`] /
/// [`DfsEvent::LengthDone`].
///
/// # Panics
/// Panics under the same conditions as the underlying algorithm.
pub fn count_patterns(
    n: usize,
    blocks: &[u32],
    max_length: usize,
    algorithm: Algorithm,
    on_tick: impl Fn(),
) -> Vec<u128> {
    match algorithm {
        Algorithm::Dp => count_patterns_dp(n, blocks, max_length, |event| {
            if matches!(event, DpEvent::Mask) {
                on_tick();
            }
        }),
        Algorithm::Dfs => count_patterns_dfs(n, blocks, max_length, |event| {
            if matches!(event, DfsEvent::Progress { .. }) {
                on_tick();
            }
        }),
    }
}

/// Next bitmask with the same popcount as `x` (Gosper's hack).
///
/// Used to enumerate masks in popcount-ascending order so that each popcount
/// layer can emit a [`DpEvent::LengthDone`] as soon as it completes.
const fn gosper_next(x: u32) -> u32 {
    let c = x & x.wrapping_neg();
    let r = x.wrapping_add(c);
    (((r ^ x) >> 2) / c) | r
}

/// Computes pattern counts when there are no visibility constraints.
///
/// When every `blocks[i * n + j] == 0` every move is unconditionally legal, so
/// the number of valid patterns of length k is the falling factorial
/// `P(n, k) = n! / (n-k)! = n × (n-1) × … × (n-k+1)`.
fn count_unconstrained(n: usize, max_length: usize) -> Vec<u128> {
    let mut counts = vec![0u128; max_length + 1];
    counts[0] = 1;
    let mut perm: u128 = 1;
    for (k, slot) in counts.iter_mut().enumerate().skip(1) {
        perm *= (n - k + 1) as u128;
        *slot = perm;
    }
    counts
}

/// Counts every valid pattern via bottom-up bitmask dynamic programming.
///
/// `blocks[i * n + j]` must hold the bitmask of nodes that must already be
/// visited before the move `i → j` is legal (see [`crate::grid::compute_blocks`]).
///
/// `max_length` bounds the pattern lengths considered: any prefix whose length
/// reaches `max_length` is not extended, so the exponential inner work for
/// longer prefixes is never performed.
///
/// Returns a `Vec<u128>` of length `max_length + 1` where `counts[k]` is the
/// number of valid patterns of exactly `k` nodes. `counts[0] = 1` (the empty
/// pattern).
///
/// `u128` is used because for n ≥ 21 the total count can exceed `u64::MAX`
/// (e.g. an unrestricted 21-node graph can produce 21! ≈ 5.1 × 10¹⁹ patterns,
/// while `u64::MAX` ≈ 1.8 × 10¹⁹).
///
/// # Complexity
/// With `L = max_length`, extension work is bounded by the prefixes of length
/// `< L`, so the runtime shrinks from the full `O(N² · 2ᴺ)` to
/// `O(N² · Σ_{k<L} C(N, k))` when `L < N`.
///
/// # Panics
/// Panics if `n > MAX_POINTS`, `blocks.len() != n * n`, or `max_length > n`.
pub fn count_patterns_dp<F: FnMut(DpEvent)>(
    n: usize,
    blocks: &[u32],
    max_length: usize,
    mut on_event: F,
) -> Vec<u128> {
    assert!(n <= MAX_POINTS, "N={n} exceeds the maximum of {MAX_POINTS}");
    assert_eq!(blocks.len(), n * n, "blocks matrix must be n × n");
    assert!(
        max_length <= n,
        "max_length={max_length} must not exceed n={n}"
    );

    if blocks.iter().all(|&b| b == 0) {
        let counts = count_unconstrained(n, max_length);
        for (k, &c) in counts.iter().enumerate() {
            on_event(DpEvent::LengthDone {
                length: k,
                count: c,
            });
        }
        return counts;
    }

    let mut counts = vec![0u128; max_length + 1];
    counts[0] = 1;
    on_event(DpEvent::LengthDone {
        length: 0,
        count: 1,
    });
    if n == 0 || max_length == 0 {
        return counts;
    }

    let num_masks: usize = 1 << n;
    let full_mask: u32 = (1u32 << n) - 1;

    // dp[mask * n + node] = number of valid orderings that visit exactly the
    // set encoded by `mask` and end at `node`. Mask-major layout keeps the
    // inner scan over endpoints contiguous in memory.
    //
    // Per-state values can reach (len-1)! at the full mask, which exceeds
    // u64::MAX starting around n=22 — hence u128 (same rationale as `counts`).
    let mut dp = vec![0u128; num_masks * n];

    for v in 0..n {
        dp[(1usize << v) * n + v] = 1;
    }
    counts[1] = n as u128;
    on_event(DpEvent::LengthDone {
        length: 1,
        count: counts[1],
    });

    // Enumerate masks grouped by popcount (ascending). Every proper subset of
    // a popcount-p mask has popcount < p, so subset states are still
    // guaranteed to be populated before use. Grouping by popcount lets us
    // report `counts[p+1]` the moment the last popcount-p mask is processed,
    // enabling streaming output without waiting for the full run.
    //
    // Masks whose length already equals `max_length` cannot be extended into
    // a counted pattern, so their inner loops are skipped — but they are
    // still ticked so the caller's mask-granular progress bar reaches 100%.
    for p in 1..=n {
        let mut mask: u32 = (1u32 << p) - 1;
        let last: u32 = mask << (n - p);
        loop {
            on_event(DpEvent::Mask);
            if p < max_length {
                let base = (mask as usize) * n;

                let mut visited = mask;
                while visited != 0 {
                    let end_bit = visited & visited.wrapping_neg();
                    visited ^= end_bit;
                    let end = end_bit.trailing_zeros() as usize;

                    let ways = dp[base + end];
                    if ways == 0 {
                        continue;
                    }

                    let mut free = !mask & full_mask;
                    while free != 0 {
                        let next_bit = free & free.wrapping_neg();
                        free ^= next_bit;
                        let next = next_bit.trailing_zeros() as usize;

                        let blockers = blocks[end * n + next];
                        if mask & blockers == blockers {
                            counts[p + 1] += ways;
                            // Writes into the terminal layer would never be
                            // read, since masks of length `max_length` are
                            // skipped by the outer `p < max_length` guard.
                            if p + 1 < max_length {
                                let new_mask = (mask | next_bit) as usize;
                                dp[new_mask * n + next] += ways;
                            }
                        }
                    }
                }
            }
            if mask == last {
                break;
            }
            mask = gosper_next(mask);
        }
        // All contributions to counts[p+1] came from popcount-p masks, so the
        // value is now final.
        if p < max_length {
            on_event(DpEvent::LengthDone {
                length: p + 1,
                count: counts[p + 1],
            });
        }
    }

    counts
}

/// Counts every valid pattern via depth-first search with `O(N)` memory.
///
/// This is the memory-light counterpart to [`count_patterns_dp`]. Instead of
/// materialising a `2ⁿ × N` DP table, the search walks every valid prefix
/// explicitly, keeping only the current visited bitmask and the recursion
/// stack (depth bounded by `max_length ≤ N`).
///
/// Semantically the two counters are interchangeable: given the same inputs
/// they must produce identical `Vec<u128>` outputs (the tests in this module
/// enforce that equivalence). The tradeoff is runtime — without the DP's
/// memoisation each valid pattern is re-expanded along its own path, so time
/// scales with the number of valid prefixes rather than with `N² · 2ᴺ`. For
/// small-to-moderate `N` the DP is faster; for large `N` (roughly `N ≥ 23`)
/// the DP table no longer fits in memory and this DFS is the only option.
///
/// `blocks[i * n + j]` must hold the bitmask of nodes that must already be
/// visited before the move `i → j` is legal (see [`crate::grid::compute_blocks`]).
///
/// `max_length` bounds the pattern lengths considered: once a prefix reaches
/// `max_length` the search backtracks instead of extending.
///
/// Returns a `Vec<u128>` of length `max_length + 1` where `counts[k]` is the
/// number of valid patterns of exactly `k` nodes. `counts[0] = 1` (the empty
/// pattern).
///
/// `u128` is used for the same reason as in [`count_patterns_dp`]: for
/// `n ≥ 21` the total count can exceed `u64::MAX`.
///
/// `on_event` is invoked with [`DfsEvent::Progress`] once per top-level
/// `(start, second)` pair — `n · (n − 1)` times during the constrained
/// search, zero times on the unconstrained fast path — and with
/// [`DfsEvent::LengthDone`] once per length at the very end. The
/// fine-grained progress granularity is deliberate: ticking only once per
/// starting node (as an earlier version did) left the progress bar stuck
/// near `1/n` for most of the run, because the first start's subtree
/// dominates the wall-clock cost.
///
/// Per-length results cannot stream mid-computation the way [`DpEvent`]
/// does: every `counts[k]` keeps accumulating until the final starting
/// subtree completes. `Progress` exposes the running tally so callers can
/// still render a live partial total, and all [`DfsEvent::LengthDone`]
/// events fire together at the end — matching DP's event shape so output
/// plumbing can be shared.
///
/// # Memory
/// `O(N + max_length)` — one `u128` per length slot plus a recursion stack
/// whose depth never exceeds `max_length`. No exponential table is allocated.
///
/// # Complexity
/// Proportional to the number of valid prefixes of length `≤ max_length`,
/// i.e. `O(Σ_{k≤L} (valid patterns of length k))`. In the worst case (no
/// blockers) this degenerates to `Σ_{k≤L} P(n, k)`, but that path is served
/// by the closed-form fast path and never reaches the DFS.
///
/// # Panics
/// Panics if `n > MAX_POINTS`, `blocks.len() != n * n`, or `max_length > n`.
/// Like [`count_patterns_dp`], this function uses a `u32` bitmask for the
/// visited set, which caps `n` at 32; `MAX_POINTS` is the tighter system
/// ceiling shared by both counters.
pub fn count_patterns_dfs<F: FnMut(DfsEvent<'_>)>(
    n: usize,
    blocks: &[u32],
    max_length: usize,
    mut on_event: F,
) -> Vec<u128> {
    assert!(
        n <= MAX_POINTS,
        "N={n} exceeds the supported maximum of {MAX_POINTS}"
    );
    assert_eq!(blocks.len(), n * n, "blocks matrix must be n × n");
    assert!(
        max_length <= n,
        "max_length={max_length} must not exceed n={n}"
    );

    if blocks.iter().all(|&b| b == 0) {
        let counts = count_unconstrained(n, max_length);
        for (k, &c) in counts.iter().enumerate() {
            on_event(DfsEvent::LengthDone {
                length: k,
                count: c,
            });
        }
        return counts;
    }

    let mut counts = vec![0u128; max_length + 1];
    counts[0] = 1;
    if n != 0 && max_length >= 1 {
        counts[1] = n as u128;
    }

    if n >= 1 && max_length >= 2 {
        let full_mask: u32 = (1u32 << n) - 1;
        let mut ctx = DfsCtx {
            n,
            blocks,
            full_mask,
            max_length,
            counts: &mut counts,
        };
        // Top-level loop is split into (start, second) so that progress
        // ticks fire `n · (n − 1)` times instead of `n`. The extra
        // granularity is what lets the CLI progress bar advance visibly on
        // large instances where each starting subtree takes hours or more.
        for start in 0..n {
            let mask = 1u32 << start;
            let row = start * n;
            for second in 0..n {
                if second == start {
                    continue;
                }
                let next_bit = 1u32 << second;
                let blockers = ctx.blocks[row + second];
                if mask & blockers == blockers {
                    ctx.counts[2] += 1;
                    if 2 < ctx.max_length {
                        ctx.extend(mask | next_bit, second, 2);
                    }
                }
                on_event(DfsEvent::Progress {
                    counts: &*ctx.counts,
                });
            }
        }
    }

    for (k, &c) in counts.iter().enumerate() {
        on_event(DfsEvent::LengthDone {
            length: k,
            count: c,
        });
    }
    counts
}

// State threaded through every recursive frame of the DFS.
struct DfsCtx<'a> {
    n: usize,
    blocks: &'a [u32],
    full_mask: u32,
    max_length: usize,
    counts: &'a mut [u128],
}

impl DfsCtx<'_> {
    // Extends the current prefix `(mask, end)` of length `len` by every
    // legal next node and accumulates the resulting pattern lengths. The
    // caller guarantees `len < max_length`, so `counts[len + 1]` is always a
    // valid slot.
    fn extend(&mut self, mask: u32, end: usize, len: usize) {
        let row = end * self.n;
        let mut free = !mask & self.full_mask;
        while free != 0 {
            let next_bit = free & free.wrapping_neg();
            free ^= next_bit;
            let next = next_bit.trailing_zeros() as usize;

            let blockers = self.blocks[row + next];
            if mask & blockers == blockers {
                self.counts[len + 1] += 1;
                if len + 1 < self.max_length {
                    self.extend(mask | next_bit, next, len + 1);
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::grid::{GridDefinition, build_grid_definition, compute_blocks};

    fn grid(dimensions: usize, points: Vec<Vec<i32>>) -> GridDefinition {
        GridDefinition { dimensions, points }
    }

    // Runs both counters, asserts they agree, and returns the result.
    // Every test that checks output values goes through this helper so that
    // both algorithms are verified in a single pass.
    fn count(n: usize, blocks: &[u32], max_length: usize) -> Vec<u128> {
        let dp = count_patterns_dp(n, blocks, max_length, |_| {});
        let dfs = count_patterns_dfs(n, blocks, max_length, |_| {});
        assert_eq!(
            dp, dfs,
            "DP and DFS counts diverge for n={n}, max_length={max_length}"
        );
        dp
    }

    #[test]
    fn android_3x3_matches_known_pattern_counts() {
        #[rustfmt::skip]
        let g = grid(
            2,
            vec![
                vec![0, 0], vec![1, 0], vec![2, 0],
                vec![0, 1], vec![1, 1], vec![2, 1],
                vec![0, 2], vec![1, 2], vec![2, 2],
            ],
        );
        g.validate().unwrap();
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let counts = count(n, &blocks, n);

        assert_eq!(counts[0], 1);
        assert_eq!(counts[1], 9);
        assert_eq!(counts[2], 56);
        assert_eq!(counts[4], 1_624);
        assert_eq!(counts[5], 7_152);
        assert_eq!(counts[6], 26_016);
        assert_eq!(counts[7], 72_912);
        assert_eq!(counts[8], 140_704);
        assert_eq!(counts[9], 140_704);
        assert_eq!(counts[4..=9].iter().sum::<u128>(), 389_112);
    }

    #[test]
    fn no_three_collinear_collapses_to_permutations() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![1, 1], vec![0, 1]]);
        let blocks = compute_blocks(&g);
        assert!(blocks.iter().all(|&b| b == 0));
        let n = g.points.len();
        assert_eq!(count(n, &blocks, n), vec![1, 4, 12, 24, 24]);
    }

    #[test]
    fn blocker_becomes_transparent_once_visited() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![2, 0]]);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let counts = count(n, &blocks, n);

        assert_eq!(counts[1], 3);
        assert_eq!(counts[2], 4);
        // A→B→C, B→A→C, B→C→A, C→B→A survive.
        assert_eq!(counts[3], 4);
    }

    #[test]
    fn edge_cases_zero_and_one_point() {
        let empty = grid(2, vec![]);
        empty.validate().unwrap();
        let blocks = compute_blocks(&empty);
        assert!(blocks.is_empty());
        assert_eq!(count(0, &blocks, 0), vec![1]);

        let single = grid(2, vec![vec![7, 7]]);
        single.validate().unwrap();
        let blocks = compute_blocks(&single);
        assert_eq!(blocks, vec![0]);
        assert_eq!(count(1, &blocks, 1), vec![1, 1]);
    }

    #[test]
    fn generated_3x3_matches_known_pattern_counts() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let counts = count(n, &blocks, n);
        assert_eq!(counts[4..=9].iter().sum::<u128>(), 389_112);
    }

    #[test]
    fn max_length_truncates_counts_to_prefix_of_full_run() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let full = count(n, &blocks, n);
        for cap in 0..=n {
            let capped = count(n, &blocks, cap);
            assert_eq!(capped.len(), cap + 1, "unexpected length for cap={cap}");
            assert_eq!(
                capped.as_slice(),
                &full[..=cap],
                "truncated counts disagree with full run at cap={cap}"
            );
        }
    }

    // Regression test: with n=21 the sum of patterns exceeds u64::MAX (≈1.84×10¹⁹)
    // because 21! ≈ 5.1×10¹⁹. Before the fix, `counts` used u64 and panicked with
    // "attempt to add with overflow" on `grid 4x4 -f 5`.
    // This test requires ~700 MB of DP table.
    #[test]
    #[ignore = "allocates ~700 MB — run manually with: cargo test -- --ignored"]
    fn count_4x4_plus_5_free_does_not_overflow() {
        let g = build_grid_definition(&[4, 4], 5);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        assert_eq!(n, 21);
        let counts = count_patterns_dp(n, &blocks, n, |_| {});
        // In a zero-blocker graph, counts[n] = n!; free points guarantee many
        // paths exceed u64::MAX, so u128 is necessary.
        assert!(
            counts[n] > u128::from(u64::MAX),
            "expected counts[21] to exceed u64::MAX but got {}",
            counts[n]
        );
    }

    // Regression test for silent overflow in the DP table itself. At n=24, the
    // per-endpoint `ways` stored at mask length 23 reaches ≈21! ≈ 5.1×10¹⁹,
    // which wraps in u64 and produces counts[24] < counts[23] — a monotonicity
    // violation that is provably impossible (every full pattern of length 24
    // extends some length-23 prefix). We assert on every suffix length to lock
    // the invariant in place.
    //
    // This allocates ~6.4 GB for the DP table, so it is ignored by default.
    #[test]
    #[ignore = "allocates ~6.4 GB — run manually with: cargo test -- --ignored"]
    fn count_4x4_plus_8_free_is_monotonic_in_length() {
        let g = build_grid_definition(&[4, 4], 8);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        assert_eq!(n, 24);
        let counts = count_patterns_dp(n, &blocks, n, |_| {});
        for k in 1..=n {
            assert!(
                counts[k] >= counts[k - 1],
                "counts[{k}]={} must be >= counts[{}]={}; DP likely overflowed",
                counts[k],
                k - 1,
                counts[k - 1]
            );
        }
        // With 8 free orthogonal axes the base-grid collinearities are
        // preserved, so counts[n] is strictly less than n! but still well above
        // u64::MAX.
        assert!(counts[n] > u128::from(u64::MAX));
    }

    #[test]
    fn max_length_zero_on_nonempty_grid_returns_only_empty_pattern() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        assert_eq!(count(g.points.len(), &blocks, 0), vec![1]);
    }

    #[test]
    fn max_length_one_reports_only_singletons() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        assert_eq!(count(g.points.len(), &blocks, 1), vec![1, 9]);
    }

    #[test]
    fn max_length_four_matches_android_minimum_run() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        assert_eq!(count(g.points.len(), &blocks, 4)[4], 1_624);
    }

    // Verify the closed-form formula P(n,k) = n!/(n-k)! is mathematically
    // correct for n = 0..=7 by checking against the falling-factorial definition.
    #[test]
    fn unconstrained_formula_is_falling_factorial() {
        for n in 0..=7usize {
            let zero_blocks = vec![0u32; n * n];
            let counts = count(n, &zero_blocks, n);
            assert_eq!(counts.len(), n + 1);

            let mut expected = vec![0u128; n + 1];
            expected[0] = 1;
            let mut perm = 1u128;
            for (k, slot) in expected.iter_mut().enumerate().skip(1) {
                perm *= (n - k + 1) as u128;
                *slot = perm;
            }
            assert_eq!(counts, expected, "n={n}");
        }
    }

    // For a grid with no collinear triplets the fast path must fire and must
    // produce the same counts that the DP previously returned (regression guard).
    #[test]
    fn fast_path_matches_known_dp_result_for_unconstrained_grid() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![1, 1], vec![0, 1]]);
        let blocks = compute_blocks(&g);
        assert!(
            blocks.iter().all(|&b| b == 0),
            "expected zero block matrix for a square grid"
        );
        let n = g.points.len();
        // P(4, k) for k=0..4: [1, 4, 12, 24, 24]
        assert_eq!(count(n, &blocks, n), vec![1, 4, 12, 24, 24]);
    }

    // For a grid with collinear points the fast path must NOT fire; the DP
    // result must differ from the unconstrained formula.
    #[test]
    fn constrained_grid_does_not_use_fast_path() {
        // Three collinear points: node 1 blocks 0→2 and 2→0.
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![2, 0]]);
        let blocks = compute_blocks(&g);
        assert!(
            blocks.iter().any(|&b| b != 0),
            "expected non-zero block matrix for collinear points"
        );
        let n = g.points.len();
        let counts = count(n, &blocks, n);
        // Constrained: only 4 valid length-3 patterns, not 3! = 6.
        assert_eq!(counts[3], 4);
        // Unconstrained formula would give P(3,3) = 6.
        assert_ne!(counts[3], 6);
    }

    // DfsEvent::Progress must fire n · (n-1) times during a constrained run
    // (one tick per top-level (start, second) pair), and zero times when the
    // unconstrained fast path short-circuits the computation.
    #[test]
    fn dfs_progress_fires_once_per_start_second_pair_when_constrained() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let ticks = std::cell::Cell::new(0usize);
        count_patterns_dfs(n, &blocks, n, |event| {
            if matches!(event, DfsEvent::Progress { .. }) {
                ticks.set(ticks.get() + 1);
            }
        });
        assert_eq!(ticks.get(), n * (n - 1));
        assert_eq!(ticks.get() as u64, dfs_progress_ticks(n));
    }

    #[test]
    fn dfs_progress_does_not_fire_on_unconstrained_fast_path() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![1, 1], vec![0, 1]]);
        let blocks = compute_blocks(&g);
        assert!(blocks.iter().all(|&b| b == 0));
        let n = g.points.len();
        let ticks = std::cell::Cell::new(0usize);
        count_patterns_dfs(n, &blocks, n, |event| {
            if matches!(event, DfsEvent::Progress { .. }) {
                ticks.set(ticks.get() + 1);
            }
        });
        assert_eq!(ticks.get(), 0);
    }

    // DFS must emit one LengthDone event for every slot of the returned
    // vector, with values matching the vector. This is what the CLI relies
    // on to print per-length lines through the same plumbing as DP.
    #[test]
    fn dfs_length_done_events_match_returned_counts() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let seen = std::cell::RefCell::new(Vec::<(usize, u128)>::new());
        let counts = count_patterns_dfs(n, &blocks, n, |event| {
            if let DfsEvent::LengthDone { length, count } = event {
                seen.borrow_mut().push((length, count));
            }
        });
        let expected: Vec<(usize, u128)> = counts.iter().copied().enumerate().collect();
        assert_eq!(*seen.borrow(), expected);
    }

    // Fast path must still emit LengthDone events so the CLI gets the same
    // streaming output for every DFS invocation, regardless of whether the
    // constrained search actually ran.
    #[test]
    fn dfs_length_done_events_fire_on_fast_path() {
        let g = grid(2, vec![vec![0, 0], vec![1, 0], vec![1, 1], vec![0, 1]]);
        let blocks = compute_blocks(&g);
        assert!(blocks.iter().all(|&b| b == 0));
        let n = g.points.len();
        let seen = std::cell::RefCell::new(Vec::<(usize, u128)>::new());
        let counts = count_patterns_dfs(n, &blocks, n, |event| {
            if let DfsEvent::LengthDone { length, count } = event {
                seen.borrow_mut().push((length, count));
            }
        });
        let expected: Vec<(usize, u128)> = counts.iter().copied().enumerate().collect();
        assert_eq!(*seen.borrow(), expected);
    }

    // DfsEvent::Progress carries a running tally. By the time the final
    // Progress event fires every contribution has been accumulated, so its
    // snapshot must equal the returned `counts`.
    #[test]
    fn dfs_progress_final_snapshot_matches_counts() {
        let g = build_grid_definition(&[3, 3], 0);
        let blocks = compute_blocks(&g);
        let n = g.points.len();
        let last = std::cell::RefCell::new(Vec::<u128>::new());
        let counts = count_patterns_dfs(n, &blocks, n, |event| {
            if let DfsEvent::Progress { counts } = event {
                last.borrow_mut().clear();
                last.borrow_mut().extend_from_slice(counts);
            }
        });
        assert_eq!(*last.borrow(), counts);
    }
}
