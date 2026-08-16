//! [`SourceProgram`] - the language-neutral fragment a frontend hands to the
//! lowering.

use std::collections::BTreeSet;

use landav_bound::{Origin, Symbol};

use crate::{
    cond_id::CondId, expr_id::ExprId, source_cond::SourceCond, source_expr::SourceExpr,
    source_stmt::SourceStmt, stmt_id::StmtId, var_name::VarName,
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

/// A `u32` arena index as a `usize`, without an `as` cast.
///
/// The workspace denies `cast_possible_truncation`, and on a 16-bit target
/// this conversion genuinely can truncate. Saturating produces an index that
/// is out of bounds, which every accessor here already reports as `None`.
fn index_of(id: u32) -> usize {
    usize::try_from(id).unwrap_or(usize::MAX)
}
