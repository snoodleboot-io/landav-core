//! [`TransKind`] - which member of the transcendental pair a `Trans` node is.

use serde::{Deserialize, Serialize};

use crate::canonical::Canonical;

/// Which member of the transcendental pair a [`crate::BoundKind::Trans`] node
/// is.
///
/// `Pow` and `Log` are adjoints on the naturals, share the [`crate::Base`]
/// `>= 2` invariant, and have identical arity, so they share one constructor.
/// That is what reconciles the algebra's five closure operators with the
/// frozen count of six enum variants.
///
/// The wire tags are pinned with `#[serde(rename)]` so that renaming these
/// Rust identifiers - a pure readability refactor with no compile error
/// anywhere - cannot break the hosted platform's ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TransKind {
    /// `base ^ arg`.
    #[serde(rename = "pow")]
    Pow,
    /// `ceil(log_base(max(1, arg)))`.
    #[serde(rename = "log")]
    Log,
}

impl TransKind {
    /// The canonical tag, written out rather than taken from declaration
    /// order.
    ///
    /// Reordering the variants of this enum must not change any normal form.
    #[must_use]
    pub const fn canonical_tag(self) -> u8 {
        match self {
            Self::Pow => 0,
            Self::Log => 1,
        }
    }
}

impl Canonical for TransKind {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.canonical_tag().cmp(&other.canonical_tag())
    }

    fn write_canonical(&self, out: &mut Vec<u8>) {
        out.push(self.canonical_tag());
    }
}
