//! [`Terms`] - the operands of a `Sum` or `Prod` node.

use crate::{bound::Bound, canonical::Canonical};

/// The operands of an n-ary [`crate::BoundKind::Sum`] or
/// [`crate::BoundKind::Prod`], with **two or more** elements, held in
/// canonical order.
///
/// Two invariants, both maintained by the only code that can mint one:
///
/// 1. **Arity `>= 2`.** A one-element `Sum` and its single operand denote the
///    same function, so allowing both would give one value two
///    representations - a determinism hazard before normalisation has even
///    started. The smart constructors collapse arity 0 and 1.
/// 2. **Sorted** by [`crate::Canonical::canonical_cmp`], not by `Ord` (which
///    [`Bound`] deliberately does not have). Sorting makes structural equality
///    agree with associative-commutative equality at each node,
///    deterministically and without an interner.
///
/// # There is no public constructor
///
/// `Terms` can be *observed* but not *built* from outside this crate. That
/// closes two holes at once: a validated `Terms` cannot be moved from a `Sum`
/// into a `Prod` (they share this payload type), and a caller cannot mint
/// `Sum[Sum[a, b], c]` and break flatness - the one invariant Rust's type
/// system cannot express. Flatness is a canonicity property rather than a
/// soundness property, but canonicity *is* LAN-58 AC3.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Terms(Vec<Bound>);

impl Terms {
    /// Wraps operands that the caller has already flattened, folded, sorted
    /// (and, for [`MaxTerms`], deduplicated) into canonical order.
    ///
    /// Crate private: the invariants are maintained by the smart constructors
    /// on [`Bound`], which are the only code that can reach this.
    pub(crate) fn from_canonical(operands: Vec<Bound>) -> Self {
        Self(operands)
    }
    /// The operands, in canonical order. Always at least two.
    #[must_use]
    pub fn as_slice(&self) -> &[Bound] {
        &self.0
    }

    /// The number of operands. Always at least two.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always `false`; the arity invariant is `>= 2`. Present because clippy
    /// requires it alongside [`Self::len`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Canonical for Terms {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        crate::bound::compare_operands(&self.0, &other.0)
    }

    fn write_canonical(&self, out: &mut Vec<u8>) {
        crate::bound::write_operands(&self.0, out);
    }
}
