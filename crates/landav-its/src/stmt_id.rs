//! [`StmtId`] - a handle into a [`crate::SourceProgram`]'s statement arena.

/// The identity of a statement inside one [`crate::SourceProgram`].
///
/// See [`crate::ExprId`] for why these are arena handles and not `Box`es.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StmtId(pub(crate) u32);

impl StmtId {
    /// The index this handle stands for.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}
