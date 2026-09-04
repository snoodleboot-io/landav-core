//! [`SourceProgramBuilder`] - the only way to build a [`SourceProgram`].

use std::collections::BTreeSet;

use landav_bound::{Origin, Symbol};

use crate::{
    MAX_ARENA_NODES, arith_op::ArithOp, compare_op::CompareOp, cond_id::CondId,
    construct::Construct, declared_effect::DeclaredEffect, expr_id::ExprId, extent::Extent,
    range_spec::RangeSpec, source_cond::SourceCond, source_expr::SourceExpr,
    source_program::SourceProgram, source_stmt::SourceStmt, stmt_id::StmtId, var_name::VarName,
    writes::Writes,
};

/// Builds a [`SourceProgram`] node by node.
///
/// Every method takes the node's [`Origin`] alongside its content, because a
/// node without a position cannot be blamed and non-negotiable 3 says every
/// failure names its subject *and* where it is. There is deliberately no
/// origin-free convenience constructor: the one thing a frontend is certain to
/// omit if it is optional is exactly the thing a report needs.
///
/// # Never panics, even when overfed
///
/// The arenas are capped at [`MAX_ARENA_NODES`] each. Past the cap the builder
/// stops recording, sets an overflow flag, and keeps returning handles - it
/// does not panic, does not allocate without limit, and does not silently
/// alias one node onto another's index. [`SourceProgram::overflowed`] carries
/// the flag out, and [`crate::lower`] refuses a program that carries it,
/// because a truncated program is exactly the silent-omission failure
/// criterion 4 forbids.
#[derive(Debug, Clone)]
pub struct SourceProgramBuilder {
    name: Symbol,
    params: Vec<VarName>,
    exprs: Vec<SourceExpr>,
    expr_origins: Vec<Origin>,
    conds: Vec<SourceCond>,
    cond_origins: Vec<Origin>,
    stmts: Vec<SourceStmt>,
    stmt_origins: Vec<Origin>,
    origin: Origin,
    overflowed: bool,
    volatile: BTreeSet<VarName>,
}

impl SourceProgramBuilder {
    /// A builder for a function called `name`, declared at `origin`, taking
    /// `params`.
    #[must_use]
    pub fn new(name: impl Into<Symbol>, origin: Origin, params: Vec<VarName>) -> Self {
        Self {
            name: name.into(),
            params,
            exprs: Vec::new(),
            expr_origins: Vec::new(),
            conds: Vec::new(),
            cond_origins: Vec::new(),
            stmts: Vec::new(),
            stmt_origins: Vec::new(),
            origin,
            overflowed: false,
            volatile: BTreeSet::new(),
        }
    }

    /// Records that `name` denotes a property of an object rather than a value.
    ///
    /// See [`SourceProgram::is_volatile`]. Only the frontend knows which of its
    /// variables are of that kind, and a consumer that guessed from the
    /// spelling would be guessing.
    pub fn mark_volatile(&mut self, name: VarName) {
        self.volatile.insert(name);
    }

    // -- expressions --------------------------------------------------------

    /// An integer literal.
    pub fn int(&mut self, value: i64, origin: Origin) -> ExprId {
        self.push_expr(SourceExpr::Int { value }, origin)
    }

    /// A read of an integer variable.
    pub fn var(&mut self, name: VarName, origin: Origin) -> ExprId {
        self.push_expr(SourceExpr::Var { name }, origin)
    }

    /// A binary arithmetic operation.
    pub fn arith(&mut self, op: ArithOp, left: ExprId, right: ExprId, origin: Origin) -> ExprId {
        self.push_expr(SourceExpr::Arith { op, left, right }, origin)
    }

    /// Arithmetic negation.
    pub fn neg(&mut self, operand: ExprId, origin: Origin) -> ExprId {
        self.push_expr(SourceExpr::Neg { operand }, origin)
    }

