//! [`Assumption`] - what could not be discharged.

use crate::{symbol::Symbol, var_id::VarId};

/// The obligation a derivation could not discharge.
///
/// Language neutral by construction: every variant that names something names
/// it with a frontend-supplied [`Symbol`].
///
/// **There is deliberately no `Unknown` variant.** Adding one is then a
/// reviewable diff against "failure must carry blame", rather than the path of
/// least resistance.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Assumption {
    /// The loop or recursion was not shown to terminate.
    TerminationNotProved,
    /// No size bound was derived for this variable.
    SizeNotBounded {
        /// The variable.
        var: VarId,
    },
    /// A callee has no cost contract and no derived bound.
    CalleeCostUnknown {
        /// The callee, as the frontend names it.
        callee: Symbol,
    },
    /// Recursion is present and no ranking function was found.
    RecursionNotRanked,
    /// The expression grew past [`crate::MAX_DEPTH`] and was widened to
    /// `omega`.
    ExpressionDepthExceeded,
    /// The resource has no model for this construct.
    ResourceNotModelled {
        /// What was not modelled.
        detail: Symbol,
    },
}
