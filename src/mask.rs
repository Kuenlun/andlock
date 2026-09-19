// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
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
    /// Empty visited set.
    const ZERO: Self;
    /// Largest supported node count for this mask width.
    const MAX_POINTS: usize;

    /// One bit at index i, which must be below the mask bit width.
    fn bit(i: usize) -> Self;
    /// The lowest n bits set, with n at most [`Self::MAX_POINTS`].
    fn low_bits(n: usize) -> Self;
    /// Number of set bits.
    fn count_ones(self) -> u32;
    /// Index of the lowest set bit, or the bit width for an empty mask.
    fn trailing_zeros(self) -> u32;
    /// Two's-complement negation, wrapping at the mask width.
    #[must_use]
    fn wrapping_neg(self) -> Self;
    /// Subtract one, wrapping at the mask width.
    #[must_use]
    fn wrapping_sub_one(self) -> Self;
    /// Next mask with the same popcount (Gosper's hack).
    /// The mask must be nonzero and have a representable successor.
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
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "n is below the bit width, so the shifted one is nonzero."
            )]
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
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "The nonzero input mask has a nonzero isolated low bit."
            )]
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
    /// 32-bit visited set.
    U32,
    /// 64-bit visited set.
    U64,
    /// 128-bit visited set.
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
