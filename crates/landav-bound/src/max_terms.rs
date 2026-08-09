//! [`MaxTerms`] - the operands of a `Max` node.

use crate::{bound::Bound, canonical::Canonical};

/// The operands of a [`crate::BoundKind::Max`] node: two or more, in canonical
/// order, and **pairwise distinct**.
///
/// `max` is idempotent, so `max(a, a, b)` and `max(a, b)` denote the same
/// function. Deduplicating in the type rather than in a normalisation pass
/// removes the duplicate representation before LAN-58 ever sees it, and
/// discharges LAN-58's max-idempotence rewrite before egg is involved. `Sum`
/// and `Prod` are *not* idempotent, which is exactly why they use
/// [`crate::Terms`] and this is a separate type rather than a boolean flag.
///
/// As with [`crate::Terms`], there is no public constructor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MaxTerms(Vec<Bound>);

impl MaxTerms {
    /// Wraps operands that the caller has already flattened, folded, sorted
    /// (and, for [`MaxTerms`], deduplicated) into canonical order.
    ///
    /// Crate private: the invariants are maintained by the smart constructors
    /// on [`Bound`], which are the only code that can reach this.
    pub(crate) fn from_canonical(operands: Vec<Bound>) -> Self {
        Self(operands)
    }
    /// The operands, in canonical order, pairwise distinct. Always at least
    /// two.
    #[must_use]
    pub fn as_slice(&self) -> &[Bound] {
        &self.0
    }

    /// The number of operands. Always at least two.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always `false`. Present because clippy requires it alongside
    /// [`Self::len`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Canonical for MaxTerms {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        crate::bound::compare_operands(&self.0, &other.0)
    }

    fn write_canonical(&self, out: &mut Vec<u8>) {
        crate::bound::write_operands(&self.0, out);
    }
}
