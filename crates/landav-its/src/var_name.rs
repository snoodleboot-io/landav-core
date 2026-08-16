//! [`VarName`] - the identity of a program variable in the source fragment.

use landav_bound::Symbol;

/// The name of an integer-valued program variable, as the frontend spells it.
///
/// A newtype over [`Symbol`] so that a program variable and a bound's
/// input-size variable ([`landav_bound::VarId`]) cannot be confused at a call
/// site: they are different things that happen to share a carrier. `Ord` is
/// content derived, which is what lets every collection in this crate publish
/// a deterministic order.
///
/// # The contract this name carries
///
/// A `VarName` denotes a **mathematical integer**, unbounded in both
/// directions. That is an obligation on the frontend, not a fact Core can
/// check: if a frontend emits a `VarName` for a value that is a float, a
/// string, an object, or a machine integer that wraps, every bound derived
/// downstream is unsound. A frontend that cannot establish integrality must
/// emit [`crate::Construct`]-tagged unsupported nodes instead - which is why
/// there is no "unknown type" spelling here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarName(Symbol);

impl VarName {
    /// Wraps a frontend-supplied variable name.
    #[must_use]
    pub fn new(name: impl Into<Symbol>) -> Self {
        Self(name.into())
    }

    /// The name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The underlying symbol.
    #[must_use]
    pub const fn symbol(&self) -> &Symbol {
        &self.0
    }
}

impl core::fmt::Display for VarName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
