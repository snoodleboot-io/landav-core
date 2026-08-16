//! [`CondId`] - a handle into a [`crate::SourceProgram`]'s condition arena.

/// The identity of a condition inside one [`crate::SourceProgram`].
///
/// Conditions live in their own arena rather than sharing the expression one
/// because they are a different sort: an expression denotes an integer, a
/// condition denotes a truth value, and the lowering treats them completely
/// differently - an expression becomes a [`crate::Polynomial`], a condition
/// becomes a set of guarded transitions. Keeping them apart makes "a condition
/// used where an integer was meant" unrepresentable instead of a runtime
/// check.
///
/// See [`crate::ExprId`] for why these are arena handles and not `Box`es.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CondId(pub(crate) u32);

impl CondId {
    /// The index this handle stands for.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}
