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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    /// Per-width sanity for [`Mask::low_bits`]: at the edge `n = MAX_POINTS`
    /// the shift `1 << n` is the high bit of the type, so `(1 << n) - 1`
    /// covers every position the algorithm will ever address.
    #[test]
    fn low_bits_at_max_points_covers_every_position() {
        assert_eq!(<u32 as Mask>::low_bits(31), 0x7FFF_FFFF);
        assert_eq!(<u64 as Mask>::low_bits(63), 0x7FFF_FFFF_FFFF_FFFF);
        assert_eq!(
            <u128 as Mask>::low_bits(127),
            0x7FFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF,
        );
        // n == 0 collapses to the empty mask in every width.
        assert_eq!(<u32 as Mask>::low_bits(0), 0);
        assert_eq!(<u64 as Mask>::low_bits(0), 0);
        assert_eq!(<u128 as Mask>::low_bits(0), 0);
    }

    /// `bit(i)` is a single-bit mask; the position is recoverable via
    /// `trailing_zeros`. Sweeping every position covers the inherent
    /// implementations all three impls forward to.
    #[test]
    fn bit_and_trailing_zeros_round_trip_per_width() {
        for i in 0..<u32 as Mask>::MAX_POINTS {
            let b = <u32 as Mask>::bit(i);
            assert_eq!(b.count_ones(), 1);
            assert_eq!(b.trailing_zeros() as usize, i);
        }
        for i in 0..<u64 as Mask>::MAX_POINTS {
            let b = <u64 as Mask>::bit(i);
            assert_eq!(b.count_ones(), 1);
            assert_eq!(b.trailing_zeros() as usize, i);
        }
        for i in 0..<u128 as Mask>::MAX_POINTS {
            let b = <u128 as Mask>::bit(i);
            assert_eq!(b.count_ones(), 1);
            assert_eq!(b.trailing_zeros() as usize, i);
        }
    }

    /// `wrapping_neg` and `wrapping_sub_one` are the only methods on the
    /// trait whose semantics differ from the obvious arithmetic
    /// equivalents at the type boundary; pinning them per-width prevents
    /// a future impl from forwarding to a checked variant by accident.
    #[test]
    fn wrapping_helpers_match_inherent_semantics() {
        assert_eq!(<u32 as Mask>::wrapping_neg(0), 0);
        assert_eq!(<u32 as Mask>::wrapping_neg(1), u32::MAX);
        assert_eq!(<u64 as Mask>::wrapping_neg(0), 0);
        assert_eq!(<u64 as Mask>::wrapping_neg(1), u64::MAX);
        assert_eq!(<u128 as Mask>::wrapping_neg(0), 0);
        assert_eq!(<u128 as Mask>::wrapping_neg(1), u128::MAX);

        // `wrapping_sub_one(1)` is the canonical "all bits below this one"
        // mask: with the low bit set it must collapse to zero.
        assert_eq!(<u32 as Mask>::wrapping_sub_one(1), 0);
        assert_eq!(<u32 as Mask>::wrapping_sub_one(0x8000_0000), 0x7FFF_FFFF);
        assert_eq!(<u64 as Mask>::wrapping_sub_one(1), 0);
        assert_eq!(<u128 as Mask>::wrapping_sub_one(1), 0);
    }

    /// Gosper's hack: the next mask of the same popcount starting from
    /// `0b0011` is `0b0101`. We only assert the first hop per width;
    /// the colex-rank perfect-hash test in [`crate::counter::tests`]
    /// already pins a full sweep on the `u32` impl through the DP, and
    /// the cross-width parity tests cover `u64` and `u128` by running
    /// the full algorithm.
    #[test]
    fn gosper_next_advances_to_colex_successor() {
        assert_eq!(<u32 as Mask>::gosper_next(0b0011), 0b0101);
        assert_eq!(<u64 as Mask>::gosper_next(0b0011), 0b0101);
        assert_eq!(<u128 as Mask>::gosper_next(0b0011), 0b0101);
    }

    /// `smallest_for` walks the `(u32, u64, u128)` ladder so the existing
    /// fast path is preserved at `n ≤ 31` and the wider widths are only
    /// engaged when strictly necessary.
    #[test]
    fn smallest_for_picks_the_cheapest_sufficient_width() {
        assert_eq!(smallest_for(0), Some(Width::U32));
        assert_eq!(smallest_for(31), Some(Width::U32));
        assert_eq!(smallest_for(32), Some(Width::U64));
        assert_eq!(smallest_for(63), Some(Width::U64));
        assert_eq!(smallest_for(64), Some(Width::U128));
        assert_eq!(smallest_for(127), Some(Width::U128));
        assert_eq!(smallest_for(128), None);
        assert_eq!(smallest_for(usize::MAX), None);
    }

    /// `MAX_POINTS` exposes the widest impl's ceiling so error messages
    /// can name a single number without a per-call lookup.
    #[test]
    fn max_points_matches_u128_ceiling() {
        assert_eq!(MAX_POINTS, 127);
    }
}
