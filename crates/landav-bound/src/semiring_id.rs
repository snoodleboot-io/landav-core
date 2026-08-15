//! [`SemiringId`] - the identity of an *algebra*.

/// The stable identity of a cost semiring.
///
/// This names the **algebra**, not the resource. Three registered resources
/// (`ops`, `alloc`, `queries`) share the algebra `additive`, so a
/// `SemiringId` does not identify what was counted.
///
/// It must therefore **never** be used as a cache key. See
/// [`crate::ResourceId`] and [`crate::CacheKeyMaterial`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemiringId(&'static str);

impl SemiringId {
    /// Names an algebra.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// The name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[cfg(test)]
mod identity {
    use super::SemiringId;
    use crate::{b::B, dioid::Dioid, max_plus::MaxPlus};

    /// `as_str` round-trips the name exactly.
    #[test]
    fn the_name_round_trips_byte_for_byte() {
        assert_eq!(SemiringId::new("additive").as_str(), "additive");
        assert_eq!(SemiringId::new("peak").as_str(), "peak");
        assert_eq!(SemiringId::new("").as_str(), "");
    }

    /// The two shipped algebras are distinct, and their names are frozen: they
    /// appear in every [`crate::LawFailure`] a future instance produces.
    #[test]
    fn the_shipped_algebras_are_named_and_distinct() {
        assert_eq!(B::SEMIRING.as_str(), "additive");
        assert_eq!(MaxPlus::SEMIRING.as_str(), "peak");
        assert_ne!(B::SEMIRING, MaxPlus::SEMIRING);
    }

    /// `new` is `const`, so it can be an associated const on a `Dioid`.
    #[test]
    fn new_is_usable_in_a_const_context() {
        const ADDITIVE: SemiringId = SemiringId::new("additive");
        assert_eq!(ADDITIVE, B::SEMIRING);
    }
}
