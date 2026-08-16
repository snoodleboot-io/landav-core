//! [`Rat`] - an exact rational, used only inside a summation.

/// A rational number in lowest terms, with a positive denominator.
///
/// # Why this exists at all
///
/// Summing a polynomial in closed form goes through rationals even when the
/// answer is an integer. `sum over k < T of k` is `T(T-1)/2`, and the halving
/// is unavoidable in the middle of the calculation - but in
/// `sum over k < T of (1 + 2k)` the coefficient `2` clears the denominator and
/// the `-T` cancels against the `+T` from the constant term, leaving `T^2`.
///
/// So the rationals are **internal machinery**, never a result. A
/// [`landav_bound::Bound`] cannot hold one, and the summation refuses to
/// produce a bound unless every surviving coefficient came out a non-negative
/// integer. See [`Rat::to_natural`].
///
/// # Saturating, not wrapping
///
/// Every operation saturates to `None` on overflow rather than wrapping. A
/// wrapped numerator would produce a plausible-looking coefficient that is
/// numerically wrong, and the whole point of this type is that the arithmetic
/// is exact or absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rat {
    /// Carries the sign. Cancellation is the reason this is signed: intermediate
    /// coefficients go negative even when the answer does not.
    numerator: i128,
    /// Always strictly positive, so the sign lives in one place.
    denominator: i128,
}

impl Rat {
    /// Zero.
    pub const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };

    /// A whole number.
    #[must_use]
    pub const fn whole(value: i128) -> Self {
        Self {
            numerator: value,
            denominator: 1,
        }
    }

    /// `numerator / denominator`, reduced. `None` for a zero denominator.
    #[must_use]
    pub fn new(numerator: i128, denominator: i128) -> Option<Self> {
        if denominator == 0 {
            return None;
        }
        // The sign is carried by the numerator so that equality is structural
        // once both are reduced.
        let (numerator, denominator) = if denominator < 0 {
            (numerator.checked_neg()?, denominator.checked_neg()?)
        } else {
            (numerator, denominator)
        };
        let divisor = gcd(numerator.unsigned_abs(), denominator.unsigned_abs());
        let divisor = i128::try_from(divisor).ok()?;
        if divisor == 0 {
            return Some(Self::ZERO);
        }
        Some(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    /// Whether this is exactly zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.numerator == 0
    }

    /// `self + other`, or `None` on overflow.
    ///
    /// Named `plus` rather than `add` to match `Nat`'s vocabulary in
    /// `landav-bound`, and so it cannot be mistaken for `std::ops::Add`, which
    /// this type deliberately does not implement - every operation here is
    /// partial, and an operator that returns `Self` would have to panic.
    #[must_use]
    pub fn plus(self, other: Self) -> Option<Self> {
        let left = self.numerator.checked_mul(other.denominator)?;
        let right = other.numerator.checked_mul(self.denominator)?;
        Self::new(
            left.checked_add(right)?,
            self.denominator.checked_mul(other.denominator)?,
        )
    }

    /// `self * other`, or `None` on overflow.
    #[must_use]
    pub fn times(self, other: Self) -> Option<Self> {
        Self::new(
            self.numerator.checked_mul(other.numerator)?,
            self.denominator.checked_mul(other.denominator)?,
        )
    }

    /// `self / divisor`, or `None` for a zero divisor or on overflow.
    #[must_use]
    pub fn divide_by_whole(self, divisor: i128) -> Option<Self> {
        Self::new(self.numerator, self.denominator.checked_mul(divisor)?)
    }

    /// This as a count, if it is a non-negative whole number.
    ///
    /// # The gate
    ///
    /// This is where the internal rationals meet the bound algebra, and it is
    /// deliberately narrow. `None` means the closed form exists but cannot be
    /// *written down* as a bound - either because a denominator survived, or
    /// because a coefficient came out negative and bounds live in the naturals.
    ///
    /// A caller that gets `None` must fall back to an approximation and say so.
    /// It must never round, because rounding a coefficient down makes a bound
    /// unsound and rounding it up makes an exact answer silently loose.
    #[must_use]
    pub fn to_natural(self) -> Option<u64> {
        if self.denominator != 1 || self.numerator < 0 {
            return None;
        }
        u64::try_from(self.numerator).ok()
    }
}

/// Binary GCD's simpler cousin; the inputs here are small.
const fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let next = a % b;
        a = b;
        b = next;
    }
    a
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::Rat;

    #[test]
    fn a_rational_reduces_to_lowest_terms() {
        assert_eq!(Rat::new(2, 4), Rat::new(1, 2));
        assert_eq!(Rat::new(-6, 8), Rat::new(-3, 4));
    }

    /// The sign lives in the numerator, so two spellings of the same negative
    /// value compare equal.
    #[test]
    fn the_sign_is_carried_by_the_numerator() {
        assert_eq!(Rat::new(1, -2), Rat::new(-1, 2));
    }

    #[test]
    fn a_zero_denominator_is_refused() {
        assert_eq!(Rat::new(1, 0), None);
    }

    /// The cancellation this type exists for: `1 - 2/2 = 0`.
    #[test]
    fn coefficients_can_cancel_to_zero() {
        let one = Rat::whole(1);
        let minus_half = Rat::new(-1, 2).expect("a valid rational");
        let two = Rat::whole(2);
        let sum = one
            .plus(minus_half.times(two).expect("no overflow"))
            .expect("no overflow");
        assert!(sum.is_zero());
    }

    #[test]
    fn only_a_non_negative_whole_number_becomes_a_count() {
        assert_eq!(Rat::whole(3).to_natural(), Some(3));
        assert_eq!(Rat::ZERO.to_natural(), Some(0));
        // A surviving denominator: the closed form is not writable as a bound.
        assert_eq!(Rat::new(1, 2).and_then(Rat::to_natural), None);
        // Negative: bounds live in the naturals.
        assert_eq!(Rat::whole(-1).to_natural(), None);
    }

    /// Saturating rather than wrapping. A wrapped numerator would be a
    /// plausible-looking coefficient that is numerically wrong.
    #[test]
    fn overflow_is_absent_rather_than_wrapped() {
        let huge = Rat::whole(i128::MAX);
        assert_eq!(huge.times(huge), None);
        assert_eq!(huge.plus(huge), None);
    }
}
