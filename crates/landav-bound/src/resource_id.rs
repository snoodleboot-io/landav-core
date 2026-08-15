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

#[cfg(test)]
mod identity {
    use super::ResourceId;

    /// `as_str` round-trips the name exactly. This value is what
    /// `--resource` accepts *and* what the cache key is built from, so a
    /// constructor that normalised the case or trimmed the hyphen would make
    /// `peak-mem` and `peakmem` two keys for one analysis.
    #[test]
    fn the_name_round_trips_byte_for_byte() {
        assert_eq!(ResourceId::new("peak-mem").as_str(), "peak-mem");
        assert_eq!(ResourceId::new("ops").as_str(), "ops");
        assert_eq!(ResourceId::new("").as_str(), "");
    }

    /// `Display` is the bare name: it reaches `--help` and the unknown-value
    /// error, where a `ResourceId("ops")` wrapper would be noise.
    #[test]
    fn display_is_the_bare_name() {
        assert_eq!(ResourceId::new("alloc").to_string(), "alloc");
        assert_eq!(
            ResourceId::new("peak-mem").to_string(),
            ResourceId::new("peak-mem").as_str()
        );
    }

    /// Distinct names are distinct ids, and `Ord` is content derived - the
    /// registry's ordered surface depends on it.
    #[test]
    fn identity_and_order_are_content_derived() {
        assert_eq!(ResourceId::new("ops"), ResourceId::new("ops"));
        assert_ne!(ResourceId::new("ops"), ResourceId::new("alloc"));
        assert!(ResourceId::new("alloc") < ResourceId::new("ops"));
    }

    /// `new` is `const`, so the registry can build a `static` table.
    #[test]
    fn new_is_usable_in_a_const_context() {
        const OPS: ResourceId = ResourceId::new("ops");
        assert_eq!(OPS.as_str(), "ops");
    }
}
