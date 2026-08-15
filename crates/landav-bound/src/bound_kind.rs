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

impl BoundKind {
    /// The fieldless constructor tag.
    pub(crate) const fn shape(&self) -> crate::bound_shape::BoundShape {
        use crate::bound_shape::BoundShape;

        match self {
            Self::Const(_) => BoundShape::Const,
            Self::Var(_) => BoundShape::Var,
            Self::Sum(_) => BoundShape::Sum,
            Self::Max(_) => BoundShape::Max,
            Self::Prod(_) => BoundShape::Prod,
            Self::Trans { .. } => BoundShape::Trans,
        }
    }
}

impl Canonical for BoundKind {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;

        // Shapes order by the **explicitly written** canonical tag, never by
        // Rust declaration order.
        let tags = self
            .shape()
            .canonical_tag()
            .cmp(&other.shape().canonical_tag());
        if tags != Ordering::Equal {
            return tags;
        }
        match (self, other) {
            (Self::Const(left), Self::Const(right)) => left.canonical_cmp(right),
            (Self::Var(left), Self::Var(right)) => left.canonical_cmp(right),
            (Self::Sum(left), Self::Sum(right)) | (Self::Prod(left), Self::Prod(right)) => {
                left.canonical_cmp(right)
            }
            (Self::Max(left), Self::Max(right)) => left.canonical_cmp(right),
            (
                Self::Trans {
                    kind: left_kind,
                    base: left_base,
                    arg: left_arg,
                },
                Self::Trans {
                    kind: right_kind,
                    base: right_base,
                    arg: right_arg,
                },
            ) => left_kind
                .canonical_cmp(right_kind)
                .then_with(|| left_base.canonical_cmp(right_base))
                .then_with(|| left_arg.canonical_cmp(right_arg)),
            // Unreachable: the tags above already agree, so the shapes do.
            _ => Ordering::Equal,
        }
    }

    fn write_canonical(&self, out: &mut Vec<u8>) {
        out.push(self.shape().canonical_tag());
        match self {
            Self::Const(magnitude) => magnitude.write_canonical(out),
            Self::Var(var) => var.write_canonical(out),
            Self::Sum(terms) | Self::Prod(terms) => terms.write_canonical(out),
            Self::Max(terms) => terms.write_canonical(out),
            Self::Trans { kind, base, arg } => {
                kind.write_canonical(out);
                base.write_canonical(out);
                arg.write_canonical(out);
            }
        }
    }
}
