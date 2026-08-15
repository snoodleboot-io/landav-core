//! [`Base`] - a validated integer base for `pow` and `log`.

use serde::{Deserialize, Serialize};

use crate::{bound_error::BoundError, canonical::Canonical};

/// An integer base for [`crate::Bound::pow`] and [`crate::Bound::log`],
/// guaranteed `>= 2`.
///
/// Bases 0 and 1 are the entire domain-error surface of both operators
/// (`log_1` is undefined, `1^x` is constant, `0^x` is `1, 0, 0, ...` and
/// therefore **not monotone**). This newtype makes them unrepresentable, which
/// is what allows [`crate::Nat::ceil_log`] and [`crate::Nat::exp_of`] to be
/// infallible and what makes `Pow` monotone at all.
///
/// `try_from` rather than a derived `Deserialize`: serde must not be an escape
/// hatch around the invariant.
///
/// **Base is anti-monotone and that is fine.** `k1 >= k2` implies
/// `log_k1(x) <= log_k2(x)`. The base is a literal, not an argument, so
/// argument-wise monotonicity is unaffected - but it constrains LAN-58:
/// `log_2(x) -> log_4(x)` is **unsound** as a bound rewrite, while
/// `log_4(x) -> log_2(x)` is sound-but-loosening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32")]
pub struct Base(u32);

impl Base {
    /// Base 2 - the common case, available without touching `Result`.
    pub const TWO: Self = Self(2);
    /// Base 10.
    pub const TEN: Self = Self(10);

    /// Validates `k >= 2`.
    ///
    /// # Errors
    ///
    /// [`BoundError::BaseTooSmall`] if `k < 2`.
    pub fn new(k: u32) -> Result<Self, BoundError> {
        if k < 2 {
            return Err(BoundError::BaseTooSmall { got: k });
        }
        Ok(Self(k))
    }

    /// The validated base, always `>= 2`.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for Base {
    type Error = BoundError;

    fn try_from(k: u32) -> Result<Self, Self::Error> {
        Self::new(k)
    }
}

impl Canonical for Base {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.cmp(&other.0)
    }

    fn write_canonical(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0.to_be_bytes());
    }
}
