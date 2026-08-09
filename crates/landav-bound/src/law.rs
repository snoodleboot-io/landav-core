//! [`Law`] - the authoritative numbering of the dioid laws.

/// The eleven frozen laws, named once so no document can renumber them.
///
/// Earlier drafts called zero-sum-freeness `L8` in one place and `L6` in
/// another, with `L8` reserved for idempotence elsewhere - which would have
/// shipped one law implemented twice and another not at all. This enum is the
/// single source of truth; [`crate::dioid::Dioid`] documents the content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Law {
    /// L1: `plus` associative, commutative, identity `zero`.
    PlusMonoid,
    /// L2: `times` associative, identity `one`.
    TimesMonoid,
    /// L3: `times` distributes over `plus`, both sides.
    Distributivity,
    /// L4: `times(zero, a) == zero == times(a, zero)`.
    Annihilation,
    /// L5: star unfolding, as an equation, both sides.
    StarUnfolding,
    /// L6: `plus(a, b) == zero` implies both are `zero`.
    ZeroSumFreedom,
    /// L7: antisymmetry of the canonical preorder.
    Antisymmetry,
    /// L8: `zero() != one()`.
    NonDegeneracy,
    /// L9: `star(zero) == one`.
    StarAtZero,
    /// L10: `a <= b` implies `star(a) <= star(b)`.
    StarMonotonicity,
    /// L11: `plus(a, a) == a` for all `a` iff `PLUS_IDEMPOTENT`, with a
    /// counter-witness required when the flag is `false`.
    Idempotence,
}

impl Law {
    /// Every law, in numbering order.
    pub const ALL: [Self; 11] = [
        Self::PlusMonoid,
        Self::TimesMonoid,
        Self::Distributivity,
        Self::Annihilation,
        Self::StarUnfolding,
        Self::ZeroSumFreedom,
        Self::Antisymmetry,
        Self::NonDegeneracy,
        Self::StarAtZero,
        Self::StarMonotonicity,
        Self::Idempotence,
    ];

    /// The law's number, as `L1` .. `L11`.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::PlusMonoid => "L1",
            Self::TimesMonoid => "L2",
            Self::Distributivity => "L3",
            Self::Annihilation => "L4",
            Self::StarUnfolding => "L5",
            Self::ZeroSumFreedom => "L6",
            Self::Antisymmetry => "L7",
            Self::NonDegeneracy => "L8",
            Self::StarAtZero => "L9",
            Self::StarMonotonicity => "L10",
            Self::Idempotence => "L11",
        }
    }
}

impl core::fmt::Display for Law {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.tag())
    }
}
