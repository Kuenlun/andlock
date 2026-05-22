// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Bitmask integer abstraction for the visited-set in [`crate::counter`].
//!
//! The DP encodes the set of already-visited nodes as a bitmask. The mask
//! width caps `n` at one less than the integer's bit count, since the
//! algorithm computes `(1 << n) - 1` and would shift past the type width
//! at `n = WIDTH`. Three impls ship in this crate:
//!
//! | Mask | `MAX_POINTS` | Notes                                           |
//! |------|-------------:|-------------------------------------------------|
//! | `u32`  | 31         | Same speed as the prior `u32`-only path.        |
//! | `u64`  | 63         | Same speed as `u32` on 64-bit CPUs.             |
//! | `u128` | 127        | ~1.5–2× slower; reserved for very large `n`.    |
//!
//! [`smallest_for`] returns the cheapest sufficient width for a given
//! `n`; the binary's pipeline uses it to pick the M that
//! [`crate::counter::count_patterns_dp`] is monomorphised on, so the
//! type parameter never escapes to the CLI.

use std::ops::{BitAnd, BitOr, BitOrAssign, BitXor, BitXorAssign, Not, Shl};

/// Bitmask integer that the DP uses to encode the visited-node set.
///
/// Implementations forward each method to the corresponding inherent
/// method on the underlying primitive — there is no extra arithmetic.
/// Monomorphization gives the compiler full freedom to emit native
/// `popcnt`, `tzcnt`, and bitwise instructions per width.
///
/// # Invariants assumed by the DP
/// All callers within `andlock` already enforce these via earlier
/// asserts; impls do not re-check them in the hot loop:
/// - `low_bits(n)` is only called with `n <= MAX_POINTS`, so the shift
///   `1 << n` stays within the type width.
/// - `bit(i)` is only called with `i < MAX_POINTS`, same rationale.
/// - `wrapping_sub_one` is only ever invoked on a non-zero single-bit
///   mask, so it never wraps in practice.
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
    /// The all-zero mask. Used as the loop sentinel for set-bit
    /// extraction in the DP (`while mask != Self::ZERO`).
    const ZERO: Self;

    /// Largest `n` this mask type accepts.
    ///
    /// Equal to `BITS - 1`: the DP computes `(1 << n) - 1` for the full
    /// mask, so `n` must satisfy `n < BITS` for the shift to be defined.
    const MAX_POINTS: usize;

    /// `1 << i`. Defined for `i < MAX_POINTS`; calling with
    /// `i >= MAX_POINTS + 1` (i.e. shift past the type width) panics
    /// in debug builds and is undefined behaviour the rest of the time
    /// — the inherent `<<` operator's contract.
    fn bit(i: usize) -> Self;

    /// `(1 << n) - 1`, the "first `n` bits set" mask. Defined for
    /// `n <= MAX_POINTS`; passing `n > MAX_POINTS` panics on the shift
    /// inside the impl. Callers in [`crate::counter`] gate this with
    /// the `n <= M::MAX_POINTS` assert at the top of
    /// [`crate::counter::count_patterns_dp`].
    fn low_bits(n: usize) -> Self;

    /// Number of set bits.
    fn count_ones(self) -> u32;

    /// Position of the lowest set bit. The DP only invokes this on a
    /// non-zero argument (an extracted single-bit mask), so the result
    /// is always strictly less than `BITS`.
    fn trailing_zeros(self) -> u32;

    /// Two's-complement negation, used for the `x & x.wrapping_neg()`
    /// idiom that extracts the lowest set bit of `x`.
    #[must_use]
    fn wrapping_neg(self) -> Self;

    /// `self - 1`, wrapping. Invoked only on a non-zero single-bit mask
    /// (`next_bit`), so the result is "all bits below `next_bit` set"
    /// without ever wrapping in practice.
    #[must_use]
    fn wrapping_sub_one(self) -> Self;

    /// Next mask with the same popcount as `self` (Gosper's hack).
    /// Used to enumerate masks in popcount-ascending colex order so
    /// each popcount layer can stream a [`crate::counter::DpEvent::LengthDone`]
    /// event the moment it completes.
    #[must_use]
    fn gosper_next(self) -> Self;
}

impl Mask for u32 {
    const ZERO: Self = 0;
    const MAX_POINTS: usize = 31;

    #[inline]
    fn bit(i: usize) -> Self {
        1u32 << i
    }
    #[inline]
    fn low_bits(n: usize) -> Self {
        (1u32 << n) - 1
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

impl Mask for u64 {
    const ZERO: Self = 0;
    const MAX_POINTS: usize = 63;

    #[inline]
    fn bit(i: usize) -> Self {
        1u64 << i
    }
    #[inline]
    fn low_bits(n: usize) -> Self {
        (1u64 << n) - 1
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

impl Mask for u128 {
    const ZERO: Self = 0;
    const MAX_POINTS: usize = 127;

    #[inline]
    fn bit(i: usize) -> Self {
        1u128 << i
    }
    #[inline]
    fn low_bits(n: usize) -> Self {
        (1u128 << n) - 1
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

/// Hard upper bound across every shipped [`Mask`] impl.
///
/// Equal to `<u128 as Mask>::MAX_POINTS`. The grid validator uses this as
/// the public ceiling so that error messages and JSON-rejection paths
/// stay consistent regardless of which width a particular `n` selects at
/// runtime.
pub const MAX_POINTS: usize = <u128 as Mask>::MAX_POINTS;

/// Width tag returned by [`smallest_for`].
///
/// Carrying it in an enum instead of a `usize` lets the dispatcher in
/// [`crate::counter`] `match` exhaustively, so adding a fourth width is
/// a compile error at every dispatch site rather than a silent
/// fallthrough.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Width {
    /// `u32` mask, `n <= 31`.
    U32,
    /// `u64` mask, `32 <= n <= 63`.
    U64,
    /// `u128` mask, `64 <= n <= 127`.
    U128,
}

/// Returns the smallest [`Mask`] impl that can represent `n` points,
/// or `None` when `n` exceeds even the widest supported type.
///
/// The walk is `u32` → `u64` → `u128`; matching that order keeps the
/// existing fast path on `u32` for any `n` that fits there.
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
