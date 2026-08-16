//! [`SourceCond`] - one node of the fragment's condition language.

use landav_bound::Symbol;

use crate::{compare_op::CompareOp, cond_id::CondId, construct::Construct, expr_id::ExprId};

/// A truth-valued condition, as one arena node.
///
/// # Truthiness is the frontend's problem
///
/// There is no "is this value true" node. `if x:` in a language where a bare
/// integer is a condition means `x != 0`, and *that* is a language fact, so
/// the frontend spells it as a [`SourceCond::Compare`] against zero. Core
/// never learns the rule, which is non-negotiable 4 applied to the smallest
/// case that is easy to get wrong.
///
/// # Short-circuit evaluation
///
/// [`SourceCond::And`] and [`SourceCond::Or`] are the *logical* connectives,
/// not the short-circuiting operators of any particular language, and the
/// difference does not matter here because every condition in this fragment is
/// pure: a condition either contains no [`SourceCond::Unsupported`] node, in
/// which case evaluating both sides has no effect and cannot fail, or it
/// contains one and the whole lowering refuses. A frontend whose `and` can
/// hide a side effect must emit [`Construct::Call`] rather than an `And`.
/// Exhaustive on purpose; see [`crate::SourceExpr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceCond {
    /// A comparison of two integer expressions.
    Compare {
        /// Which comparison.
        op: CompareOp,
        /// The left operand.
        left: ExprId,
        /// The right operand.
        right: ExprId,
    },
    /// Conjunction.
    And {
        /// The left conjunct.
        left: CondId,
        /// The right conjunct.
        right: CondId,
    },
    /// Disjunction.
    Or {
        /// The left disjunct.
        left: CondId,
        /// The right disjunct.
        right: CondId,
    },
    /// Negation.
    Not {
        /// The negated condition.
        operand: CondId,
    },
    /// A condition the frontend could not translate.
    ///
    /// Refused rather than approximated. Approximating an *unknown* condition
    /// to "either branch may be taken" would be sound for the control flow, but
    /// a condition the frontend could not translate may also have an effect on
    /// the integer state - a call, an assignment expression - and there is no
    /// sound over-approximation of an unknown effect.
    Unsupported {
        /// What was refused.
        construct: Construct,
        /// Frontend-supplied specifics, if any.
        detail: Option<Symbol>,
    },
}
