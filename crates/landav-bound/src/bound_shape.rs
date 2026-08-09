//! [`BoundShape`] - the fieldless mirror of the six constructors.

/// A fieldless mirror of [`crate::BoundKind`]'s six constructors.
///
/// Two jobs, both mechanical rather than remembered:
///
/// 1. **Exhaustiveness of the test suite.** [`BoundShape::ALL`] drives the
///    monotonicity property harness through an exhaustive `match`, so adding a
///    seventh constructor is a compile error in the tests as well as in the
///    algebra. LAN-56's "for any constructor" becomes a compile error rather
///    than a review checklist item.
/// 2. **Pinning the canonical order.** [`BoundShape::canonical_tag`] assigns
///    the tags **explicitly**. Alphabetising the variants of
///    [`crate::BoundKind`] is a serde no-op, compiles clean, and passes every
///    law and monotonicity test - and under a *derived* order it would change
///    the canonical child order, therefore every normal form, every golden
///    test and every persisted cache key. Writing the tags out is what makes
///    declaration order not load bearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BoundShape {
    /// A literal magnitude, possibly `omega`.
    Const,
    /// An input-size variable.
    Var,
    /// `t0 + t1 + ...`.
    Sum,
    /// `max(t0, t1, ...)`.
    Max,
    /// `t0 * t1 * ...`.
    Prod,
    /// `base ^ arg` or `ceil(log_base(max(1, arg)))`.
    Trans,
}

impl BoundShape {
    /// Every constructor, in canonical-tag order.
    pub const ALL: [Self; 6] = [
        Self::Const,
        Self::Var,
        Self::Sum,
        Self::Max,
        Self::Prod,
        Self::Trans,
    ];

    /// The canonical tag. **Part of the frozen normal form.**
    ///
    /// Written out, not taken from declaration order. Changing any value here
    /// changes every normal form and requires bumping
    /// [`crate::NORMAL_FORM_VERSION`].
    #[must_use]
    pub const fn canonical_tag(self) -> u8 {
        match self {
            Self::Const => 0,
            Self::Var => 1,
            Self::Sum => 2,
            Self::Max => 3,
            Self::Prod => 4,
            Self::Trans => 5,
        }
    }

    /// The name used in diagnostics and in the wire form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Const => "const",
            Self::Var => "var",
            Self::Sum => "sum",
            Self::Max => "max",
            Self::Prod => "prod",
            Self::Trans => "trans",
        }
    }
}

#[cfg(test)]
mod frozen_tags {
    use super::BoundShape;

    /// The canonical tags are part of the frozen normal form. Changing any of
    /// them changes every persisted cache key, so it must fail here first.
    #[test]
    fn tags_are_pinned() {
        assert_eq!(BoundShape::Const.canonical_tag(), 0);
        assert_eq!(BoundShape::Var.canonical_tag(), 1);
        assert_eq!(BoundShape::Sum.canonical_tag(), 2);
        assert_eq!(BoundShape::Max.canonical_tag(), 3);
        assert_eq!(BoundShape::Prod.canonical_tag(), 4);
        assert_eq!(BoundShape::Trans.canonical_tag(), 5);
        assert_eq!(BoundShape::ALL.len(), 6);
    }
}
