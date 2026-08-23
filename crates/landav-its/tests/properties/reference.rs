//! An **implementation-independent reference semantics**, for both languages.
//!
//! Nothing in this module calls [`landav_its::lower`],
//! [`landav_its::Polynomial::evaluate`], [`landav_its::Guard::holds`] or
//! [`landav_its::Constraint::holds`]. Polynomial evaluation is written out
//! again from the definition — a sum over monomials of a coefficient times a
//! product of powers — so that a property comparing the lowering against this
//! file is comparing an implementation against a specification rather than
//! against itself.
//!
//! # The two semantics
//!
//! [`interpret`] is the **source** semantics: a deterministic operational
//! interpreter for [`SourceProgram`], written from the doc comments on
//! [`landav_its::SourceStmt`] and [`landav_its::RangeSpec`].
//!
//! [`explore`] is the **target** semantics: a nondeterministic explorer for
//! [`Its`], written from the doc comment on [`landav_its::Transition`] —
//! *when the guard holds, the system may move, applying the update
//! simultaneously* — and from [`landav_its::Update`]'s rule that an
//! unmentioned variable is unchanged.
//!
//! The soundness property is that the second admits everything the first does.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use landav_its::{
    ArithOp, CompareOp, CondId, Constraint, ExprId, Its, LocationId, Polynomial, Relation,
    SourceCond, SourceExpr, SourceProgram, SourceStmt, StmtId, Transition,
};

/// A valuation of named integer variables.
///
/// `i128` rather than `i64` so that the reference can distinguish "the program
/// computed a large value" from "the reference overflowed", which an `i64`
/// reference could not do without reproducing the crate's own checked
/// arithmetic.
pub type State = BTreeMap<String, i128>;

/// How a source-level run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// Ran to completion, by falling off the end or by returning.
    Terminated,
    /// Hit the step budget. The program may or may not terminate; the
    /// reference declines to say.
    Exhausted,
    /// Arithmetic left the range of `i128`, or a handle named no node. The
    /// reference declines to say what the program does.
    Undefined,
}

/// The result of interpreting a [`SourceProgram`].
#[derive(Debug, Clone)]
pub struct Run {
    /// The final valuation.
    pub state: State,
    /// How many statements were executed.
    pub steps: u64,
    /// The same run, scored in `landav-engine`'s unit.
    ///
    /// # Why a second number and not a factor of the first
    ///
    /// [`Self::steps`] counts what this interpreter does: one per statement,
    /// including the loop statement itself, and one per loop test including the
    /// final failing one. `landav-engine` counts something deliberately
    /// different - its own documentation defines it as "one per statement
    /// executed, plus one per loop iteration for the loop's own test and
    /// increment" - so a loop costs `count * (1 + body)` there and
    /// `2 + count * (1 + body)` here.
    ///
    /// The gap is not a constant: it compounds with nesting, so no factor
    /// relates the two. Scoring the same run twice is the only way to compare
    /// the engine against a semantics it did not write.
    ///
    /// Written from the engine's doc comment, not from its code. Nothing in
    /// this module calls `landav_engine::cost`, for the same reason nothing in
    /// it calls `lower`.
    pub charged: u64,
    /// How the run ended.
    pub ending: Ending,
}

/// The two counters one run accumulates.
///
/// Threaded together so that a statement cannot be counted in one unit and
/// forgotten in the other.
#[derive(Debug, Clone, Copy, Default)]
struct Tally {
    /// Statements executed, plus one per loop test.
    steps: u64,
    /// The same run in `landav-engine`'s unit. See [`Run::charged`].
    charged: u64,
}

/// Control flow inside the interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Normal,
    Returned,
    Exhausted,
    Undefined,
}

