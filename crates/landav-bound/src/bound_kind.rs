//! [`BoundKind`] - the six constructors, as an observation type.

use crate::{
    base::Base, bound::Bound, canonical::Canonical, max_terms::MaxTerms, nat::Nat, terms::Terms,
    trans_kind::TransKind, var_id::VarId,
};

/// The six constructors of the bound algebra.
///
/// ```text
/// Bound ::= Const(Nat)                             -- N u {omega}
///         | Var(VarId)                             -- frontend-supplied size variable
///         | Sum(Terms)                             -- n-ary, n >= 2
///         | Max(MaxTerms)                          -- n-ary, n >= 2, distinct
///         | Prod(Terms)                            -- n-ary, n >= 2
///         | Trans { kind, base, arg }              -- base^arg | ceil(log_base(max(1, arg)))
/// ```
///
/// # This is an observation type, not a construction type
///
/// You can `match` on it - that is the whole point, and it is why it is not
/// `#[non_exhaustive]`: a seventh constructor *should* cost a compile error in
/// every downstream `match`, because a wildcard arm on a bound algebra is a
/// soundness hole the compiler stops pointing at.
///
/// You cannot turn one back into a [`Bound`]. There is no `Bound::from_kind`.
/// Every route into `Bound` goes through a smart constructor, which is what
/// makes the depth limit, the free-variable summary, flatness and canonical
/// ordering unbypassable in safe code.
///
/// # Why `omega` is inside `Const` rather than beside it
///
/// There is no `BoundKind::Omega`. `omega` is an inhabitant of [`Nat`], so
/// every `match` arm that handles a constant handles `omega`, and no operator
/// can be written that forgets the case. A separate variant would create
/// exactly the unmatched, unconsidered top element that "no panics on omega in
/// any operator" exists to prevent.
///
/// # Why `Pow` and `Log` share one variant
///
/// They are the only two shapes whose base is a constant rather than a
/// sub-bound, the only two with arity exactly one, they share the
/// `base >= 2` invariant, and they are adjoints on `N`. One variant means one
/// validation site, one monotonicity obligation, one rewrite family, and one
/// place to get `omega` right.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BoundKind {
    /// A literal magnitude, possibly `omega`.
    Const(Nat),
    /// An input-size variable, ranging over `N u {omega}`.
    ///
    /// Variables range over the **whole** lattice including `omega`, because
    /// [`crate::TotalValuation::saturating`] maps an absent variable to
    /// `omega`. This is why `Bound::prod` may not fold `0 * x` to `0` for a
    /// symbolic `x`: at `x = omega` the product is `omega`.
    Var(VarId),
    /// `t0 + t1 + ...`, two or more operands, flat and canonically ordered.
    Sum(Terms),
    /// `max(t0, t1, ...)`, two or more distinct operands.
    Max(MaxTerms),
    /// `t0 * t1 * ...`, two or more operands, flat and canonically ordered.
    Prod(Terms),
    /// `base ^ arg`, or `ceil(log_base(max(1, arg)))`.
    Trans {
        /// Which of the adjoint pair.
        kind: TransKind,
        /// Guaranteed `>= 2`.
        base: Base,
        /// The single operand.
        arg: Bound,
    },
}

impl Canonical for BoundKind {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        todo!()
    }

    fn write_canonical(&self, out: &mut Vec<u8>) {
        todo!()
    }
}
