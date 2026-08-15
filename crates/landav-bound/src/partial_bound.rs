//! [`PartialBound`] - a sound over-approximation with named blame.

use crate::{blames::Blames, bound::Bound};

/// A sound over-approximation together with at least one named unaccounted
/// term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialBound {
    bound: Bound,
    blames: Blames,
}

impl PartialBound {
    /// Pairs a bound with a non-empty blame ledger.
    #[must_use]
    pub fn new(bound: Bound, blames: Blames) -> Self {
        todo!()
    }

    /// The reported bound. Sound, possibly `omega`-bearing.
    #[must_use]
    pub fn bound(&self) -> &Bound {
        todo!()
    }

    /// What was not accounted for, in canonical order.
    #[must_use]
    pub fn blames(&self) -> &Blames {
        todo!()
    }
}
