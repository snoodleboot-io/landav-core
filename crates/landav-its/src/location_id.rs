//! [`LocationId`] - the identity of a control location.

/// One control location of the emitted system.
///
/// Locations are numbered by the lowering in allocation order and rendered as
/// `l0`, `l1`, ... . The numbering is an implementation detail of the lowering
/// and not a contract: a change to the traversal order renumbers every
/// location without changing what the system means. Tests that need to name a
/// location should reach for [`crate::Location::label`], which is derived from
/// the source construct, rather than the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocationId(pub(crate) u32);

impl LocationId {
    /// The number this location is rendered with.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

impl core::fmt::Display for LocationId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "l{}", self.0)
    }
}
