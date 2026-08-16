//! [`ItsVar`] - a variable of the emitted integer transition system.

use landav_bound::Symbol;

use crate::var_name::VarName;

/// One integer variable of the emitted system.
///
/// Two kinds of variable end up here and they are deliberately the same type:
/// the program's own variables, and the fresh ones the lowering introduces to
/// desugar a counted loop. A solver has no reason to tell them apart, and a
/// second type would only invite a lowering that forgot to declare one of the
/// two families in the variable tuple - which is a malformed system, not a
/// type error.
///
/// [`crate::Its::params`] carries the subset that a derived bound may be
/// expressed in, which is the distinction that *does* matter downstream.
///
/// # Range
///
/// An `ItsVar` ranges over the mathematical integers, unbounded in both
/// directions, matching both the source fragment's contract (see
/// [`VarName`]) and KoAT's semantics. Nothing in this crate models a machine
/// word, and nothing in it wraps.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItsVar(Symbol);

impl ItsVar {
    /// Wraps a name.
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

impl From<&VarName> for ItsVar {
    fn from(name: &VarName) -> Self {
        Self(name.symbol().clone())
    }
}

impl core::fmt::Display for ItsVar {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
