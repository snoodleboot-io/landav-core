//! [`ResourceId`] - the identity of a *resource*, and the cache key component.

/// The stable identity of a registered resource: `ops`, `alloc`, `peak-mem`,
/// `queries`.
///
/// Distinct from [`crate::SemiringId`], and the distinction is a soundness
/// one. Keying an incremental cache on the semiring means
/// `landav check app.py --resource alloc` populates an entry that
/// `--resource ops` then hits, reporting the allocation bound as the operation
/// count - silently, with plausible numbers. The cache key is built from this
/// type; see [`crate::CacheKeyMaterial`].
///
/// This is also the value `--resource` accepts on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceId(&'static str);

impl ResourceId {
    /// Names a resource.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// The name, exactly as `--resource` spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl core::fmt::Display for ResourceId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}