/// Interprets `program` from `initial`, executing at most `budget` statements.
///
/// # The semantics, stated
///
/// * an assignment evaluates its right-hand side in the current state and
///   rebinds one variable;
/// * `if` evaluates the condition and takes exactly one branch;
/// * `while` re-tests the condition before every iteration;
/// * `for` evaluates both endpoints **once**, counts on a variable the body
///   cannot reach, and copies the counter into the loop variable at the start
///   of each iteration — so a body that assigns to the loop variable or to a
///   variable in `stop` does not change the trip count. When the range is
///   empty the loop variable is left as it was.
/// * `return` ends the run; statements after it in the same block do not
///   execute.
#[must_use]
pub fn interpret(program: &SourceProgram, initial: &State, budget: u64) -> Run {
    let mut state = initial.clone();
    let mut tally = Tally::default();
    let flow = run_block(program, program.body(), &mut state, &mut tally, budget);
    Run {
        state,
        steps: tally.steps,
        charged: tally.charged,
        ending: match flow {
            Flow::Normal | Flow::Returned => Ending::Terminated,
            Flow::Exhausted => Ending::Exhausted,
            Flow::Undefined => Ending::Undefined,
        },
    }
}

fn run_block(
    program: &SourceProgram,
    body: &[StmtId],
    state: &mut State,
    tally: &mut Tally,
    budget: u64,
) -> Flow {
    for id in body {
        let flow = run_stmt(program, *id, state, tally, budget);
        if flow != Flow::Normal {
            return flow;
        }
    }
    Flow::Normal
}

fn run_stmt(
    program: &SourceProgram,
    id: StmtId,
    state: &mut State,
    tally: &mut Tally,
    budget: u64,
) -> Flow {
    if tally.steps >= budget {
        return Flow::Exhausted;
    }
    tally.steps += 1;

    let Some(stmt) = program.stmt(id) else {
        return Flow::Undefined;
    };

    match stmt {
        SourceStmt::Assign { target, value } => {
            // One source step, in both units.
            tally.charged += 1;
            match eval(program, *value, state) {
                Some(computed) => {
                    state.insert(target.as_str().to_owned(), computed);
                    Flow::Normal
                }
                None => Flow::Undefined,
            }
        }

        SourceStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            // The test itself, charged once whichever arm is taken.
            tally.charged += 1;
            match decide(program, *cond, state) {
                Some(true) => run_block(program, then_body, state, tally, budget),
                Some(false) => run_block(program, else_body, state, tally, budget),
                None => Flow::Undefined,
            }
        }

        SourceStmt::While { cond, body } => loop {
            if tally.steps >= budget {
                return Flow::Exhausted;
            }
            tally.steps += 1;
            match decide(program, *cond, state) {
                Some(false) => return Flow::Normal,
                None => return Flow::Undefined,
                Some(true) => {}
            }
            // One step of loop overhead per *executed* iteration. The final
            // failing test is not an iteration and costs nothing in the
            // engine's unit; the loop statement itself costs nothing either.
            tally.charged += 1;
            let flow = run_block(program, body, state, tally, budget);
            if flow != Flow::Normal {
                return flow;
            }
        },

        SourceStmt::ForRange {
            target,
            range,
            body,
        } => {
            // Both endpoints, once, in the state before the loop.
            let (Some(mut counter), Some(limit)) = (
                eval(program, range.start, state),
                eval(program, range.stop, state),
            ) else {
                return Flow::Undefined;
            };
            let step = i128::from(range.step.get());
            loop {
                if tally.steps >= budget {
                    return Flow::Exhausted;
                }
                tally.steps += 1;
                let more = if step > 0 {
                    counter < limit
                } else {
                    counter > limit
                };
                if !more {
                    return Flow::Normal;
                }
                // See the `while` arm: one per iteration, none for the loop
                // statement and none for the test that ends it.
                tally.charged += 1;
                state.insert(target.as_str().to_owned(), counter);
                let flow = run_block(program, body, state, tally, budget);
                if flow != Flow::Normal {
                    return flow;
                }
                let Some(next) = counter.checked_add(step) else {
                    return Flow::Undefined;
                };
                counter = next;
            }
        }

        SourceStmt::Return => {
            tally.charged += 1;
            Flow::Returned
        }

        // A refused construct has no source semantics: the reference declines
        // rather than inventing one. Programs containing these are never fed
        // to the soundness property, because `lower` refuses them.
        SourceStmt::Unsupported { .. } => Flow::Undefined,
    }
}

