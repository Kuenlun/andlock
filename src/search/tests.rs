// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

use super::*;

#[test]
fn weighted_accumulation_never_emits_wrapped_counts() -> Result<(), TryReserveError> {
    for tail_length in [1usize, 2] {
        for multiply in [false, true] {
            let blocks = [0u128; 16];
            let mut counter = PrefixCounter {
                allowed: None,
                n: 4,
                blocks: &blocks,
                full_mask: 15,
                prefix_length: 1,
                tail_length,
                count: if multiply { 0u128 } else { u128::MAX },
                scratch: DpScratch::allocate_constrained(3, tail_length)?,
                reduced: vec![0; 9],
                on_event: |_| ControlFlow::Continue(()),
                cancelled: false,
                overflow: false,
            };
            counter.visit(1, 0, 1, if multiply { u128::MAX } else { 1 });
            assert!(counter.overflow);
            assert!(!counter.cancelled);
        }
    }
    Ok(())
}

#[test]
fn seeded_overflow_never_finalizes_a_partial_target() -> Result<(), TryReserveError> {
    for weight in [1u128, 41] {
        let blocks = vec![0u128; 41 * 41];
        let reduced = vec![0u128; 40 * 40];
        let mut counter = PrefixCounter {
            allowed: None,
            n: 41,
            blocks: &blocks,
            full_mask: (1u128 << 41) - 1,
            prefix_length: 1,
            tail_length: 40,
            count: 7u128,
            scratch: DpScratch::allocate(40, &reduced, 40)?,
            reduced,
            on_event: |_| ControlFlow::Continue(()),
            cancelled: false,
            overflow: false,
        };
        counter.visit(1, 0, 1, weight);
        assert!(counter.overflow);
        assert_eq!(counter.count, 7);
        assert!(!counter.cancelled);
    }
    Ok(())
}

#[test]
fn big_targets_accumulate_weighted_continuations_beyond_u128() -> Result<(), TryReserveError> {
    for (tail_length, continuations) in [(1usize, 3u32), (2, 6)] {
        let blocks = [0u128; 16];
        let mut counter = PrefixCounter {
            allowed: None,
            n: 4,
            blocks: &blocks,
            full_mask: 15,
            prefix_length: 1,
            tail_length,
            count: BigUint::from(u128::MAX),
            scratch: DpScratch::allocate_constrained(3, tail_length)?,
            reduced: vec![0; 9],
            on_event: |_| ControlFlow::Continue(()),
            cancelled: false,
            overflow: false,
        };
        counter.visit(1, 0, 1, u128::MAX);
        assert!(!counter.overflow);
        assert!(!counter.cancelled);
        assert_eq!(
            counter.count,
            BigUint::from(u128::MAX) * (continuations + 1)
        );
    }
    Ok(())
}

#[test]
fn big_plans_bound_every_seeded_count_and_choose_the_shortest_fitting_prefix() {
    for n in 0..=MAX_POINTS {
        for length in 0..=n {
            for budget in [0, 128, 1 << 20, u64::MAX] {
                let plan = count_plan_with::<BigUint>(n, length, budget);
                assert!(plan.table_bytes <= budget);
                assert!(local_counts_fit(
                    n - plan.prefix_length,
                    length - plan.prefix_length
                ));
                if plan.prefix_length > 0 {
                    let previous = plan.prefix_length - 1;
                    assert!(
                        dp_table_bytes(n - previous, length - previous) > budget
                            || !local_counts_fit(n - previous, length - previous)
                    );
                }
            }
        }
    }
}
