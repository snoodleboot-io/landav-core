//! [`NodeId`] - a handle into whichever of a [`crate::SourceProgram`]'s three
//! arenas holds the node.

use crate::{cond_id::CondId, expr_id::ExprId, stmt_id::StmtId};

/// One node of a [`crate::SourceProgram`], whichever arena it lives in.
///
/// # Why the arena is part of the identity
///
/// The three arenas index independently, so `ExprId(4)`, `CondId(4)` and
/// `StmtId(4)` are three different nodes that all say "4". A consumer that
/// reconciles what it visited against what the program contains -
/// [`crate::SourceProgram::unsupported_nodes`] is the reason this type exists -
/// needs a key that cannot conflate them, or an expression it charged would
/// silently excuse a statement it did not.
///
/// `Ord` is derived and content derived throughout, so a set of these has a
/// deterministic order and two runs over identical input reconcile identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeId {
    /// A node in the expression arena.
    Expr(ExprId),
    /// A node in the condition arena.
    Cond(CondId),
    /// A node in the statement arena.
    Stmt(StmtId),
}

impl core::fmt::Display for NodeId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Expr(id) => write!(f, "expr#{}", id.index()),
            Self::Cond(id) => write!(f, "cond#{}", id.index()),
            Self::Stmt(id) => write!(f, "stmt#{}", id.index()),
        }
    }
}
