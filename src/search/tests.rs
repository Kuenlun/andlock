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
                n: 4,
                blocks: &blocks,
                full_mask: 15,
                prefix_length: 1,
                tail_length,
                cap: 1 + tail_length,
                counts: [0; MAX_POINTS + 1],
                scratch: DpScratch::allocate_constrained(3, tail_length)?,
                reduced: vec![0; 9],
                on_event: |_| ControlFlow::Continue(()),
                cancelled: false,
            };
            if !multiply {
                counter.counts[1 + tail_length] = u128::MAX;
            }
            counter.visit(1, 0, 1, if multiply { u128::MAX } else { 1 });
            assert!(counter.cap < 1 + tail_length);
            assert!(!counter.cancelled);
        }
    }
    Ok(())
}

#[test]
fn seeded_overflow_preserves_every_exact_weighted_count() -> Result<(), TryReserveError> {
    for weight in [1u128, 41] {
        let blocks = vec![0u128; 41 * 41];
        let reduced = vec![0u128; 40 * 40];
        let mut counter = PrefixCounter {
            n: 41,
            blocks: &blocks,
            full_mask: (1u128 << 41) - 1,
            prefix_length: 1,
            tail_length: 40,
            cap: 41,
            counts: [0; MAX_POINTS + 1],
            scratch: DpScratch::allocate(40, &reduced, 40)?,
            reduced,
            on_event: |_| ControlFlow::Continue(()),
            cancelled: false,
        };
        counter.visit(1, 0, 1, weight);
        let mut expected = weight;
        let mut last = 1;
        for length in 2..=41 {
            let Some(value) = expected.checked_mul((42 - length) as u128) else {
                break;
            };
            expected = value;
            last = length;
            assert_eq!(counter.counts[length], expected);
        }
        assert_eq!(counter.cap, last);
        assert_eq!(counter.counts[last + 1], 0);
        assert!(!counter.cancelled);
    }
    Ok(())
}