/// The value of an expression, or `None` if it is undefined here.
fn eval(program: &SourceProgram, id: ExprId, state: &State) -> Option<i128> {
    match program.expr(id)? {
        SourceExpr::Int { value } => Some(i128::from(*value)),
        SourceExpr::Var { name } => state.get(name.as_str()).copied(),
        SourceExpr::Arith { op, left, right } => {
            let left = eval(program, *left, state)?;
            let right = eval(program, *right, state)?;
            match op {
                ArithOp::Add => left.checked_add(right),
                ArithOp::Sub => left.checked_sub(right),
                ArithOp::Mul => left.checked_mul(right),
            }
        }
        SourceExpr::Neg { operand } => eval(program, *operand, state)?.checked_neg(),
        SourceExpr::Pow { base, exponent } => {
            let base = eval(program, *base, state)?;
            let mut total: i128 = 1;
            for _ in 0..*exponent {
                total = total.checked_mul(base)?;
            }
            Some(total)
        }
        SourceExpr::Unsupported { .. } => None,
    }
}

/// The truth of a condition, or `None` if it is undefined here.
fn decide(program: &SourceProgram, id: CondId, state: &State) -> Option<bool> {
    match program.cond(id)? {
        SourceCond::Compare { op, left, right } => {
            let left = eval(program, *left, state)?;
            let right = eval(program, *right, state)?;
            // `CompareOp::holds` is a two-line total function on `i128` and is
            // the definition of the operator rather than a decision procedure,
            // so using it here does not make the reference dependent on the
            // lowering. Every path that could *approximate* is written out.
            Some(op.holds(left, right))
        }
        SourceCond::And { left, right } => {
            Some(decide(program, *left, state)? && decide(program, *right, state)?)
        }
        SourceCond::Or { left, right } => {
            Some(decide(program, *left, state)? || decide(program, *right, state)?)
        }
        SourceCond::Not { operand } => Some(!decide(program, *operand, state)?),
        SourceCond::Unsupported { .. } => None,
    }
}

// ---------------------------------------------------------------------------
// the target semantics
// ---------------------------------------------------------------------------

/// One way the system reached its exit location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitRun {
    /// The valuation on arrival.
    pub state: State,
    /// How many transitions were taken.
    pub length: u64,
}

/// What exploring an [`Its`] found.
#[derive(Debug, Clone)]
pub enum Exploration {
    /// The reachable set was enumerated fully.
    Complete {
        /// Every distinct way the exit was reached.
        exits: Vec<ExitRun>,
        /// How many configurations were visited.
        visited: usize,
    },
    /// The budget ran out first; nothing may be concluded.
    Incomplete,
}

impl Exploration {
    /// Whether some run reached the exit in a state agreeing with `expected`
    /// on every variable `expected` names, and took at least `min_length`
    /// transitions.
    #[must_use]
    pub fn admits(&self, expected: &State, min_length: u64) -> bool {
        match self {
            Self::Incomplete => false,
            Self::Complete { exits, .. } => exits.iter().any(|run| {
                run.length >= min_length
                    && expected
                        .iter()
                        .all(|(name, value)| run.state.get(name) == Some(value))
            }),
        }
    }

    /// How many configurations were enumerated.
    ///
    /// Published so a test can assert its exploration was not trivial: an
    /// `admits` check over a system that turned out to have two reachable
    /// configurations proves much less than the assertion looks like it does.
    #[must_use]
    pub fn visited(&self) -> usize {
        match self {
            Self::Incomplete => 0,
            Self::Complete { visited, .. } => *visited,
        }
    }

