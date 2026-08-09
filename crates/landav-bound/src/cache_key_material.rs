//! [`CacheKeyMaterial`] - the only sound input to an F-008 cache key.

use crate::{bound::Bound, resource_id::ResourceId};

/// The exact byte string a persistent cache key must be a digest of.
///
/// Concatenates, in this order:
///
/// 1. [`crate::NORMAL_FORM_VERSION`], so a change to the canonical order, the
///    rewrite set or the extraction cost function invalidates every entry;
/// 2. the [`ResourceId`] - **never** the [`crate::SemiringId`]. Three
///    registered resources (`ops`, `alloc`, `queries`) share one semiring, so
///    a key on the semiring serves the allocation bound as the operation
///    count, silently, with plausible numbers;
/// 3. the bound's [`crate::CanonicalBytes`].
///
/// The caller takes a **`>= 128`-bit** digest of these bytes with a
/// cryptographic hash. A 64-bit key over a persistent cache collides at around
/// `2^32` entries, and a collision serves a different program's bound.
/// This crate deliberately does not pick the digest function, so the choice is
/// not frozen into the algebra's dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKeyMaterial(Vec<u8>);

impl CacheKeyMaterial {
    /// Builds the key material for `bound` under `resource`.
    #[must_use]
    pub fn new(_resource: ResourceId, _bound: &Bound) -> Self {
        todo!()
    }

    /// The bytes to digest.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        todo!()
    }
}
