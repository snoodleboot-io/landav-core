//! [`SourceProgram`] - the language-neutral fragment a frontend hands to the
//! lowering.

use std::collections::BTreeSet;

use landav_bound::{Origin, Symbol};

use crate::{
    cond_id::CondId, expr_id::ExprId, node_id::NodeId, source_cond::SourceCond,
    source_expr::SourceExpr, source_stmt::SourceStmt, stmt_id::StmtId,
    unsupported_node::UnsupportedNode, var_name::VarName,
};

/// One function's body, expressed in the numeric fragment.
///
/// # This type is the frontend boundary
///
/// Non-negotiable 4 says no Python assumption may live outside
/// `landav-python`, and the crate graph enforces the direction:
/// `landav-python` depends on `landav-its`, so this crate cannot see a Python
/// AST even if it wanted to. What crosses the boundary is this - integer
/// variables, arithmetic, conditions, three control constructs and an explicit
/// refusal node - and nothing here mentions a language. `range` appears as
/// [`crate::RangeSpec`], a half-open integer interval with a stride, not as a
/// builtin; truthiness appears as an explicit comparison against zero;
/// compound assignment has already been expanded. Every one of those is a
/// language fact that stays on the frontend's side of the line.
///
/// # Construction
///
/// Only through [`crate::SourceProgramBuilder`], which is what keeps the arena
/// handles it issues meaningful. All accessors here are total and return
/// [`Option`]: a handle from a *different* program names an index that may not
/// exist, and the answer to that is a [`crate::LoweringError::Malformed`] with
/// blame on it, never a panic.
#[derive(Debug, Clone)]
pub struct SourceProgram {
    pub(crate) name: Symbol,
    pub(crate) params: Vec<VarName>,
    pub(crate) exprs: Vec<SourceExpr>,
    pub(crate) expr_origins: Vec<Origin>,
    pub(crate) conds: Vec<SourceCond>,
    pub(crate) cond_origins: Vec<Origin>,
    pub(crate) stmts: Vec<SourceStmt>,
    pub(crate) stmt_origins: Vec<Origin>,
    pub(crate) body: Vec<StmtId>,
    pub(crate) origin: Origin,
    pub(crate) overflowed: bool,
}

impl SourceProgram {
    /// The function's name, as the frontend spelled it.
    #[must_use]
    pub const fn name(&self) -> &Symbol {
        &self.name
    }

    /// Where the function is, as the frontend spelled the position.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The integer parameters, in declaration order.
    ///
    /// These are the variables a derived bound may be expressed *in*, so the
    /// order is part of the contract rather than an implementation detail.
    #[must_use]
    pub fn params(&self) -> &[VarName] {
        &self.params
    }

    /// The top-level statements of the function body, in source order.
    #[must_use]
    pub fn body(&self) -> &[StmtId] {
        &self.body
    }

    /// The expression `id` names, or `None` if it names nothing here.
    #[must_use]
    pub fn expr(&self, id: ExprId) -> Option<&SourceExpr> {
        self.exprs.get(index_of(id.index()))
    }

    /// Where the expression `id` names came from.
    #[must_use]
    pub fn expr_origin(&self, id: ExprId) -> Option<&Origin> {
        self.expr_origins.get(index_of(id.index()))
    }

    /// The condition `id` names, or `None` if it names nothing here.
    #[must_use]
    pub fn cond(&self, id: CondId) -> Option<&SourceCond> {
        self.conds.get(index_of(id.index()))
    }

    /// Where the condition `id` names came from.
    #[must_use]
    pub fn cond_origin(&self, id: CondId) -> Option<&Origin> {
        self.cond_origins.get(index_of(id.index()))
    }

    /// The statement `id` names, or `None` if it names nothing here.
    #[must_use]
    pub fn stmt(&self, id: StmtId) -> Option<&SourceStmt> {
        self.stmts.get(index_of(id.index()))
    }

    /// Where the statement `id` names came from.
    #[must_use]
    pub fn stmt_origin(&self, id: StmtId) -> Option<&Origin> {
        self.stmt_origins.get(index_of(id.index()))
    }

