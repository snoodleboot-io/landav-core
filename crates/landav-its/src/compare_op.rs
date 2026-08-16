//! [`CompareOp`] - the integer comparisons the fragment covers.

/// A comparison between two integer expressions.
///
/// All six are **exact** over the integers, in both polarities, which is the
/// property that makes conditional lowering lossless. Each one and its
/// negation are both expressible as a disjunction of [`crate::Constraint`]s:
/// `!=` becomes two strict inequalities and `==` negates into them, and over
/// the integers `a < b` is `b - a > 0` with no rounding anywhere. Nothing here
/// needs an approximation, so nothing here has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompareOp {
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `==`
    Eq,
    /// `!=`
    Ne,
}

impl CompareOp {
    /// The operator as it is written in source.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Eq => "==",
            Self::Ne => "!=",
        }
    }

    /// The comparison that holds exactly when this one does not.
    ///
    /// A total involution: `negate` applied twice is the identity, which is
    /// asserted as a property rather than assumed.
    #[must_use]
    pub const fn negate(self) -> Self {
        match self {
            Self::Lt => Self::Ge,
            Self::Le => Self::Gt,
            Self::Gt => Self::Le,
            Self::Ge => Self::Lt,
            Self::Eq => Self::Ne,
            Self::Ne => Self::Eq,
        }
    }

    /// Whether `left op right` holds for two concrete integers.
    ///
    /// The denotation of the operator, published because the property suite
    /// needs a definition of truth that is not the lowering's own.
    #[must_use]
    pub fn holds(self, left: i128, right: i128) -> bool {
        match self {
            Self::Lt => left < right,
            Self::Le => left <= right,
            Self::Gt => left > right,
            Self::Ge => left >= right,
            Self::Eq => left == right,
            Self::Ne => left != right,
        }
    }
}

impl core::fmt::Display for CompareOp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
