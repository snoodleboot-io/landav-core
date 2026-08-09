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
