//! Generators, and the spec-to-[`SourceProgram`] materialiser.
//!
//! # Loops that terminate by construction
//!
//! The soundness property compares a source run against the emitted system's
//! reachable set, and it can only do that when the source run *finishes*. A
//! generator that emitted arbitrary `while` loops would produce mostly
//! non-terminating programs, the property would skip most of them, and the
//! suite would quietly become vacuous — the failure mode a raised case count
//! exposes and a green CI run hides.
//!
//! So termination is arranged rather than hoped for, and differently for each
//! loop form:
//!
//! * a generated **`while`** counts down a variable that is *only* touched by
//!   its own header and decrement. No generated assignment can name it, so the
//!   trip count is fixed when the loop is entered;
//! * a generated **`for`** terminates whatever the body does, because the
//!   fragment's counted loop is defined to count on a variable the body cannot
//!   reach. That makes `for` the place to let bodies interfere — and the
//!   generator does exactly that, assigning freely to the loop variable and to
//!   the variables in `stop`, because those two are the soundness traps.
//!
//! Every generated program therefore terminates, no case is skipped, and
//! [`vacuity`] measures what the corpus actually contains rather than assuming
//! it.

use std::{collections::BTreeMap, num::NonZeroI64};

use landav_bound::Origin;
use landav_its::{
    ArithOp, CompareOp, CondId, ExprId, RangeSpec, SourceProgram, SourceProgramBuilder, StmtId,
    VarName,
};
use proptest::prelude::*;

use crate::reference::State;

/// The variables a generated assignment may target.
pub const MUTABLE: [&str; 3] = ["a", "b", "c"];

/// The function's parameters.
pub const PARAMS: [&str; 1] = ["n"];

/// Every variable a generated expression may read.
pub const READABLE: [&str; 4] = ["a", "b", "c", "n"];

/// An arithmetic expression, before it has an arena to live in.
#[derive(Debug, Clone)]
pub enum ExprSpec {
    /// A literal.
    Int(i64),
    /// A read of `READABLE[index]`.
    Var(usize),
    /// A binary operation.
    Bin(ArithOp, Box<ExprSpec>, Box<ExprSpec>),
    /// Negation.
    Neg(Box<ExprSpec>),
}

/// A condition, before it has an arena to live in.
#[derive(Debug, Clone)]
pub enum CondSpec {
    /// A comparison.
    Cmp(CompareOp, ExprSpec, ExprSpec),
    /// Conjunction.
    And(Box<CondSpec>, Box<CondSpec>),
    /// Disjunction.
    Or(Box<CondSpec>, Box<CondSpec>),
    /// Negation.
    Not(Box<CondSpec>),
}

/// A statement, before it has an arena to live in.
#[derive(Debug, Clone)]
pub enum StmtSpec {
    /// `MUTABLE[target] = value`.
    Assign {
        /// Which mutable variable.
        target: usize,
        /// What to assign.
        value: ExprSpec,
    },
    /// `if cond: then_body else: else_body`.
    If {
        /// The condition.
        cond: CondSpec,
        /// The consequent.
        then_body: Vec<StmtSpec>,
        /// The alternative.
        else_body: Vec<StmtSpec>,
    },
    /// A countdown loop: `w = trips; while w > 0: body; w = w - 1`.
    ///
    /// `w` is fresh per loop and no generated assignment can name it, so this
    /// runs exactly `max(trips, 0)` times.
    While {
        /// How many iterations.
        trips: i64,
        /// The loop body.
        body: Vec<StmtSpec>,
    },
    /// `for MUTABLE[target] in range(start, stop, step): body`.
    For {
        /// Which mutable variable the loop binds.
        target: usize,
        /// The first value.
        start: ExprSpec,
        /// The exclusive endpoint.
        stop: ExprSpec,
        /// The stride; never zero.
        step: i64,
        /// The loop body.
        body: Vec<StmtSpec>,
    },
    /// Return from the function.
    Return,
}

/// Turns a generated body into a [`SourceProgram`].
pub struct Materialiser {
    builder: SourceProgramBuilder,
    counters: usize,
    origins: usize,
}

