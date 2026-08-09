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
    /// The operands, in canonical order, pairwise distinct. Always at least
    /// two.
    #[must_use]
    pub fn as_slice(&self) -> &[Bound] {
        todo!()
    }

    /// The number of operands. Always at least two.
    #[must_use]
    pub fn len(&self) -> usize {
        todo!()
    }

    /// Always `false`. Present because clippy requires it alongside
    /// [`Self::len`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        todo!()
    }
}

impl Canonical for MaxTerms {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        todo!()
    }

    fn write_canonical(&self, out: &mut Vec<u8>) {
        todo!()
    }
}
