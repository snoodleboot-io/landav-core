//! [`Valuation`] - a total assignment of magnitudes to variables.

use crate::{nat::Nat, var_id::VarId};

/// A total assignment of magnitudes to variables.
///
/// Total **by signature**: no `Option`, no `Result`. A partial map cannot
/// implement this trait without first choosing, explicitly, what an absent
/// variable means. Pushing that decision into [`crate::TotalValuation`] is
/// what lets [`crate::Bound::eval`] be infallible, which in turn is what makes
/// "no panics on omega in any operator" checkable by reading five function
/// bodies.
pub trait Valuation {
    /// The magnitude of `var`.
    fn value_of(&self, var: &VarId) -> Nat;
}
