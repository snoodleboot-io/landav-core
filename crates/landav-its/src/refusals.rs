//! [`Refusals`] - a non-empty, canonically ordered set of refused constructs.

use landav_bound::{Blames, Origin};

use crate::{construct::Construct, unsupported::Unsupported};

/// One or more [`Unsupported`] records, held sorted and deduplicated.
///
/// # Non-empty by construction
///
/// The field is private, there is no `Default`, no `new()` taking a collection
/// and no `FromIterator`; the only constructor takes a first [`Unsupported`]
/// **by value**. This mirrors [`landav_bound::Blames`] deliberately and for
/// the same reason: an empty refusal ledger is a bare "unsupported" with
/// nothing named, and it has no representation in this crate.
///
/// # Why every refusal, and not just the first
///
/// A lowering that stopped at the first refusal would make `LAN-68`'s coverage
/// report a lie by omission - fix the first construct, discover the second,
/// repeat. The lowering therefore walks the whole program even after it knows
/// it will refuse, and collects every reason. That includes code that the
/// control flow cannot reach: a construct sitting after a `return` is still
/// reported, because "we did not look" and "we looked and it was fine" are
/// different answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusals(Vec<Unsupported>);

impl Refusals {
    /// A ledger containing exactly `first`.
    #[must_use]
    pub fn new(first: Unsupported) -> Self {
        Self(vec![first])
    }

    /// Inserts `next`, keeping the collection sorted and deduplicated.
    pub fn insert(&mut self, next: Unsupported) {
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
    pub fn as_slice(&self) -> &[Unsupported] {
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

    /// Every distinct [`Construct`] in the ledger, in canonical order.
    ///
    /// The aggregation `LAN-68`'s coverage report is built from.
    #[must_use]
    pub fn constructs(&self) -> Vec<Construct> {
        let mut seen: Vec<Construct> = self.0.iter().map(Unsupported::construct).collect();
        seen.sort_unstable();
        seen.dedup();
        seen
    }

    /// How many refusals name `construct`.
    #[must_use]
    pub fn count_of(&self, construct: Construct) -> usize {
        self.0
            .iter()
            .filter(|record| record.construct() == construct)
            .count()
    }

    /// Every position named, in canonical order, without deduplication.
    #[must_use]
    pub fn origins(&self) -> Vec<&Origin> {
        self.0.iter().map(Unsupported::origin).collect()
    }

    /// This ledger as a [`landav_bound::Blames`].
    ///
    /// The `F-015` seam; see [`Unsupported::blame`]. Total, and never empty,
    /// because `Refusals` is never empty.
    #[must_use]
    pub fn blames(&self) -> Blames {
        let mut records = self.0.iter().map(Unsupported::blame);
        // `Refusals` is non-empty by construction, so this is not a fallible
        // step in disguise; the fallback keeps the function total without a
        // panic if that invariant is ever broken by a future constructor.
        let first = records.next().unwrap_or_else(|| {
            Unsupported::new(Construct::NonIntegerValue, Origin::new("<empty>")).blame()
        });
        let mut blames = Blames::new(first);
        for record in records {
            blames.insert(record);
        }
        blames
    }
}

impl core::fmt::Display for Refusals {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (index, record) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{record}")?;
        }
        Ok(())
    }
}