    /// Whether the builder that produced this program exceeded
    /// [`crate::MAX_ARENA_NODES`].
    ///
    /// A program that overflowed is **incomplete** - nodes past the cap were
    /// not recorded - so lowering it refuses rather than emitting a system
    /// that is missing part of the program. Reporting the flag rather than
    /// panicking in the builder is what keeps a frontend fed hostile input
    /// from taking the process down.
    #[must_use]
    pub const fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Every `Unsupported` node in the program, in canonical arena order,
    /// **whether or not anything points at it**.
    ///
    /// # Why a scan and not a traversal, and why it is published
    ///
    /// A traversal reaches only the nodes the control flow can reach *and* that
    /// something points at. Neither is guaranteed. A frontend translating
    /// `return f()` has an expression it must not lose - the call has an unknown
    /// cost - but this fragment's `return` carries no value, so the node it
    /// built may have no parent. Relying on a traversal silently drops it, and a
    /// silently dropped refusal is the truncation `LAN-67` criterion 4 forbids.
    ///
    /// [`crate::lower`] is built on exactly this scan, which is why it can
    /// promise that building an `Unsupported` node anywhere refuses the program.
    /// It is published because a *second* consumer now derives costs from the
    /// structured source directly, and that consumer's obligation is the same
    /// one: it must be able to check that everything it charged is everything
    /// the program contains. Sharing one implementation is what stops the two
    /// answers drifting apart.
    ///
    /// Note the deliberate absence of deduplication - see [`UnsupportedNode`].
    /// Every node is yielded separately, because two identical refusals are two
    /// costs.
    pub fn unsupported_nodes(&self) -> impl Iterator<Item = UnsupportedNode> + '_ {
        let exprs = self.exprs.iter().enumerate().filter_map(|(index, node)| {
            let SourceExpr::Unsupported { construct, detail } = node else {
                return None;
            };
            Some(UnsupportedNode::new(
                NodeId::Expr(ExprId(narrow(index))),
                *construct,
                origin_at(&self.expr_origins, index, &self.origin),
                detail.clone(),
            ))
        });
        let conds = self.conds.iter().enumerate().filter_map(|(index, node)| {
            let SourceCond::Unsupported { construct, detail } = node else {
                return None;
            };
            Some(UnsupportedNode::new(
                NodeId::Cond(CondId(narrow(index))),
                *construct,
                origin_at(&self.cond_origins, index, &self.origin),
                detail.clone(),
            ))
        });
        let stmts = self.stmts.iter().enumerate().filter_map(|(index, node)| {
            let SourceStmt::Unsupported {
                construct, detail, ..
            } = node
            else {
                return None;
            };
            Some(UnsupportedNode::new(
                NodeId::Stmt(StmtId(narrow(index))),
                *construct,
                origin_at(&self.stmt_origins, index, &self.origin),
                detail.clone(),
            ))
        });
        exprs.chain(conds).chain(stmts)
    }

    /// Every variable name the program mentions, read or written, in canonical
    /// order.
    ///
    /// Includes the parameters, whether or not the body mentions them: a
    /// parameter is part of the state on entry even if it is never read.
    /// Used to pick fresh internal names that cannot collide with a
    /// frontend-supplied one, and to declare the emitted system's variable
    /// tuple.
    #[must_use]
    pub fn variables(&self) -> BTreeSet<VarName> {
        let mut names: BTreeSet<VarName> = self.params.iter().cloned().collect();
        for expr in &self.exprs {
            if let SourceExpr::Var { name } = expr {
                names.insert(name.clone());
            }
        }
        for stmt in &self.stmts {
            match stmt {
                SourceStmt::Assign { target, .. } => {
                    names.insert(target.clone());
                }
                SourceStmt::ForRange { target, .. } => {
                    names.insert(target.clone());
                }
                SourceStmt::If { .. }
                | SourceStmt::While { .. }
                | SourceStmt::Return
                | SourceStmt::Unsupported { .. } => {}
            }
        }
        names
    }
}

/// The origin recorded for the node at `index`, or the program's own.
///
/// The two vectors are pushed in lock-step by
/// [`crate::SourceProgramBuilder`], so a missing entry is not reachable; the
/// fallback keeps this total rather than fallible, because a node with no
/// position is still worth reporting at the function's position.
fn origin_at(origins: &[Origin], index: usize, fallback: &Origin) -> Origin {
    origins.get(index).unwrap_or(fallback).clone()
}

/// A `usize` arena index back as the `u32` the handle carries.
///
/// The builder refuses past [`crate::MAX_ARENA_NODES`], which is far below
/// `u32::MAX`, so an index that does not fit cannot have been recorded. The
/// workspace denies truncating casts, and saturating produces a handle every
/// accessor already reports as `None`.
fn narrow(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

/// A `u32` arena index as a `usize`, without an `as` cast.
///
/// The workspace denies `cast_possible_truncation`, and on a 16-bit target
/// this conversion genuinely can truncate. Saturating produces an index that
/// is out of bounds, which every accessor here already reports as `None`.
fn index_of(id: u32) -> usize {
    usize::try_from(id).unwrap_or(usize::MAX)
}
