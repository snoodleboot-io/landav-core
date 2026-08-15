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

#[cfg(test)]
mod frozen_numbering {
    use super::Law;

    /// The whole point of this enum is that no document can renumber the laws.
    /// Pinned one at a time, because a `zip` of two lists that both moved
    /// would pass.
    #[test]
    fn tags_are_pinned() {
        assert_eq!(Law::PlusMonoid.tag(), "L1");
        assert_eq!(Law::TimesMonoid.tag(), "L2");
        assert_eq!(Law::Distributivity.tag(), "L3");
        assert_eq!(Law::Annihilation.tag(), "L4");
        assert_eq!(Law::StarUnfolding.tag(), "L5");
        assert_eq!(Law::ZeroSumFreedom.tag(), "L6");
        assert_eq!(Law::Antisymmetry.tag(), "L7");
        assert_eq!(Law::NonDegeneracy.tag(), "L8");
        assert_eq!(Law::StarAtZero.tag(), "L9");
        assert_eq!(Law::StarMonotonicity.tag(), "L10");
        assert_eq!(Law::Idempotence.tag(), "L11");
    }

    /// `ALL` is in numbering order, complete, and duplicate free. The law
    /// suite iterates it to build its per-law report, so a missing or repeated
    /// entry silently loses or double counts a law's coverage.
    #[test]
    fn all_is_complete_ordered_and_duplicate_free() {
        assert_eq!(Law::ALL.len(), 11);
        assert_eq!(
            Law::ALL,
            [
                Law::PlusMonoid,
                Law::TimesMonoid,
                Law::Distributivity,
                Law::Annihilation,
                Law::StarUnfolding,
                Law::ZeroSumFreedom,
                Law::Antisymmetry,
                Law::NonDegeneracy,
                Law::StarAtZero,
                Law::StarMonotonicity,
                Law::Idempotence,
            ]
        );
        let mut sorted: Vec<Law> = Law::ALL.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 11, "ALL must not repeat a law");
    }

    /// `Display` is the tag, so a failure message names the law by number.
    /// `L10` and `L11` are the ones a `tag()` written with a `&tag[..2]`
    /// slice would truncate.
    #[test]
    fn display_is_the_tag() {
        for law in Law::ALL {
            assert_eq!(law.to_string(), law.tag());
        }
        assert_eq!(Law::StarMonotonicity.to_string(), "L10");
        assert_eq!(Law::Idempotence.to_string(), "L11");
    }
}
