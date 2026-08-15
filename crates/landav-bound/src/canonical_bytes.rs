//! [`CanonicalBytes`] - the stable byte form of a bound.

/// The canonical byte encoding of a [`crate::Bound`].
///
/// Exists because neither obvious cache key is sound:
///
/// * `HashMap<Bound, _>` / `RandomState` is seeded per process, so a persisted
///   cache misses 100% of the time across runs and any code that *iterates*
///   such a map produces non-deterministic output;
/// * `DefaultHasher` is stable within a release but std documents the
///   algorithm as unspecified across releases, and it is 64-bit - a
///   persistent cache collides at around `2^32` entries, and a collision
///   serves *a different program's bound*, which is the unsound direction.
///
/// These bytes are the sound key *material*. They are not themselves the key:
/// see [`crate::CacheKeyMaterial`], which prefixes them with
/// [`crate::NORMAL_FORM_VERSION`] and the [`crate::ResourceId`] and is what a
/// `>= 128`-bit digest must be taken over.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalBytes(Vec<u8>);

impl CanonicalBytes {
    /// Wraps an already-canonical encoding.
    #[must_use]
    pub(crate) fn from_vec(bytes: Vec<u8>) -> Self {
        todo!()
    }

    /// The encoding.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        todo!()
    }

    /// The encoding's length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        todo!()
    }

    /// Always `false` - a canonical encoding always carries at least a version
    /// tag. Present because clippy requires it alongside [`Self::len`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        todo!()
    }
}