    /// A power with a literal non-negative exponent.
    pub fn pow(&mut self, base: ExprId, exponent: u32, origin: Origin) -> ExprId {
        self.push_expr(SourceExpr::Pow { base, exponent }, origin)
    }

    /// An expression the frontend could not translate.
    pub fn unsupported_expr(&mut self, construct: Construct, origin: Origin) -> ExprId {
        self.push_expr(
            SourceExpr::Unsupported {
                construct,
                detail: None,
                bounded_by: None,
                evaluates: Vec::new(),
                declared: None,
                writes: Writes::unstated(),
            },
            origin,
        )
    }

    /// An expression the frontend could not translate, with specifics.
    pub fn unsupported_expr_detailed(
        &mut self,
        construct: Construct,
        detail: impl Into<Symbol>,
        origin: Origin,
    ) -> ExprId {
        self.push_expr(
            SourceExpr::Unsupported {
                construct,
                detail: Some(detail.into()),
                bounded_by: None,
                evaluates: Vec::new(),
                declared: None,
                writes: Writes::unstated(),
            },
            origin,
        )
    }

    /// An expression the frontend could not translate, whose magnitude is
    /// dominated by `bounded_by`.
    ///
    /// See [`SourceExpr::Unsupported`]: the named expression stays part of the
    /// program, so a consumer walking it reaches and charges whatever regions
    /// it contains.
    pub fn unsupported_expr_bounded(
        &mut self,
        construct: Construct,
        detail: impl Into<Symbol>,
        bounded_by: ExprId,
        origin: Origin,
    ) -> ExprId {
        self.push_expr(
            SourceExpr::Unsupported {
                construct,
                detail: Some(detail.into()),
                bounded_by: Some(bounded_by),
                evaluates: Vec::new(),
                declared: None,
                writes: Writes::unstated(),
            },
            origin,
        )
    }

    /// An expression the frontend could not translate, which evaluated
    /// `evaluates` on the way in.
    ///
    /// See [`SourceExpr::Unsupported`]: the listed expressions stay part of the
    /// program, so a walk reaches them and charges every region inside them
    /// **at its own position**. This node keeps its own region and its own
    /// hole; nothing here bounds its value.
    pub fn unsupported_expr_evaluating(
        &mut self,
        construct: Construct,
        detail: impl Into<Symbol>,
        evaluates: Vec<ExprId>,
        origin: Origin,
    ) -> ExprId {
        self.push_expr(
            SourceExpr::Unsupported {
                construct,
                detail: Some(detail.into()),
                bounded_by: None,
                evaluates,
                declared: None,
                writes: Writes::unstated(),
            },
            origin,
        )
    }

    /// An expression the frontend did not translate but **can account for**.
    ///
    /// The node is still an [`SourceExpr::Unsupported`] - it has no value this
    /// fragment can write down - but it is no longer a refusal: it costs what
    /// `declared` says and forgets only what `declared` says it may change. See
    /// [`DeclaredEffect`] for what may be declared, and
    /// [`SourceExpr::Unsupported`] for the rule that one of these may only be
    /// built where nothing reads the value.
    ///
    /// `evaluates` carries the same meaning it does on an ordinary refusal, and
    /// carries more weight here: a declared node no longer denotes `omega`, so
    /// it no longer covers for a call written inside it. Whatever the node
    /// evaluated on the way in has to be listed, or it disappears from the
    /// program with nothing standing in for it.
    pub fn declared_expr(
        &mut self,
        construct: Construct,
        detail: impl Into<Symbol>,
        evaluates: Vec<ExprId>,
        declared: DeclaredEffect,
        origin: Origin,
    ) -> ExprId {
        self.push_expr(
            SourceExpr::Unsupported {
                construct,
                detail: Some(detail.into()),
                bounded_by: None,
                evaluates,
                declared: Some(declared),
                writes: Writes::unstated(),
            },
            origin,
        )
    }

