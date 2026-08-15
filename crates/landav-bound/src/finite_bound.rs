//! [`FiniteBound`] - a bound proven free of `omega`.

use crate::bound::Bound;

/// A [`Bound`] proven to contain no `omega`.
///
/// Private field; the only way in is [`FiniteBound::try_new`]. Together with
/// the non-empty [`crate::Blames`] and a [`crate::Verdict::classify`] that is
/// the only public route to a verdict, this is how "failure must carry blame"
/// becomes a type-system property rather than a code-review rule.
///
/// `omega`-freeness is a **necessary** condition for `Proved`, not a
/// sufficient one: see [`crate::Verdict::classify`], which additionally
/// requires an empty blame ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiniteBound(Bound);

impl FiniteBound {
    /// Checks that `bound` mentions `omega` nowhere.
    ///
    /// # Errors
    ///
    /// Returns the original bound unchanged if it mentions `omega`.
    pub fn try_new(bound: Bound) -> Result<Self, Bound> {
        todo!()
    }

    /// The underlying bound.
    #[must_use]
    pub fn get(&self) -> &Bound {
        todo!()
    }
}
