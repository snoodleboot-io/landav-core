//! [`ExprId`] - a handle into a [`crate::SourceProgram`]'s expression arena.

/// The identity of an arithmetic expression inside one [`crate::SourceProgram`].
///
/// # Why the source IR is an arena and not a tree
///
/// Non-negotiable 2 says library code must never panic, and a stack overflow
/// is worse than a panic: it aborts the process, so the blame path that makes
/// a partial result useful dies with it. A `Box`-linked expression tree has
/// **three** separate recursive hazards - building it, walking it, and
/// *dropping* it - and the third is the one that gets missed, because it is
/// generated code that no `#[test]` names. `Drop` for a 500 000-deep tree
/// overflows the stack even if every algorithm in the crate is a worklist.
///
/// Indices into a flat `Vec` have none of those hazards by construction:
/// dropping a `Vec<SourceExpr>` is a linear walk of a flat buffer whatever
/// shape the *logical* tree has. That is why the abstraction a frontend fills
/// in is an arena, and it is the reason this crate needs no depth guard of its
/// own to satisfy non-negotiable 2.
///
/// # Handles are scoped to one program
///
/// An `ExprId` is meaningful only in the [`crate::SourceProgram`] that issued
/// it. Using one against a different program is a frontend bug, and the
/// lowering reports it as [`crate::LoweringError::Malformed`] rather than
/// panicking - every arena lookup in this crate goes through a total,
/// `Option`-returning accessor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExprId(pub(crate) u32);

impl ExprId {
    /// The index this handle stands for.
    ///
    /// Published for diagnostics and test assertions. It is not a constructor:
    /// there is no way to build an `ExprId` from an arbitrary number outside
    /// this crate, which is what keeps a handle from naming a node that does
    /// not exist.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}