    /// An expression the frontend could not translate, saying which locals
    /// evaluating it may rebind.
    ///
    /// The refusal is unchanged - the node is still a hole and still a reason
    /// the program does not lower - and only its *effect* is narrowed. See
    /// [`Writes`] for what a frontend may claim and what it owes for the claim.
    pub fn unsupported_expr_writing(
        &mut self,
        construct: Construct,
        detail: Option<Symbol>,
        writes: Writes,
        origin: Origin,
    ) -> ExprId {
        self.push_expr(
            SourceExpr::Unsupported {
                construct,
                detail,
                bounded_by: None,
                evaluates: Vec::new(),
                declared: None,
                writes,
            },
            origin,
        )
    }

    // -- conditions ---------------------------------------------------------

    /// A comparison of two integer expressions.
    pub fn compare(
        &mut self,
        op: CompareOp,
        left: ExprId,
        right: ExprId,
        origin: Origin,
    ) -> CondId {
        self.push_cond(SourceCond::Compare { op, left, right }, origin)
    }

    /// Conjunction.
    pub fn and(&mut self, left: CondId, right: CondId, origin: Origin) -> CondId {
        self.push_cond(SourceCond::And { left, right }, origin)
    }

    /// Disjunction.
    pub fn or(&mut self, left: CondId, right: CondId, origin: Origin) -> CondId {
        self.push_cond(SourceCond::Or { left, right }, origin)
    }

    /// Negation.
    pub fn not(&mut self, operand: CondId, origin: Origin) -> CondId {
        self.push_cond(SourceCond::Not { operand }, origin)
    }

    /// A condition the frontend could not translate.
    pub fn unsupported_cond(&mut self, construct: Construct, origin: Origin) -> CondId {
        self.push_cond(
            SourceCond::Unsupported {
                construct,
                detail: None,
                declared: None,
                writes: Writes::unstated(),
            },
            origin,
        )
    }

    /// A condition the frontend did not translate but **can account for**.
    ///
    /// The condition counterpart of [`SourceProgramBuilder::declared_expr`].
    /// Unlike the expression form there is nothing to say about a value here -
    /// a condition has none - so what stays unknown is only which branch runs,
    /// and [`crate::lower`] already answers that with "either".
    pub fn declared_cond(
        &mut self,
        construct: Construct,
        detail: impl Into<Symbol>,
        declared: DeclaredEffect,
        origin: Origin,
    ) -> CondId {
        self.push_cond(
            SourceCond::Unsupported {
                construct,
                detail: Some(detail.into()),
                declared: Some(declared),
                writes: Writes::unstated(),
            },
            origin,
        )
    }

    /// A condition the frontend could not translate, with specifics.
    pub fn unsupported_cond_detailed(
        &mut self,
        construct: Construct,
        detail: impl Into<Symbol>,
        origin: Origin,
    ) -> CondId {
        self.push_cond(
            SourceCond::Unsupported {
                construct,
                detail: Some(detail.into()),
                declared: None,
                writes: Writes::unstated(),
            },
            origin,
        )
    }

    /// A condition the frontend could not translate, saying which locals
    /// evaluating it may rebind. The condition counterpart of
    /// [`SourceProgramBuilder::unsupported_expr_writing`].
    pub fn unsupported_cond_writing(
        &mut self,
        construct: Construct,
        detail: Option<Symbol>,
        writes: Writes,
        origin: Origin,
    ) -> CondId {
        self.push_cond(
            SourceCond::Unsupported {
                construct,
                detail,
                declared: None,
                writes,
            },
            origin,
        )
    }

    // -- statements ---------------------------------------------------------

    /// `target = value`.
    pub fn assign(&mut self, target: VarName, value: ExprId, origin: Origin) -> StmtId {
        self.push_stmt(SourceStmt::Assign { target, value }, origin)
    }

