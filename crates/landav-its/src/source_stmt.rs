//! [`SourceStmt`] - one node of the fragment's statement language.

use landav_bound::Symbol;

use crate::{
    cond_id::CondId, construct::Construct, expr_id::ExprId, range_spec::RangeSpec, stmt_id::StmtId,
    var_name::VarName,
};

/// A statement, as one arena node.
///
/// Bodies are `Vec<StmtId>`: a flat list of handles, so the arena stays a flat
/// buffer and dropping a deeply nested program is a linear walk rather than a
/// recursive one.
///
/// # The whole fragment is here
///
/// Six variants, five of which do something and one of which refuses. That is
/// the entire statement language this story covers, and the shortness is the
/// point: the crate-level docs justify each inclusion and each exclusion, and
/// a construct that is not in this enum is one a frontend must spell as
/// [`SourceStmt::Unsupported`].
/// Exhaustive on purpose; see [`crate::SourceExpr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceStmt {
    /// `target = value`, where `target` is a single integer variable.
    ///
    /// Lowers **exactly**, to one transition with a polynomial update.
    /// Compound assignment (`x += e`) is this with the frontend having already
    /// expanded it to `x = x + e`; the expansion is a language fact and stays
    /// on the frontend's side.
    Assign {
        /// The variable assigned.
        target: VarName,
        /// The value assigned.
        value: ExprId,
    },
    /// `if cond: then_body else: else_body`.
    ///
    /// An empty `else_body` is an `if` with no `else`, not a missing branch.
    If {
        /// The condition.
        cond: CondId,
        /// The consequent.
        then_body: Vec<StmtId>,
        /// The alternative; empty when there is none.
        else_body: Vec<StmtId>,
    },
    /// `while cond: body`.
    While {
        /// The loop condition, tested before each iteration.
        cond: CondId,
        /// The loop body.
        body: Vec<StmtId>,
    },
    /// `for target in range(start, stop, step): body`.
    ///
    /// See [`RangeSpec`] for the iteration space and for the two evaluation
    /// facts the lowering has to preserve.
    ForRange {
        /// The variable bound to each value in turn.
        target: VarName,
        /// The iteration space.
        range: RangeSpec,
        /// The loop body.
        body: Vec<StmtId>,
    },
    /// Return from the function.
    ///
    /// Carries no value. The emitted system models **runtime**, not results,
    /// so the returned expression contributes nothing to a transition - but a
    /// frontend must still translate that expression, because it may contain a
    /// construct that has to be refused.
    Return,
    /// A statement the frontend could not translate.
    ///
    /// See [`crate::SourceExpr::Unsupported`] for why this is a node rather
    /// than an omission.
    Unsupported {
        /// What was refused.
        construct: Construct,
        /// Frontend-supplied specifics, if any.
        detail: Option<Symbol>,
    },
}