    /// Every reached exit state, for diagnostics on failure.
    #[must_use]
    pub fn exit_states(&self) -> Vec<&State> {
        match self {
            Self::Incomplete => Vec::new(),
            Self::Complete { exits, .. } => exits.iter().map(|run| &run.state).collect(),
        }
    }
}

/// Enumerates every configuration the system can reach from `initial`.
///
/// Breadth-first over `(location, valuation)` pairs. Variables the caller does
/// not name start at zero — which is deliberate: the lowering's fresh loop
/// counters are set by an initialising transition before they are read, so the
/// property must hold whatever they start at.
#[must_use]
pub fn explore(its: &Its, initial: &State, max_configs: usize, max_length: u64) -> Exploration {
    let mut start_state: State = State::new();
    for var in its.vars() {
        let value = initial.get(var.as_str()).copied().unwrap_or(0);
        start_state.insert(var.as_str().to_owned(), value);
    }

    let mut seen: BTreeSet<(u32, Vec<(String, i128)>)> = BTreeSet::new();
    let mut queue: VecDeque<(LocationId, State, u64)> = VecDeque::new();
    let mut exits: Vec<ExitRun> = Vec::new();
    let mut visited = 0_usize;

    queue.push_back((its.start(), start_state, 0));

    while let Some((location, state, length)) = queue.pop_front() {
        let key = (
            location.index(),
            state
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect::<Vec<_>>(),
        );
        if !seen.insert(key) {
            continue;
        }
        visited += 1;
        if visited > max_configs || length > max_length {
            return Exploration::Incomplete;
        }

        if location == its.exit() {
            exits.push(ExitRun {
                state: state.clone(),
                length,
            });
            // The exit has no outgoing transitions, but continuing rather than
            // `continue`-ing would be equally correct; the loop below simply
            // finds none.
        }

        for transition in its.transitions_from(location) {
            match guard_holds(transition, &state) {
                None => return Exploration::Incomplete,
                Some(false) => continue,
                Some(true) => {}
            }
            let Some(next) = apply(transition, &state) else {
                return Exploration::Incomplete;
            };
            queue.push_back((transition.target(), next, length.saturating_add(1)));
        }
    }

    Exploration::Complete { exits, visited }
}

/// What simulating an [`Its`] found.
///
/// # Why simulation, when the system is nondeterministic
///
/// An integer transition system may branch, and [`explore`] handles the
/// general case. But on the *exact* fragment the emitted system should not
/// branch at all: `if`'s two guards are the two polarities of one condition
/// and so partition the states, a loop header's continue and exit guards
/// likewise, and everything else has a single successor. Simulation asserts
/// that, and asserting it is worth more than tolerating it — a lowering whose
/// branch guards overlapped, or left a gap, would still pass an
/// over-approximation check while being visibly wrong.
///
/// Several transitions leading to the **same** successor configuration count
/// as one. That is not a fudge: `a > 0 or a > 1` legitimately produces two
/// normal-form clauses that both hold at `a = 2`, and they differ only in
/// which guard admitted them, not in where they go or what they do.
#[derive(Debug, Clone)]
pub enum Simulation {
    /// Reached the exit location.
    Terminated {
        /// The valuation on arrival.
        state: State,
        /// How many transitions were taken.
        length: u64,
    },
    /// More than one distinct successor was available.
    Nondeterministic {
        /// Where the branch was.
        location: u32,
        /// How many distinct successors there were.
        options: usize,
    },
    /// No transition was enabled, and this is not the exit.
    Stuck {
        /// Where the system stopped.
        location: u32,
    },
    /// The transition budget ran out.
    Budget,
    /// A guard or an update could not be evaluated.
    Undefined,
}