    /// `if cond: then_body else: else_body`.
    pub fn if_else(
        &mut self,
        cond: CondId,
        then_body: Vec<StmtId>,
        else_body: Vec<StmtId>,
        origin: Origin,
    ) -> StmtId {
        self.push_stmt(
            SourceStmt::If {
                cond,
                then_body,
                else_body,
            },
            origin,
        )
    }

    /// `while cond: body`.
    pub fn while_loop(&mut self, cond: CondId, body: Vec<StmtId>, origin: Origin) -> StmtId {
        self.push_stmt(SourceStmt::While { cond, body }, origin)
    }

    /// `for target in range(...): body`.
    pub fn for_range(
        &mut self,
        target: VarName,
        range: RangeSpec,
        body: Vec<StmtId>,
        origin: Origin,
    ) -> StmtId {
        self.push_stmt(
            SourceStmt::ForRange {
                target,
                range,
                body,
            },
            origin,
        )
    }

    /// Return from the function.
    pub fn return_stmt(&mut self, origin: Origin) -> StmtId {
        self.push_stmt(SourceStmt::Return, origin)
    }

    /// Abandon the current computation.
    ///
    /// See [`SourceStmt::Raise`]: one step, and an edge out of every region it
    /// stands in.
    pub fn raise_stmt(&mut self, origin: Origin) -> StmtId {
        self.push_stmt(SourceStmt::Raise, origin)
    }

    /// A body that may be abandoned partway through.
    ///
    /// See [`SourceStmt::Protected`] for the cost rule and for the obligation a
    /// consumer takes on by walking into `body`.
    pub fn protected(
        &mut self,
        body: Vec<StmtId>,
        handler: Vec<StmtId>,
        cleanup: Vec<StmtId>,
        origin: Origin,
    ) -> StmtId {
        self.push_stmt(
            SourceStmt::Protected {
                body,
                handler,
                cleanup,
            },
            origin,
        )
    }

    /// A statement the frontend could not translate.
    ///
    /// The node stands for the whole statement, so a consumer that charges one
    /// step per statement charges that step as well as the region. See
    /// [`Extent`] for the case where it should not, and
    /// [`SourceProgramBuilder::unsupported_stmt_with`] for how to say so.
    pub fn unsupported_stmt(&mut self, construct: Construct, origin: Origin) -> StmtId {
        self.unsupported_stmt_with(construct, None, Extent::Statement, origin)
    }

    /// A statement the frontend could not translate, with specifics.
    pub fn unsupported_stmt_detailed(
        &mut self,
        construct: Construct,
        detail: impl Into<Symbol>,
        origin: Origin,
    ) -> StmtId {
        self.unsupported_stmt_with(construct, Some(detail.into()), Extent::Statement, origin)
    }

    /// A statement the frontend could not translate, saying how much of a source
    /// statement it stands for.
    ///
    /// The arena has no expression slot on `Return` and none at all on a refused
    /// assignment, so something unanalysable inside one of those has to become a
    /// statement of its own. [`Extent::Fragment`] is how a frontend says that
    /// this is what happened - the statement it came from is in the arena beside
    /// it, paying its own step, so this node is charged for the region alone.
    pub fn unsupported_stmt_with(
        &mut self,
        construct: Construct,
        detail: Option<Symbol>,
        extent: Extent,
        origin: Origin,
    ) -> StmtId {
        self.unsupported_stmt_writing(construct, detail, extent, Writes::unstated(), origin)
    }

    /// A statement the frontend could not translate, saying which locals it
    /// may rebind.
    ///
    /// The shape of a refused *binding*: `total = <unreadable>` is still
    /// refused, and it rebinds `total` and nothing else, so a value the engine
    /// knew for any other name survives it. See [`Writes`], and `LAN-100` for
    /// the measurement that made this the next thing to build.
    pub fn unsupported_stmt_writing(
        &mut self,
        construct: Construct,
        detail: Option<Symbol>,
        extent: Extent,
        writes: Writes,
        origin: Origin,
    ) -> StmtId {
        self.push_stmt(
            SourceStmt::Unsupported {
                construct,
                detail,
                extent,
                declared: None,
                writes,
            },
            origin,
        )
    }

