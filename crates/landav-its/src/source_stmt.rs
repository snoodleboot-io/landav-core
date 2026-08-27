//! [`SourceStmt`] - one node of the fragment's statement language.

use landav_bound::Symbol;

use crate::{
    cond_id::CondId, construct::Construct, declared_effect::DeclaredEffect, expr_id::ExprId,
    extent::Extent, range_spec::RangeSpec, stmt_id::StmtId, var_name::VarName,
};

/// A statement, as one arena node.
///
/// Bodies are `Vec<StmtId>`: a flat list of handles, so the arena stays a flat
/// buffer and dropping a deeply nested program is a linear walk rather than a
/// recursive one.
///
/// # The whole fragment is here
///
/// Eight variants, seven of which do something and one of which refuses. That
/// is the entire statement language this story covers, and the shortness is the
/// point: the crate-level docs justify each inclusion and each exclusion, and
/// a construct that is not in this enum is one a frontend must spell as
/// [`SourceStmt::Unsupported`].
/// Exhaustive on purpose; see [`crate::SourceExpr`].
///
/// # Two variants a consumer may cost but the lowering will not accept
///
/// [`SourceStmt::Raise`] and [`SourceStmt::Protected`] are here so a *cost*
/// consumer can walk into an exception handler's body instead of treating the
/// whole `try` as one opaque region. [`crate::lower`] refuses both: an
/// [`crate::Update`] is a total map with no havoc, so a transition system
/// admitting a `try` would assert the integer state is unchanged across a body
/// that may have run only in part, which is worse than refusing it. The
/// engine's reach and the transition system's are two different numbers, and
/// this is where they differ.
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
    /// Abandon the current computation: one step, and an **edge out of every
    /// region it stands in**.
    ///
    /// Carries no value, for the same reason [`SourceStmt::Return`] does not:
    /// the emitted system models runtime rather than results. A frontend must
    /// still translate the expression beside it, because it may contain a
    /// construct that has to be refused, and the refusals become statements of
    /// their own at the position the expression is evaluated.
    ///
    /// # Why this is not a region
    ///
    /// A `raise` costs one step and assigns to nothing, so charging it as an
    /// unanalysable region would be both looser than necessary and *wrong in
    /// the one way that matters*: a region forgets every value the analysis
    /// knew, and a `raise` in a `try` body would then erase the trip count of a
    /// loop in the handler beside it.
    ///
    /// What it does do is leave early, so a loop containing one may stop before
    /// its counter is exhausted. That is the same shape as a `Return` and a
    /// consumer must treat it the same way: the count still **dominates** the
    /// number of iterations performed, but it is no longer attained, and an
    /// equality claim over it is false.
    Raise,
    /// A body that may be abandoned partway through, with the statements that
    /// run when it is.
    ///
    /// Covers `try`/`except`/`else`/`finally` and the context-manager
    /// statement, whose implicit entry and exit calls a frontend spells as
    /// ordinary refusals inside `body` and `cleanup`.
    ///
    /// # The cost rule, and why it is a sum rather than a maximum
    ///
    /// One execution costs at most `body + handler + cleanup`.
    ///
    /// * `handler` **adds to** `body` rather than replacing it. An exception is
    ///   raised from *inside* the body, so the body's cost up to that point is
    ///   already spent when the handler starts. Reading `except` as though it
    ///   were the `else` arm of an `if` - a maximum over the two - understates
    ///   by the whole of whichever is smaller, without limit as the two grow
    ///   together.
    /// * The maximum over prefixes of the body *is* the whole body, and nothing
    ///   here says **where** the exception fired, so the whole body is the
    ///   honest over-approximation. It is only an over-approximation, which is
    ///   why a consumer must not report the result as an equality.
    /// * `cleanup` is **outside** the choice, because it runs on the normal path
    ///   and the exceptional one alike. Charging it to the exceptional path only
    ///   understates every normal run by the whole of the `finally` body.
    ///
    /// Several `except` clauses concatenate into one `handler`: exactly one of
    /// them runs, so their sum dominates it.
    ///
    /// # The obligation that comes with walking in
    ///
    /// Which statements of `body` ran at all depends on where the exception
    /// hit, so **neither the entry value nor the post-body value of a name the
    /// body writes is the one that holds afterwards**. A consumer that derives
    /// a later loop's trip count from such a name publishes a bound the program
    /// exceeds. There is no expression for the value, so there is no complete
    /// bound; forgetting every name on the way out is the answer.
    Protected {
        /// The statements that may be abandoned partway through.
        ///
        /// A `try`'s `else` clause belongs here, appended: it runs after the
        /// body exactly when the body completed.
        body: Vec<StmtId>,
        /// The statements that run when the body is abandoned; empty when there
        /// is no handler.
        handler: Vec<StmtId>,
        /// The statements that run on **every** path out of the body; empty
        /// when there are none.
        cleanup: Vec<StmtId>,
    },
    /// A statement the frontend could not translate.
    ///
    /// See [`crate::SourceExpr::Unsupported`] for why this is a node rather
    /// than an omission, and [`Extent`] for why a frontend has to say how much
    /// of a source statement this node stands for.
    Unsupported {
        /// What was refused.
        construct: Construct,
        /// Frontend-supplied specifics, if any.
        detail: Option<Symbol>,
        /// Whether this is a whole statement or a fragment of the one beside it.
        extent: Extent,
        /// What the frontend can nevertheless say about this node's cost and
        /// effect, if anything.
        ///
        /// A declared node is a real statement: [`crate::lower`] emits a
        /// transition for it rather than refusing the program. `extent` still
        /// decides whether the source step is charged here or by the statement
        /// beside it. See [`DeclaredEffect`].
        declared: Option<DeclaredEffect>,
    },
}
