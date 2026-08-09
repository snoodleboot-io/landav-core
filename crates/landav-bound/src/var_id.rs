//! [`VarId`] - the identity of an input-size variable.

use crate::{canonical::Canonical, symbol::Symbol};

/// The identity of an input-size variable.
///
/// A newtype over [`Symbol`] so that a variable name and an unaccounted-term
/// name cannot be mixed up at a call site, and so `VarId`'s `Ord` - which
/// [`crate::Bound::vars`] and [`crate::TotalValuation`] both sort by - is
/// content derived.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarId(Symbol);

impl VarId {
    /// Wraps a frontend-supplied name.
    #[must_use]
    pub fn new(name: impl Into<Symbol>) -> Self {
        todo!()
    }

    /// The underlying name.
    #[must_use]
    pub fn symbol(&self) -> &Symbol {
        todo!()
    }
}

impl core::fmt::Display for VarId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        todo!()
    }
}

impl Canonical for VarId {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        todo!()
    }

    fn write_canonical(&self, out: &mut Vec<u8>) {
        todo!()
    }
}