impl Materialiser {
    /// A materialiser for a function called `name`.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            builder: SourceProgramBuilder::new(
                name,
                Origin::new(format!("{name}.py:1:1")),
                PARAMS.iter().map(|param| VarName::new(*param)).collect(),
            ),
            counters: 0,
            origins: 0,
        }
    }

    /// The finished program.
    #[must_use]
    pub fn finish(mut self, body: &[StmtSpec]) -> SourceProgram {
        // Every variable is initialised on entry, so that the source
        // interpreter and the emitted system start from the same state and no
        // read is of an unbound name.
        let mut prologue = Vec::new();
        for name in MUTABLE {
            let origin = self.origin();
            let zero = self.builder.int(0, origin.clone());
            prologue.push(self.builder.assign(VarName::new(name), zero, origin));
        }
        let mut statements = prologue;
        for spec in body {
            statements.extend(self.stmt(spec));
        }
        self.builder.build(statements)
    }

    fn origin(&mut self) -> Origin {
        self.origins += 1;
        Origin::new(format!("gen.py:{}:1", self.origins))
    }

    fn expr(&mut self, spec: &ExprSpec) -> ExprId {
        let origin = self.origin();
        match spec {
            ExprSpec::Int(value) => self.builder.int(*value, origin),
            ExprSpec::Var(index) => {
                let name = READABLE[index % READABLE.len()];
                self.builder.var(VarName::new(name), origin)
            }
            ExprSpec::Bin(op, left, right) => {
                let left = self.expr(left);
                let right = self.expr(right);
                self.builder.arith(*op, left, right, origin)
            }
            ExprSpec::Neg(operand) => {
                let operand = self.expr(operand);
                self.builder.neg(operand, origin)
            }
        }
    }

    fn cond(&mut self, spec: &CondSpec) -> CondId {
        let origin = self.origin();
        match spec {
            CondSpec::Cmp(op, left, right) => {
                let left = self.expr(left);
                let right = self.expr(right);
                self.builder.compare(*op, left, right, origin)
            }
            CondSpec::And(left, right) => {
                let left = self.cond(left);
                let right = self.cond(right);
                self.builder.and(left, right, origin)
            }
            CondSpec::Or(left, right) => {
                let left = self.cond(left);
                let right = self.cond(right);
                self.builder.or(left, right, origin)
            }
            CondSpec::Not(operand) => {
                let operand = self.cond(operand);
                self.builder.not(operand, origin)
            }
        }
    }

    fn block(&mut self, specs: &[StmtSpec]) -> Vec<StmtId> {
        specs.iter().flat_map(|spec| self.stmt(spec)).collect()
    }

    fn stmt(&mut self, spec: &StmtSpec) -> Vec<StmtId> {
        match spec {
            StmtSpec::Assign { target, value } => {
                let value = self.expr(value);
                let origin = self.origin();
                let name = MUTABLE[target % MUTABLE.len()];
                vec![self.builder.assign(VarName::new(name), value, origin)]
            }

            StmtSpec::If {
                cond,
                then_body,
                else_body,
            } => {
                let cond = self.cond(cond);
                let then_body = self.block(then_body);
                let else_body = self.block(else_body);
                let origin = self.origin();
                vec![self.builder.if_else(cond, then_body, else_body, origin)]
            }

            StmtSpec::While { trips, body } => {
                self.counters += 1;
                let counter = VarName::new(format!("w{}", self.counters));

                let origin = self.origin();
                let initial = self.builder.int(*trips, origin.clone());
                let setup = self.builder.assign(counter.clone(), initial, origin);

                // `while counter > 0`
                let origin = self.origin();
                let read = self.builder.var(counter.clone(), origin.clone());
                let zero = self.builder.int(0, origin.clone());
                let test = self.builder.compare(CompareOp::Gt, read, zero, origin);

                let mut body = self.block(body);

                // `counter = counter - 1`, which is what makes it terminate.
                let origin = self.origin();
                let read = self.builder.var(counter.clone(), origin.clone());
                let one = self.builder.int(1, origin.clone());
                let decrement = self.builder.arith(ArithOp::Sub, read, one, origin.clone());
                body.push(self.builder.assign(counter, decrement, origin));

                let origin = self.origin();
                vec![setup, self.builder.while_loop(test, body, origin)]
            }

            StmtSpec::For {
                target,
                start,
                stop,
                step,
                body,
            } => {
                let start = self.expr(start);
                let stop = self.expr(stop);
                let body = self.block(body);
                let origin = self.origin();
                let stride = NonZeroI64::new(*step).unwrap_or(NonZeroI64::new(1).unwrap());
                let name = MUTABLE[target % MUTABLE.len()];
                vec![self.builder.for_range(
                    VarName::new(name),
                    RangeSpec::new(start, stop, stride),
                    body,
                    origin,
                )]
            }

            StmtSpec::Return => {
                let origin = self.origin();
                vec![self.builder.return_stmt(origin)]
            }
        }
    }
}

/// What a generated corpus actually contained.
///
/// Counted rather than assumed: a generator that stopped producing loops would
/// leave every soundness property passing and meaningless.
#[derive(Debug, Clone, Copy, Default)]
pub struct Vacuity {
    /// How many programs were examined.
    pub programs: usize,
    /// How many contained at least one loop.
    pub with_loops: usize,
    /// How many contained a loop inside another loop.
    pub with_nested_loops: usize,
    /// How many contained a conditional.
    pub with_conditionals: usize,
}

/// Measures what `specs` contains.
#[must_use]
pub fn vacuity(specs: &[Vec<StmtSpec>]) -> Vacuity {
    let mut measured = Vacuity {
        programs: specs.len(),
        ..Vacuity::default()
    };
    for body in specs {
        if contains_loop(body, 0) {
            measured.with_loops += 1;
        }
        if contains_loop(body, 1) {
            measured.with_nested_loops += 1;
        }
        if contains_conditional(body) {
            measured.with_conditionals += 1;
        }
    }
    measured
}

