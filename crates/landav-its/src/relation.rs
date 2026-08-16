//! [`Relation`] - how a [`crate::Constraint`]'s polynomial compares to zero.

/// The comparison in a constraint `p R 0`.
///
/// # Three, not six
///
/// Every constraint is normalised to compare a polynomial against **zero**, so
/// `a <= b` and `b >= a` are the same constraint written twice and only one
/// spelling survives. That leaves three relations rather than six, and it is
/// what makes [`crate::Guard`] canonical enough to deduplicate.
///
/// `!=` is deliberately **not** here. Over the integers `p != 0` is
/// `p > 0 \/ -p > 0`, a genuine disjunction, and a disjunction is not one
/// guard - it is two transitions. Expanding it during the disjunctive-normal-
/// form step keeps every emitted guard a plain conjunction, which is what
/// every ITS solver expects and avoids depending on a `!=` extension that
/// KoAT's format does not require dialects to support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Relation {
    /// `p >= 0`
    Ge,
    /// `p > 0`
    Gt,
    /// `p = 0`
    Eq,
}

impl Relation {
    /// The relation as KoAT writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ge => ">=",
            Self::Gt => ">",
            Self::Eq => "=",
        }
    }

    /// Whether `value R 0` holds.
    #[must_use]
    pub const fn holds(self, value: i128) -> bool {
        match self {
            Self::Ge => value >= 0,
            Self::Gt => value > 0,
            Self::Eq => value == 0,
        }
    }
}

impl core::fmt::Display for Relation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
