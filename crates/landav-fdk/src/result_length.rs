//! [`ResultLength`] - how a signature row relates its result's length to an
//! argument's.

use serde::Deserialize;

/// What a pack declares about **how many values a callee's result holds**.
///
/// # This is not a cost, and the separation is the whole reason it exists
///
/// [`crate::CostClass`] answers *how much work did the callee do*. This answers
/// *how many values came back*. They are two claims about two different things
/// and a row must be able to make either without the other:
///
/// ```text
/// [[signature]]
/// callee = "sorted"
/// cost = "loglinear"                    # still unresolvable, still a hole
/// result_length = "exactly-argument0"   # and yet its result is len(argument)
/// ```
///
/// `sorted(records)` yields exactly `len(records)` values *and* costs
/// `n log n`. A frontend that read the length declaration as a discharge of the
/// cost would publish a complete bound for a program that sorts - a number the
/// program exceeds, which is the failure class with a zero target on this
/// project. Keeping the two in two fields, read through two accessors with two
/// different admissibility gates, is what stops that being a comment nobody
/// reads: see [`crate::SignaturePack::length_relation`] beside
/// [`crate::SignaturePack::signature`].
///
/// # Exactness lives in the value, not in a second field
///
/// `set(x)` collapses equal elements, so `{1, 1, 2}` from a three-element input
/// holds two. Its result's length is an **upper** bound and never an equality,
/// and a loop over it is `O` and never `Theta` - the same distinction `LAN-91`
/// drew for a set display. Spelling that as a separate `exact = false` flag
/// would let a row default the wrong way in silence; spelling it in the variant
/// means a reviewer reads the difference in the diff.
///
/// # Argument 0, and what that means for a method row
///
/// Every variant names **argument 0**, because that is where every
/// length-preserving builtin in the shipped pack takes the thing it preserves
/// the length of. For a method row - `items`, `keys`, `values` - argument 0 is
/// the **receiver**: `len(d.items()) == len(d)` needs no new vocabulary, only
/// the frontend agreeing which expression argument 0 is at a call site it can
/// see. A callee that preserved the length of a *later* argument would need a
/// variant of its own, and none in the pack does.
///
/// # Unknown is the default, and the default fails closed
///
/// A row that says nothing declares nothing. The pack is an allowlist: a callee
/// with no `result_length` confers no length, exactly as a callee absent from
/// the pack does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ResultLength {
    /// Nothing is claimed. The result's length is not a function of anything
    /// this pack can name.
    #[default]
    Unknown,
    /// The result holds exactly as many values as argument 0 does.
    ExactlyArgument0,
    /// The result holds **at most** as many values as argument 0 does.
    ///
    /// `set`, `frozenset`: equal elements collapse, so the count is an
    /// inequality and a loop over the result is `O` rather than `Theta`.
    AtMostArgument0,
}

impl ResultLength {
    /// Whether this row says anything at all about its result's length.
    #[must_use]
    pub const fn is_declared(self) -> bool {
        !matches!(self, Self::Unknown)
    }

    /// Whether the relation is an equality rather than an upper bound.
    ///
    /// [`Self::Unknown`] answers `false`, which is the safe reading in the one
    /// place it could be asked by mistake: an over-approximation labelled
    /// `Exact` is the error this type exists to make hard.
    #[must_use]
    pub const fn is_exact(self) -> bool {
        matches!(self, Self::ExactlyArgument0)
    }
}
