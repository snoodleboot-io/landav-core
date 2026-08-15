//! [`Blames`] - a non-empty, canonically ordered set of blame records.

use crate::blame::Blame;

/// One or more [`Blame`] records, held sorted and deduplicated.
///
/// The field is private; there is no `Default`, no `new()` taking a
/// collection, and no `FromIterator`. The only constructor takes a first
/// `Blame` **by value**. An empty blame list - a bare "unknown" - has no
/// representation in this crate.
///
/// Sorted and deduplicated on every insertion, so the published order is
/// content determined. Push order would otherwise be whatever the engine's
/// container produced, and that order reaches the report text and the
/// serialised partial bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blames(Vec<Blame>);

impl Blames {
    /// A ledger containing exactly `first`.
    #[must_use]
    pub fn new(first: Blame) -> Self {
        Self(vec![first])
    }

    /// Inserts `next`, keeping the collection sorted and deduplicated.
    pub fn insert(&mut self, next: Blame) {
        // Sorted and deduplicated on insertion, so the published order is
        // content determined rather than push determined.
        if let Err(at) = self.0.binary_search(&next) {
            self.0.insert(at, next);
        }
    }

    /// Merges `other` into this ledger.
    pub fn merge(&mut self, other: Self) {
        for record in other.0 {
            self.insert(record);
        }
    }

    /// The records, sorted and deduplicated. Always at least one.
    #[must_use]
    pub fn as_slice(&self) -> &[Blame] {
        &self.0
    }

    /// The number of records. Always at least one.
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