/// Runs the system, requiring a unique successor at every step.
#[must_use]
pub fn simulate(its: &Its, initial: &State, budget: u64) -> Simulation {
    let mut state: State = State::new();
    for var in its.vars() {
        state.insert(
            var.as_str().to_owned(),
            initial.get(var.as_str()).copied().unwrap_or(0),
        );
    }

    let mut location = its.start();
    let mut length = 0_u64;

    loop {
        if location == its.exit() {
            return Simulation::Terminated { state, length };
        }
        if length >= budget {
            return Simulation::Budget;
        }

        let mut successors: Vec<(LocationId, State)> = Vec::new();
        for transition in its.transitions_from(location) {
            match guard_holds(transition, &state) {
                None => return Simulation::Undefined,
                Some(false) => continue,
                Some(true) => {}
            }
            let Some(next) = apply(transition, &state) else {
                return Simulation::Undefined;
            };
            let candidate = (transition.target(), next);
            if !successors.contains(&candidate) {
                successors.push(candidate);
            }
        }

        match successors.len() {
            0 => {
                return Simulation::Stuck {
                    location: location.index(),
                };
            }
            1 => {
                // `len` is 1, so `pop` yields the only element.
                let Some((target, next)) = successors.pop() else {
                    return Simulation::Undefined;
                };
                location = target;
                state = next;
                length += 1;
            }
            options => {
                return Simulation::Nondeterministic {
                    location: location.index(),
                    options,
                };
            }
        }
    }
}

/// Whether every conjunct of the transition's guard holds.
fn guard_holds(transition: &Transition, state: &State) -> Option<bool> {
    let mut all = true;
    for constraint in transition.guard().constraints() {
        all &= constraint_holds(constraint, state)?;
    }
    Some(all)
}

/// Whether one constraint holds — written from the definition of
/// [`Relation`], not by calling the crate's own decision procedure.
fn constraint_holds(constraint: &Constraint, state: &State) -> Option<bool> {
    let value = evaluate(constraint.polynomial(), state)?;
    Some(match constraint.relation() {
        Relation::Ge => value >= 0,
        Relation::Gt => value > 0,
        Relation::Eq => value == 0,
    })
}

/// The simultaneous post-state of a transition.
///
/// Every right-hand side is evaluated in `state`, and only then are the
/// results installed — so `{x := y, y := x}` swaps. A variable the update does
/// not mention is copied across unchanged.
fn apply(transition: &Transition, state: &State) -> Option<State> {
    let mut next = state.clone();
    let mut computed: Vec<(String, i128)> = Vec::new();
    for (var, polynomial) in transition.update().assignments() {
        computed.push((var.as_str().to_owned(), evaluate(polynomial, state)?));
    }
    for (name, value) in computed {
        next.insert(name, value);
    }
    Some(next)
}

/// A polynomial's value: the sum over monomials of a coefficient times a
/// product of powers.
///
/// Written out from the definition. This is the function whose independence
/// from [`Polynomial::evaluate`] the whole suite rests on, which is why it is
/// eight lines of arithmetic and no method calls beyond the accessors.
#[must_use]
pub fn evaluate(polynomial: &Polynomial, state: &State) -> Option<i128> {
    let mut total: i128 = 0;
    for monomial in polynomial.monomials() {
        let mut term = i128::from(monomial.coefficient());
        for (var, exponent) in monomial.powers() {
            let value = *state.get(var.as_str())?;
            for _ in 0..*exponent {
                term = term.checked_mul(value)?;
            }
        }
        total = total.checked_add(term)?;
    }
    Some(total)
}

/// Every comparison the reference can express, for the exhaustiveness test.
#[must_use]
pub const fn all_comparisons() -> [CompareOp; 6] {
    [
        CompareOp::Lt,
        CompareOp::Le,
        CompareOp::Gt,
        CompareOp::Ge,
        CompareOp::Eq,
        CompareOp::Ne,
    ]
}