    /// A statement the frontend did not translate but **can account for**.
    ///
    /// The statement counterpart of [`SourceProgramBuilder::declared_expr`], and
    /// the shape a frontend that hoists a discarded expression's refusals into
    /// statements ends up with. `extent` decides whether the source step is
    /// charged here or by the statement beside it, exactly as it does for a
    /// refusal; `declared` decides everything else.
    pub fn declared_stmt(
        &mut self,
        construct: Construct,
        detail: Option<Symbol>,
        extent: Extent,
        declared: DeclaredEffect,
        origin: Origin,
    ) -> StmtId {
        self.push_stmt(
            SourceStmt::Unsupported {
                construct,
                detail,
                extent,
                declared: Some(declared),
                writes: Writes::unstated(),
            },
            origin,
        )
    }

    /// Records that this program is incomplete for a reason the builder could
    /// not see.
    ///
    /// # The one legitimate caller
    ///
    /// A frontend that translates an expression it will not keep - to record
    /// what the expression refuses, without leaving the nodes dangling in the
    /// program - does that translation into a *separate* builder and hoists the
    /// refusals across. If that separate builder hit [`MAX_ARENA_NODES`], its
    /// refusals are short, and the program the frontend is really building must
    /// carry that fact or [`crate::lower`] would accept a program with a
    /// silently dropped refusal in it.
    ///
    /// There is deliberately no way to clear the flag.
    pub const fn mark_overflowed(&mut self) {
        self.overflowed = true;
    }

    // -- finishing ----------------------------------------------------------

    /// The finished program, with `body` as the function's top-level
    /// statements.
    #[must_use]
    pub fn build(self, body: Vec<StmtId>) -> SourceProgram {
        SourceProgram {
            name: self.name,
            params: self.params,
            exprs: self.exprs,
            expr_origins: self.expr_origins,
            conds: self.conds,
            cond_origins: self.cond_origins,
            stmts: self.stmts,
            stmt_origins: self.stmt_origins,
            body,
            origin: self.origin,
            overflowed: self.overflowed,
            volatile: self.volatile,
        }
    }

    // -- arena plumbing -----------------------------------------------------

    fn push_expr(&mut self, node: SourceExpr, origin: Origin) -> ExprId {
        match self.reserve(self.exprs.len()) {
            Some(index) => {
                self.exprs.push(node);
                self.expr_origins.push(origin);
                ExprId(index)
            }
            None => ExprId(u32::MAX),
        }
    }

    fn push_cond(&mut self, node: SourceCond, origin: Origin) -> CondId {
        match self.reserve(self.conds.len()) {
            Some(index) => {
                self.conds.push(node);
                self.cond_origins.push(origin);
                CondId(index)
            }
            None => CondId(u32::MAX),
        }
    }

    fn push_stmt(&mut self, node: SourceStmt, origin: Origin) -> StmtId {
        match self.reserve(self.stmts.len()) {
            Some(index) => {
                self.stmts.push(node);
                self.stmt_origins.push(origin);
                StmtId(index)
            }
            None => StmtId(u32::MAX),
        }
    }

    /// The index a node at `len` would take, or `None` once the cap is hit.
    ///
    /// Sets the overflow flag on the way past, so that a program built from a
    /// hostile input is *refused* rather than silently short.
    fn reserve(&mut self, len: usize) -> Option<u32> {
        if len >= MAX_ARENA_NODES {
            self.overflowed = true;
            return None;
        }
        match u32::try_from(len) {
            Ok(index) => Some(index),
            Err(_) => {
                self.overflowed = true;
                None
            }
        }
    }
}
