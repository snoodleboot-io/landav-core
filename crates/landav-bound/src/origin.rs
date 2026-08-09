//! [`Origin`] - an opaque frontend-supplied location.

use crate::symbol::Symbol;

/// An opaque, frontend-supplied location.
///
/// Core does not know what a line number is, only how to carry one back out.
/// The payload is a [`Symbol`], so `Origin` is `Ord` by content - which is
/// what lets [`crate::Blames`] impose a canonical order on itself rather than
/// inheriting whatever order the engine's containers happened to produce.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Origin(Symbol);

impl Origin {
    /// Wraps a frontend-supplied location.
    #[must_use]
    pub fn new(location: impl Into<Symbol>) -> Self {
        Self(location.into())
    }

    /// The location, as the frontend spelled it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl core::fmt::Display for Origin {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
