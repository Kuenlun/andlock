// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Global count accumulation, independent of compact fixed-width DP entries.

use std::fmt::Display;
use std::ops::ControlFlow;

use num_bigint::BigUint;

use crate::counter::DpEvent;
use crate::mask::MAX_POINTS;

/// Arithmetic shared by fixed-width and arbitrary-precision global counts.
pub trait GlobalCount: Clone + Default + Display + From<u128> {
    /// Arbitrary-precision totals require every seeded subproblem to fit `u128`.
    const ARBITRARY_PRECISION: bool;

    /// Add a weighted local count. On overflow, leave the previous value intact.
    fn add_scaled(&mut self, count: u128, weight: u128) -> bool;

    /// Add two global counts, returning `None` if the result cannot be represented.
    #[must_use]
    fn checked_add(&self, other: &Self) -> Option<Self>;

    /// Multiply by a scalar, returning `None` if the result cannot be represented.
    #[must_use]
    fn checked_mul(&self, factor: u128) -> Option<Self>;
}

impl GlobalCount for u128 {
    const ARBITRARY_PRECISION: bool = false;

    fn add_scaled(&mut self, count: u128, weight: u128) -> bool {
        let Some(total) = count
            .checked_mul(weight)
            .and_then(|value| Self::checked_add(*self, value))
        else {
            return false;
        };
        *self = total;
        true
    }

    fn checked_add(&self, other: &Self) -> Option<Self> {
        Self::checked_add(*self, *other)
    }

    fn checked_mul(&self, factor: Self) -> Option<Self> {
        Self::checked_mul(*self, factor)
    }
}

impl GlobalCount for BigUint {
    const ARBITRARY_PRECISION: bool = true;

    fn add_scaled(&mut self, count: u128, weight: u128) -> bool {
        *self += Self::from(count) * weight;
        true
    }

    fn checked_add(&self, other: &Self) -> Option<Self> {
        Some(self + other)
    }

    fn checked_mul(&self, factor: u128) -> Option<Self> {
        Some(self * factor)
    }
}

/// Counting progress with a configurable representation for finalized counts.
#[derive(Debug, Eq, PartialEq)]
pub enum CountEvent<C> {
    /// A state or prefix has been processed.
    Mask,
    /// Every contribution to this length has been counted.
    LengthDone { length: usize, count: C },
    /// The next count cannot be represented. Already emitted counts remain exact.
    Overflow,
}

impl<C: From<u128>> From<DpEvent> for CountEvent<C> {
    fn from(event: DpEvent) -> Self {
        match event {
            DpEvent::Mask => Self::Mask,
            DpEvent::LengthDone { length, count } => Self::LengthDone {
                length,
                count: C::from(count),
            },
            DpEvent::Overflow => Self::Overflow,
        }
    }
}

impl From<CountEvent<u128>> for DpEvent {
    fn from(event: CountEvent<u128>) -> Self {
        match event {
            CountEvent::Mask => Self::Mask,
            CountEvent::LengthDone { length, count } => Self::LengthDone { length, count },
            CountEvent::Overflow => Self::Overflow,
        }
    }
}

/// Whether every count in a seeded continuation is guaranteed to fit `u128`.
///
/// The unconstrained permutation bound also covers filtered starting points.
/// This plans partition depth; it does not replace constrained counting.
///
/// # Panics
/// Panics when `max_length > n` or `n > MAX_POINTS`.
#[must_use]
pub fn local_counts_fit(n: usize, max_length: usize) -> bool {
    assert!(max_length <= n && n <= MAX_POINTS);
    let mut bound = 1u128;
    for offset in 0..max_length {
        let Some(next) = bound.checked_mul((n - offset) as u128) else {
            return false;
        };
        bound = next;
    }
    true
}

/// Shared recurrence for a grid where every move is legal.
pub(crate) fn count_unconstrained<C: GlobalCount, F: FnMut(CountEvent<C>) -> ControlFlow<()>>(
    n: usize,
    max_length: usize,
    starts: u128,
    mut on_event: F,
) {
    if on_event(CountEvent::LengthDone {
        length: 0,
        count: C::from(1),
    })
    .is_break()
    {
        return;
    }
    let mut count = C::from(starts);
    for length in 1..=max_length {
        let factor = if length == 1 { 1 } else { n - length + 1 };
        let Some(next) = count.checked_mul(factor as u128) else {
            let _ = on_event(CountEvent::Overflow);
            return;
        };
        count = next;
        if on_event(CountEvent::LengthDone {
            length,
            count: count.clone(),
        })
        .is_break()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_fixed_count_checks_product_and_total_without_mutation() {
        let mut count = 7u128;
        assert!(!count.add_scaled(u128::MAX, 2));
        assert_eq!(count, 7);
        assert!(!count.add_scaled(u128::MAX, 1));
        assert_eq!(count, 7);
        assert!(count.add_scaled(u128::MAX - 7, 1));
        assert_eq!(count, u128::MAX);
    }

    #[test]
    fn arbitrary_precision_weighting_does_not_overflow_an_intermediate_product() {
        let mut count = BigUint::from(7u128);
        assert!(count.add_scaled(u128::MAX, 127));
        let expected = (BigUint::from(1u128) << 128) * 127u32 - 120u32;
        assert_eq!(count, expected);
        assert!(GlobalCount::checked_add(&u128::MAX, &1).is_none());
        assert_eq!(
            GlobalCount::checked_add(&BigUint::from(u128::MAX), &BigUint::from(1u128)),
            Some(BigUint::from(1u128) << 128)
        );
    }

    #[test]
    fn seeded_count_bound_matches_exact_permutations_for_every_supported_shape() {
        for n in 0..=MAX_POINTS {
            for length in 0..=n {
                let exact: BigUint = ((n - length + 1)..=n).map(BigUint::from).product();
                assert_eq!(
                    local_counts_fit(n, length),
                    exact.bits() <= 128,
                    "n={n}, length={length}"
                );
            }
        }
    }

    #[test]
    fn maximum_grid_bounds_count_and_total_payloads() {
        let mut count = BigUint::from(1u128);
        let mut total = count.clone();
        for factor in (1..=MAX_POINTS).rev() {
            count *= factor;
            total += &count;
        }
        assert_eq!(count.bits(), 710);
        assert_eq!(total.bits(), 711);
    }
}
