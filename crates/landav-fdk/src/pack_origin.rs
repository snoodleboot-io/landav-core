//! [`PackOrigin`] - where a row came from.

use std::{fmt, path::PathBuf};

/// Where a signature row came from.
///
/// # Why this is not a field of [`crate::Signature`]
///
/// A row's TOML has no origin key and must not have one: origin is not
/// something a pack author declares, it is something the *loader* knows, and a
/// pack that could name its own origin could name someone else's. So the pack
/// carries it and [`crate::SignaturePack::origin_of`] answers it.
///
/// It is also why there is no `Default`. A defaulted origin would have to be
/// one of these two, and [`Self::Builtin`] is the answer that claims more
/// trust than it earned — a user row silently attributed to the shipped pack
/// is a bound that reports no premise when it rests on one.
///
/// # What it is for
///
/// Two things, both of which need to survive a merge. A reader has to be able
/// to see that a row was overridden and by what
/// ([`crate::SignaturePack::shadowed`]), and a bound resting on a row nobody
/// shipped has to say so rather than looking like a bound resting on the
/// builtin.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum PackOrigin {
    /// The pack compiled into this binary.
    Builtin,
    /// A pack read from disk at runtime, at this path.
    File(PathBuf),
}

impl PackOrigin {
    /// Whether this is the pack that shipped with the binary.
    ///
    /// The question every trust decision actually asks. Written once here so
    /// that a caller does not match on the variants and get the answer wrong
    /// when a third one arrives.
    #[must_use]
    pub const fn is_builtin(&self) -> bool {
        matches!(self, Self::Builtin)
    }
}

impl fmt::Display for PackOrigin {
    /// Names the pack the way an operator has to see it to act on it.
    ///
    /// A path, verbatim, for a pack on disk: told that a bound rests on a row
    /// somebody supplied, the first thing a reader needs is which file to open.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin => formatter.write_str("the builtin pack"),
            Self::File(path) => write!(formatter, "{}", path.display()),
        }
    }
}