/// Whether `body` contains a loop nested at least `depth` loops deep.
fn contains_loop(body: &[StmtSpec], depth: usize) -> bool {
    body.iter().any(|spec| match spec {
        StmtSpec::While { body, .. } | StmtSpec::For { body, .. } => {
            if depth == 0 {
                true
            } else {
                contains_loop(body, depth - 1)
            }
        }
        StmtSpec::If {
            then_body,
            else_body,
            ..
        } => contains_loop(then_body, depth) || contains_loop(else_body, depth),
        StmtSpec::Assign { .. } | StmtSpec::Return => false,
    })
}

fn contains_conditional(body: &[StmtSpec]) -> bool {
    body.iter().any(|spec| match spec {
        StmtSpec::If { .. } => true,
        StmtSpec::While { body, .. } | StmtSpec::For { body, .. } => contains_conditional(body),
        StmtSpec::Assign { .. } | StmtSpec::Return => false,
    })
}

// ---------------------------------------------------------------------------
// generators
// ---------------------------------------------------------------------------

/// Small values, so that a generated loop's trip count stays explorable.
fn arb_small() -> impl Strategy<Value = i64> {
    -4_i64..=6
}

/// An arithmetic expression, at most three operators deep.
pub fn arb_expr() -> impl Strategy<Value = ExprSpec> {
    let leaf = prop_oneof![
        arb_small().prop_map(ExprSpec::Int),
        (0_usize..READABLE.len()).prop_map(ExprSpec::Var),
    ];
    leaf.prop_recursive(3, 8, 2, |inner| {
        prop_oneof![
            (
                prop_oneof![Just(ArithOp::Add), Just(ArithOp::Sub), Just(ArithOp::Mul)],
                inner.clone(),
                inner.clone()
            )
                .prop_map(|(op, left, right)| ExprSpec::Bin(
                    op,
                    Box::new(left),
                    Box::new(right)
                )),
            inner.prop_map(|operand| ExprSpec::Neg(Box::new(operand))),
        ]
    })
}

/// An expression simple enough to be a loop endpoint.
///
/// A literal or a single variable. Compound endpoints are excluded on purpose:
/// they make trip counts multiply out, and the property that matters for an
/// endpoint — that it is evaluated once, before the loop — does not need a
/// complicated expression to be exercised.
fn arb_endpoint() -> impl Strategy<Value = ExprSpec> {
    prop_oneof![
        arb_small().prop_map(ExprSpec::Int),
        (0_usize..READABLE.len()).prop_map(ExprSpec::Var),
    ]
}

/// A condition, at most two connectives deep.
pub fn arb_cond() -> impl Strategy<Value = CondSpec> {
    let comparison = (
        prop_oneof![
            Just(CompareOp::Lt),
            Just(CompareOp::Le),
            Just(CompareOp::Gt),
            Just(CompareOp::Ge),
            Just(CompareOp::Eq),
            Just(CompareOp::Ne),
        ],
        arb_expr(),
        arb_expr(),
    )
        .prop_map(|(op, left, right)| CondSpec::Cmp(op, left, right));

    comparison.prop_recursive(2, 6, 2, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone())
                .prop_map(|(left, right)| CondSpec::And(Box::new(left), Box::new(right))),
            (inner.clone(), inner.clone())
                .prop_map(|(left, right)| CondSpec::Or(Box::new(left), Box::new(right))),
            inner.prop_map(|operand| CondSpec::Not(Box::new(operand))),
        ]
    })
}

/// A statement body, with loops nested at most two deep.
pub fn arb_body() -> impl Strategy<Value = Vec<StmtSpec>> {
    let leaf = prop_oneof![
        8 => (0_usize..MUTABLE.len(), arb_expr())
            .prop_map(|(target, value)| StmtSpec::Assign { target, value }),
        1 => Just(StmtSpec::Return),
    ];

    let statement = leaf.prop_recursive(3, 24, 3, |inner| {
        prop_oneof![
            4 => (arb_cond(), prop::collection::vec(inner.clone(), 0..3),
                  prop::collection::vec(inner.clone(), 0..3))
                .prop_map(|(cond, then_body, else_body)| StmtSpec::If {
                    cond, then_body, else_body
                }),
            3 => (0_i64..4, prop::collection::vec(inner.clone(), 0..3))
                .prop_map(|(trips, body)| StmtSpec::While { trips, body }),
            3 => (
                    0_usize..MUTABLE.len(),
                    arb_endpoint(),
                    arb_endpoint(),
                    prop_oneof![Just(1_i64), Just(2), Just(-1), Just(-2)],
                    prop::collection::vec(inner, 0..3),
                )
                .prop_map(|(target, start, stop, step, body)| StmtSpec::For {
                    target, start, stop, step, body
                }),
        ]
    });

    prop::collection::vec(statement, 1..4)
}

/// An initial valuation for the parameters.
pub fn arb_state() -> impl Strategy<Value = State> {
    prop::collection::vec(-4_i128..=6, PARAMS.len()).prop_map(|values| {
        PARAMS
            .iter()
            .zip(values)
            .map(|(name, value)| ((*name).to_owned(), value))
            .collect::<BTreeMap<String, i128>>()
    })
}
