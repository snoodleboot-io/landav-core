//! [`Symbol`] - an opaque, content-ordered frontend-supplied name.

use std::sync::Arc;

use crate::canonical::Canonical;

/// An interned-by-nothing frontend-supplied name.
///
/// Deliberately `Arc<str>` and **not** an interner index. `Arc<str>`'s `Ord`
/// delegates to `str::cmp`, so it is content derived and therefore
/// deterministic across processes, machines and toolchains with no external
/// state. An index-based symbol makes the order a function of interning order,
/// which is a function of directory-walk order, which is a function of the
/// filesystem.
///
/// This is a hard constraint on LAN-58 as well: the e-graph mirror language
/// must **not** use `egg::Symbol`, whose `Ord` and `Hash` are a process-global
/// interner index, because egg breaks every extraction cost tie by the
/// language's `Ord`.
///
/// The price is an allocation per name and slower hashing. A future hash-cons
/// may *cache* the comparison result but may never *define* it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(Arc<str>);

impl Symbol {
    /// The name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Symbol {
    fn from(value: &str) -> Self {
        Self(Arc::from(value))
    }
}

impl From<String> for Symbol {
    fn from(value: String) -> Self {
        Self(Arc::from(value))
    }
}

impl core::fmt::Display for Symbol {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Canonical for Symbol {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        // `str::cmp` is content derived, so the order is identical in every
        // process and on every toolchain.
        self.as_str().cmp(other.as_str())
    }

    fn write_canonical(&self, out: &mut Vec<u8>) {
        let bytes = self.as_str().as_bytes();
        // Length prefixed, so the encoding is self-delimiting and no two
        // distinct names can produce the same byte run.
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(bytes);
    }
}
