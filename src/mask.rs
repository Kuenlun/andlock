// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Bitmask integer abstraction for the visited-set in [`crate::counter`].
//!
//! Three widths ship: `u32` (`n <= 31`), `u64` (`n <= 63`), `u128` (`n <= 127`).
//! `MAX_POINTS` for each is one less than its bit width, since the DP computes
//! `(1 << n) - 1` and needs the shift to stay in range.

use std::ops::{BitAnd, BitOr, BitOrAssign, BitXor, BitXorAssign, Not, Shl};

/// Bitmask integer encoding the visited-node set. The trait exists so the DP
/// can monomorphise per width and emit native `popcnt`/`tzcnt` instructions.
pub trait Mask:
    Copy
    + Eq
    + BitAnd<Output = Self>
    + BitOr<Output = Self>
    + BitXor<Output = Self>
    + BitOrAssign
    + BitXorAssign
    + Not<Output = Self>
    + Shl<usize, Output = Self>
{
    const ZERO: Self;
    const MAX_POINTS: usize;

    fn bit(i: usize) -> Self;
    fn low_bits(n: usize) -> Self;
    fn count_ones(self) -> u32;
    fn trailing_zeros(self) -> u32;
    #[must_use]
    fn wrapping_neg(self) -> Self;
    #[must_use]
    fn wrapping_sub_one(self) -> Self;
    /// Next mask with the same popcount (Gosper's hack).
    #[must_use]
    fn gosper_next(self) -> Self;
}

macro_rules! impl_mask {
    ($t:ty, $max:expr) => {
        impl Mask for $t {
            const ZERO: Self = 0;
            const MAX_POINTS: usize = $max;

            #[inline]
            fn bit(i: usize) -> Self {
                1 << i
            }
            #[inline]
            fn low_bits(n: usize) -> Self {
                (1 << n) - 1
            }
            #[inline]
            fn count_ones(self) -> u32 {
                Self::count_ones(self)
            }
            #[inline]
            fn trailing_zeros(self) -> u32 {
                Self::trailing_zeros(self)
            }
            #[inline]
            fn wrapping_neg(self) -> Self {
                Self::wrapping_neg(self)
            }
            #[inline]
            fn wrapping_sub_one(self) -> Self {
                self.wrapping_sub(1)
            }
            #[inline]
            fn gosper_next(self) -> Self {
                let c = self & self.wrapping_neg();
                let r = self.wrapping_add(c);
                (((r ^ self) >> 2) / c) | r
            }
        }
    };
}

impl_mask!(u32, 31);
impl_mask!(u64, 63);
impl_mask!(u128, 127);

/// Hard ceiling across every shipped [`Mask`] impl (`= 127`).
pub const MAX_POINTS: usize = <u128 as Mask>::MAX_POINTS;

/// Width tag returned by [`smallest_for`].
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Width {
    U32,
    U64,
    U128,
}

/// Smallest [`Mask`] width that fits `n` points, or `None` past [`MAX_POINTS`].
#[must_use]
pub const fn smallest_for(n: usize) -> Option<Width> {
    if n <= <u32 as Mask>::MAX_POINTS {
        Some(Width::U32)
    } else if n <= <u64 as Mask>::MAX_POINTS {
        Some(Width::U64)
    } else if n <= <u128 as Mask>::MAX_POINTS {
        Some(Width::U128)
    } else {
        None
    }
}
