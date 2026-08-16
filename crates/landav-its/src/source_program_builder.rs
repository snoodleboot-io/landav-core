//! [`SourceProgramBuilder`] - the only way to build a [`SourceProgram`].

use landav_bound::{Origin, Symbol};

use crate::{
    MAX_ARENA_NODES, arith_op::ArithOp, compare_op::CompareOp, cond_id::CondId,
    construct::Construct, expr_id::ExprId, range_spec::RangeSpec, source_cond::SourceCond,
    source_expr::SourceExpr, source_program::SourceProgram, source_stmt::SourceStmt,
    stmt_id::StmtId, var_name::VarName,
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
        }
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

    /// A statement the frontend could not translate.
    pub fn unsupported_stmt(&mut self, construct: Construct, origin: Origin) -> StmtId {
        self.push_stmt(
            SourceStmt::Unsupported {
                construct,
                detail: None,
            },
            origin,
        )
    }

    /// A statement the frontend could not translate, with specifics.
    pub fn unsupported_stmt_detailed(
        &mut self,
        construct: Construct,
        detail: impl Into<Symbol>,
        origin: Origin,
    ) -> StmtId {
        self.push_stmt(
            SourceStmt::Unsupported {
                construct,
                detail: Some(detail.into()),
            },
            origin,
        )
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
