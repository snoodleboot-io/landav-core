//! [`LawFailure`] - a law violation, named.

use crate::{law::Law, semiring_id::SemiringId};

/// A law the instance failed, with enough context to reproduce it.
///
/// The suite returns this rather than panicking, so that it can be run from
/// library code behind the `laws` feature without violating "never panic in
/// library code".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{semiring} violates {law}: {detail}")]
pub struct LawFailure {
    /// Which instance.
    pub semiring: SemiringId,
    /// Which law.
    pub law: Law,
    /// The offending elements and valuation, rendered.
    pub detail: String,
}

impl core::fmt::Display for SemiringId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
