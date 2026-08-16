//! [`ArithOp`] - the binary arithmetic the fragment covers.

/// A binary arithmetic operator over the integers.
///
/// The three that keep an expression polynomial, and no others. Division and
/// modulo are absent by design rather than by omission: they are not
/// polynomial, so there is no [`crate::Polynomial`] to put them in, and a
/// frontend meeting one must emit [`crate::Construct::IntegerDivision`]. See
/// the crate-level docs for why that is a refusal today and what the exact
/// encoding would be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArithOp {
    /// Integer addition.
    Add,
    /// Integer subtraction.
    Sub,
    /// Integer multiplication.
    Mul,
}

impl ArithOp {
    /// The operator as it is written in source and in KoAT output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
        }
    }
}

impl core::fmt::Display for ArithOp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
