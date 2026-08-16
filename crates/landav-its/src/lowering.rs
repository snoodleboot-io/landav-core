//! [`lower`] - the lowering from the numeric fragment to an integer
//! transition system.
//!
//! # Everything here is worklist-driven
//!
//! Non-negotiable 2 forbids a panic in library code, and this crate is fed by
//! a frontend that reads untrusted source. A recursive traversal turns a
//! deeply nested input into a stack overflow, which is an *abort* - strictly
//! worse than a panic, because it takes the blame path with it and cannot be
//! caught. Every traversal below therefore carries its own explicit stack:
//! statements in `Lowering::run`, expressions in `Lowering::expr_poly`,
//! conditions in `Lowering::cond_dnf`. There is no recursion, and the arena
//! representation of [`SourceProgram`] means there is no deep `Drop` either.
//!
//! # Termination
//!
//! Three facts, each cheap and each checked rather than assumed:
//!
//! * a statement is lowered **at most once** - a repeat means the same node is
//!   reachable from two places in the tree, which is a malformed program;
//! * an expression or condition child always has a **smaller arena index**
//!   than its parent, which the builder guarantees and the traversal verifies,
//!   so neither arena can contain a cycle;
//! * every expression and condition is memoised, so a shared subterm used `k`
//!   times costs one evaluation rather than `k` - without which a
//!   frontend-built DAG of `n` nodes could cost `2^n`.

use std::collections::{BTreeSet, HashSet};

use landav_bound::{Origin, Symbol};

use crate::{
    MAX_DNF_CLAUSES, arith_op::ArithOp, compare_op::CompareOp, cond_id::CondId,
    constraint::Constraint, construct::Construct, expr_id::ExprId, guard::Guard, its::Its,
    its_var::ItsVar, location::Location, location_id::LocationId, lowering_error::LoweringError,
    polynomial::Polynomial, refusals::Refusals, relation::Relation, source_cond::SourceCond,
    source_expr::SourceExpr, source_program::SourceProgram, source_stmt::SourceStmt,
    stmt_id::StmtId, transition::Transition, unsupported::Unsupported, update::Update,
    var_name::VarName,
};

/// A condition in disjunctive normal form: a disjunction of conjunctions.
///
/// The empty disjunction is `false` and admits no transition; a disjunction
/// containing the empty conjunction is `true` and admits an unguarded one.
type Dnf = Vec<Vec<Constraint>>;

/// Lowers one function's numeric fragment to an integer transition system.
///
/// # Errors
///
/// [`LoweringError::Refused`] if the program contains any construct outside
/// the fragment - listing **every** such construct, not merely the first - and
/// [`LoweringError::Malformed`] if the program is internally inconsistent.
/// Never a partial system: see [`LoweringError`] for why that is the only
/// sound answer.
pub fn lower(program: &SourceProgram) -> Result<Its, LoweringError> {
    Lowering::new(program).run()
}

/// One unit of statement work: a block, and the two locations it spans.
struct Job {
    body: Vec<StmtId>,
    entry: LocationId,
    exit: LocationId,
}

/// The lowering's mutable state.
struct Lowering<'a> {
    program: &'a SourceProgram,
    locations: Vec<Location>,
    transitions: Vec<Transition>,
    expr_memo: Vec<Option<Polynomial>>,
    cond_memo: Vec<Option<(Dnf, Dnf)>>,
    visited_stmts: HashSet<u32>,
    used_names: BTreeSet<String>,
    fresh_counter: u64,
    refusals: Option<Refusals>,
    malformed: Option<Symbol>,
    exit: LocationId,
}

impl<'a> Lowering<'a> {
    fn new(program: &'a SourceProgram) -> Self {
        Self {
            program,
            locations: Vec::new(),
            transitions: Vec::new(),
            expr_memo: vec![None; program.exprs.len()],
            cond_memo: vec![None; program.conds.len()],
            visited_stmts: HashSet::new(),
            used_names: program
                .variables()
                .iter()
                .map(|name| name.as_str().to_owned())
                .collect(),
            fresh_counter: 0,
            refusals: None,
            malformed: None,
            // Replaced in `run`; `Lowering` is private and never observed
            // before that happens.
            exit: LocationId(0),
        }
    }

    fn run(mut self) -> Result<Its, LoweringError> {
        let program = self.program;

        if program.overflowed() {
            return Err(self.malformed_error("source arena exceeded MAX_ARENA_NODES"));
        }

        let start = self.new_location("entry");
        let exit = self.new_location("exit");
        self.exit = exit;

        let mut work = vec![Job {
            body: program.body().to_vec(),
            entry: start,
            exit,
        }];

        while let Some(job) = work.pop() {
            if job.body.is_empty() {
                self.emit(
                    job.entry,
                    job.exit,
                    Guard::always(),
                    Update::identity(),
                    program.origin().clone(),
                );
                continue;
            }
            let mut current = job.entry;
            let last = job.body.len().saturating_sub(1);
            for (index, stmt) in job.body.iter().enumerate() {
                let next = if index == last {
                    job.exit
                } else {
                    self.new_location("seq")
                };
                self.lower_stmt(*stmt, current, next, &mut work);
                current = next;
            }
        }

        self.refuse_every_unsupported_node();

        if let Some(detail) = self.malformed.take() {
            return Err(LoweringError::Malformed {
                function: program.name().clone(),
                detail,
            });
        }
        if let Some(refusals) = self.refusals.take() {
            return Err(LoweringError::Refused {
                function: program.name().clone(),
                refusals,
            });
        }

        let mut vars: BTreeSet<ItsVar> = program.variables().iter().map(ItsVar::from).collect();
        for name in &self.used_names {
            vars.insert(ItsVar::new(name.as_str()));
        }
        let params: Vec<ItsVar> = program.params().iter().map(ItsVar::from).collect();

        Ok(Its {
            name: program.name().clone(),
            origin: program.origin().clone(),
            vars: vars.into_iter().collect(),
            params,
            start,
            exit,
            locations: self.locations,
            transitions: self.transitions,
        })
    }

    /// Refuses every `Unsupported` node in the program, reachable or not.
    ///
    /// # Why a scan and not the traversal
    ///
    /// The traversal only reaches nodes the control flow can reach *and* that
    /// something points at. Neither is guaranteed. A frontend translating
    /// `return f()` has an expression it must not lose — the call has an
    /// unknown cost — but this fragment's `return` carries no value, so the
    /// node it built has no parent. Relying on the traversal would silently
    /// drop it, and a silently dropped refusal is exactly the truncation
    /// `LAN-67` criterion 4 forbids: the program would lower cleanly and the
    /// derived bound would omit the call.
    ///
    /// Scanning the arenas makes that structurally impossible. Building an
    /// `Unsupported` node **anywhere**, attached or not, refuses the program.
    /// A frontend therefore cannot lose a refusal by forgetting to hang a node
    /// off something, which is the easiest mistake in the whole translation to
    /// make and the hardest to notice.
    fn refuse_every_unsupported_node(&mut self) {
        let program = self.program;

        for (index, node) in program.exprs.iter().enumerate() {
            if let SourceExpr::Unsupported { construct, detail } = node {
                let origin = origin_at(&program.expr_origins, index, program.origin());
                self.refuse(*construct, origin, detail.clone());
            }
        }
        for (index, node) in program.conds.iter().enumerate() {
            if let SourceCond::Unsupported { construct, detail } = node {
                let origin = origin_at(&program.cond_origins, index, program.origin());
                self.refuse(*construct, origin, detail.clone());
            }
        }
        for (index, node) in program.stmts.iter().enumerate() {
            if let SourceStmt::Unsupported { construct, detail } = node {
                let origin = origin_at(&program.stmt_origins, index, program.origin());
                self.refuse(*construct, origin, detail.clone());
            }
        }
    }

    // -- statements ---------------------------------------------------------

    fn lower_stmt(&mut self, id: StmtId, from: LocationId, to: LocationId, work: &mut Vec<Job>) {
        if !self.visited_stmts.insert(id.index()) {
            self.mark_malformed("a statement is reachable from two places in the program");
            return;
        }
        let program = self.program;
        let Some(stmt) = program.stmt(id) else {
            self.mark_malformed("statement handle names no node");
            return;
        };
        let origin = program
            .stmt_origin(id)
            .cloned()
            .unwrap_or_else(|| program.origin().clone());

        match stmt {
            SourceStmt::Assign { target, value } => {
                let polynomial = self.expr_poly(*value);
                self.emit(
                    from,
                    to,
                    Guard::always(),
                    Update::new([(ItsVar::from(target), polynomial)]),
                    origin,
                );
            }

            SourceStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let (positive, negative) = self.cond_dnf(*cond);
                let then_entry = self.new_location("if.then");
                let else_entry = self.new_location("if.else");
                self.emit_dnf(&positive, from, then_entry, &Update::identity(), &origin);
                self.emit_dnf(&negative, from, else_entry, &Update::identity(), &origin);
                self.push_job(work, id, then_body, then_entry, to);
                self.push_job(work, id, else_body, else_entry, to);
            }

            SourceStmt::While { cond, body } => {
                let head = self.new_location("while.head");
                self.emit(
                    from,
                    head,
                    Guard::always(),
                    Update::identity(),
                    origin.clone(),
                );
                let (positive, negative) = self.cond_dnf(*cond);
                let body_entry = self.new_location("while.body");
                self.emit_dnf(&positive, head, body_entry, &Update::identity(), &origin);
                self.emit_dnf(&negative, head, to, &Update::identity(), &origin);
                // The body's exit is the head, which is what makes the loop a
                // loop: the back edge is the block's fall-through.
                self.push_job(work, id, body, body_entry, head);
            }

            SourceStmt::ForRange {
                target,
                range,
                body,
            } => {
                self.lower_for_range(id, target, *range, body, from, to, &origin, work);
            }

            SourceStmt::Return => {
                // To the function's single exit, never to `to`: the statements
                // that follow in this block are unreachable, and leaving them
                // without an incoming edge is exactly how that is expressed.
                // They are still traversed, so a refusal inside them is still
                // reported.
                let exit = self.exit;
                self.emit(from, exit, Guard::always(), Update::identity(), origin);
            }

            // Recorded by `refuse_every_unsupported_node`, not here. Doing it
            // in both places would leave one of them dead, and dead code that
            // looks load bearing is how a real gap gets overlooked.
            SourceStmt::Unsupported { .. } => {}
        }
    }

    /// Desugars `for target in range(start, stop, step)` into a counted loop.
    ///
    /// Two facts about the source semantics have to survive, and each one is a
    /// soundness bug in both directions if it does not:
    ///
    /// * **the endpoints are evaluated once.** A body that assigns to a
    ///   variable appearing in `stop` must not change the trip count, so
    ///   `stop` is snapshotted into a fresh variable on the way in.
    /// * **the loop variable is not the counter.** A body that assigns to
    ///   `target` must not change the trip count either, so the loop counts on
    ///   a fresh variable the source cannot name and copies it into `target`
    ///   on entry to each iteration.
    ///
    /// Both are exact, not approximations.
    #[expect(
        clippy::too_many_arguments,
        reason = "each argument is a distinct part of the desugaring; bundling them into a \
                  struct would move the same seven values one indirection away without making \
                  any of them optional"
    )]
    fn lower_for_range(
        &mut self,
        id: StmtId,
        target: &VarName,
        range: crate::range_spec::RangeSpec,
        body: &[StmtId],
        from: LocationId,
        to: LocationId,
        origin: &Origin,
        work: &mut Vec<Job>,
    ) {
        let start = self.expr_poly(range.start);
        let stop = self.expr_poly(range.stop);

        let counter = self.fresh_var("for.counter");
        let limit = self.fresh_var("for.limit");

        let head = self.new_location("for.head");
        let body_entry = self.new_location("for.body");
        let latch = self.new_location("for.latch");

        // Both endpoints evaluated in the pre-state, simultaneously.
        self.emit(
            from,
            head,
            Guard::always(),
            Update::new([(counter.clone(), start), (limit.clone(), stop)]),
            origin.clone(),
        );

        // `progress > 0` is "there is another iteration", in whichever
        // direction the step travels.
        let counter_poly = Polynomial::var(counter.clone());
        let limit_poly = Polynomial::var(limit);
        let progress = if range.ascending() {
            self.checked(limit_poly.sub(&counter_poly), origin)
        } else {
            self.checked(counter_poly.sub(&limit_poly), origin)
        };
        let exhausted = self.checked(progress.negate(), origin);

        self.emit(
            head,
            body_entry,
            Guard::new([Constraint::new(progress, Relation::Gt)]),
            Update::new([(ItsVar::from(target), counter_poly.clone())]),
            origin.clone(),
        );
        self.emit(
            head,
            to,
            Guard::new([Constraint::new(exhausted, Relation::Ge)]),
            Update::identity(),
            origin.clone(),
        );

        let stride = Polynomial::constant(range.step.get());
        let advanced = self.checked(counter_poly.add(&stride), origin);
        self.emit(
            latch,
            head,
            Guard::always(),
            Update::new([(counter, advanced)]),
            origin.clone(),
        );

        self.push_job(work, id, body, body_entry, latch);
    }

    /// Queues a nested block, checking the acyclicity invariant first.
    fn push_job(
        &mut self,
        work: &mut Vec<Job>,
        parent: StmtId,
        body: &[StmtId],
        entry: LocationId,
        exit: LocationId,
    ) {
        for child in body {
            if child.index() >= parent.index() {
                self.mark_malformed("a statement body contains itself or a later statement");
                return;
            }
        }
        work.push(Job {
            body: body.to_vec(),
            entry,
            exit,
        });
    }

    // -- expressions --------------------------------------------------------

    /// The polynomial an expression denotes.
    ///
    /// **Exact**: every node of the expression language is a polynomial
    /// operation, so nothing here approximates. The only ways to fail are a
    /// refused construct, an overflowing coefficient, or a size cap, and all
    /// three record a refusal and return zero - the returned value is then
    /// meaningless, which is safe because a lowering that has recorded a
    /// refusal cannot return an `Its`.
    fn expr_poly(&mut self, root: ExprId) -> Polynomial {
        enum Task {
            Visit(ExprId),
            Build(ExprId),
        }

        let program = self.program;
        let mut stack = vec![Task::Visit(root)];

        while let Some(task) = stack.pop() {
            match task {
                Task::Visit(id) => {
                    let Some(slot) = self.expr_slot(id) else {
                        return Polynomial::zero();
                    };
                    if self.expr_memo.get(slot).is_some_and(Option::is_some) {
                        continue;
                    }
                    let Some(node) = program.expr(id) else {
                        self.mark_malformed("expression handle names no node");
                        return Polynomial::zero();
                    };
                    match node {
                        SourceExpr::Int { value } => {
                            self.store_expr(slot, Polynomial::constant(*value));
                        }
                        SourceExpr::Var { name } => {
                            self.store_expr(slot, Polynomial::var(ItsVar::from(name)));
                        }
                        SourceExpr::Arith { left, right, .. } => {
                            if !self.expr_child_ok(id, *left) || !self.expr_child_ok(id, *right) {
                                return Polynomial::zero();
                            }
                            stack.push(Task::Build(id));
                            stack.push(Task::Visit(*left));
                            stack.push(Task::Visit(*right));
                        }
                        SourceExpr::Neg { operand } => {
                            if !self.expr_child_ok(id, *operand) {
                                return Polynomial::zero();
                            }
                            stack.push(Task::Build(id));
                            stack.push(Task::Visit(*operand));
                        }
                        SourceExpr::Pow { base, .. } => {
                            if !self.expr_child_ok(id, *base) {
                                return Polynomial::zero();
                            }
                            stack.push(Task::Build(id));
                            stack.push(Task::Visit(*base));
                        }
                        // Refused by `refuse_every_unsupported_node`. Zero is
                        // a placeholder that keeps the traversal total; it is
                        // never observed, because a program with a refusal in
                        // it never yields a system.
                        SourceExpr::Unsupported { .. } => {
                            self.store_expr(slot, Polynomial::zero());
                        }
                    }
                }
                Task::Build(id) => {
                    let Some(slot) = self.expr_slot(id) else {
                        return Polynomial::zero();
                    };
                    let Some(node) = program.expr(id) else {
                        self.mark_malformed("expression handle names no node");
                        return Polynomial::zero();
                    };
                    let origin = self.expr_origin(id);
                    let built = match node {
                        SourceExpr::Arith { op, left, right } => {
                            let left = self.recall_expr(*left);
                            let right = self.recall_expr(*right);
                            let combined = match op {
                                ArithOp::Add => left.add(&right),
                                ArithOp::Sub => left.sub(&right),
                                ArithOp::Mul => left.multiply(&right),
                            };
                            self.checked(combined, &origin)
                        }
                        SourceExpr::Neg { operand } => {
                            let operand = self.recall_expr(*operand);
                            self.checked(operand.negate(), &origin)
                        }
                        SourceExpr::Pow { base, exponent } => {
                            let base = self.recall_expr(*base);
                            self.checked(base.power(*exponent), &origin)
                        }
                        SourceExpr::Int { .. }
                        | SourceExpr::Var { .. }
                        | SourceExpr::Unsupported { .. } => Polynomial::zero(),
                    };
                    self.store_expr(slot, built);
                }
            }
        }

        self.recall_expr(root)
    }

    // -- conditions ---------------------------------------------------------

    /// A condition in disjunctive normal form, in both polarities.
    ///
    /// Returns `(positive, negative)`: the transitions to emit when the
    /// condition holds, and when it does not.
    ///
    /// # Both polarities are computed, not derived from one another
    ///
    /// Negation is pushed to the leaves rather than applied to a finished
    /// normal form, which is what keeps both sides exact - the negation of a
    /// comparison is another comparison, with no disjunction introduced except
    /// by `==` and `!=`, which introduce it honestly.
    ///
    /// It is also what makes the size cap safe. When a normal form exceeds
    /// [`MAX_DNF_CLAUSES`] it is replaced by `true`, which **widens** that
    /// polarity: more states satisfy it, so more executions are admitted. Had
    /// the negative side been derived by negating the positive one, widening
    /// the positive side would have *narrowed* the negative side into an
    /// under-approximation, and a branch the program can take would have been
    /// dropped. Computing the two independently means each is widened on its
    /// own and neither can be narrowed.
    fn cond_dnf(&mut self, root: CondId) -> (Dnf, Dnf) {
        enum Task {
            Visit(CondId),
            Build(CondId),
        }

        let program = self.program;
        let mut stack = vec![Task::Visit(root)];

        while let Some(task) = stack.pop() {
            match task {
                Task::Visit(id) => {
                    let Some(slot) = self.cond_slot(id) else {
                        return (dnf_true(), dnf_true());
                    };
                    if self.cond_memo.get(slot).is_some_and(Option::is_some) {
                        continue;
                    }
                    let Some(node) = program.cond(id) else {
                        self.mark_malformed("condition handle names no node");
                        return (dnf_true(), dnf_true());
                    };
                    match node {
                        SourceCond::Compare { op, left, right } => {
                            let left = self.expr_poly(*left);
                            let right = self.expr_poly(*right);
                            let origin = self.cond_origin(id);
                            let built = self.compare_dnf(*op, &left, &right, &origin);
                            self.store_cond(slot, built);
                        }
                        SourceCond::And { left, right } | SourceCond::Or { left, right } => {
                            if !self.cond_child_ok(id, *left) || !self.cond_child_ok(id, *right) {
                                return (dnf_true(), dnf_true());
                            }
                            stack.push(Task::Build(id));
                            stack.push(Task::Visit(*left));
                            stack.push(Task::Visit(*right));
                        }
                        SourceCond::Not { operand } => {
                            if !self.cond_child_ok(id, *operand) {
                                return (dnf_true(), dnf_true());
                            }
                            stack.push(Task::Build(id));
                            stack.push(Task::Visit(*operand));
                        }
                        // As above: refused by the arena scan.
                        SourceCond::Unsupported { .. } => {
                            self.store_cond(slot, (dnf_true(), dnf_true()));
                        }
                    }
                }
                Task::Build(id) => {
                    let Some(slot) = self.cond_slot(id) else {
                        return (dnf_true(), dnf_true());
                    };
                    let Some(node) = program.cond(id) else {
                        self.mark_malformed("condition handle names no node");
                        return (dnf_true(), dnf_true());
                    };
                    let built = match node {
                        SourceCond::And { left, right } => {
                            let (left_pos, left_neg) = self.recall_cond(*left);
                            let (right_pos, right_neg) = self.recall_cond(*right);
                            (cross(&left_pos, &right_pos), disjoin(&left_neg, &right_neg))
                        }
                        SourceCond::Or { left, right } => {
                            let (left_pos, left_neg) = self.recall_cond(*left);
                            let (right_pos, right_neg) = self.recall_cond(*right);
                            (disjoin(&left_pos, &right_pos), cross(&left_neg, &right_neg))
                        }
                        SourceCond::Not { operand } => {
                            let (positive, negative) = self.recall_cond(*operand);
                            (negative, positive)
                        }
                        SourceCond::Compare { .. } | SourceCond::Unsupported { .. } => {
                            (dnf_true(), dnf_true())
                        }
                    };
                    self.store_cond(slot, built);
                }
            }
        }

        self.recall_cond(root)
    }

    /// The two normal forms of one comparison. Exact in both polarities.
    fn compare_dnf(
        &mut self,
        op: CompareOp,
        left: &Polynomial,
        right: &Polynomial,
        origin: &Origin,
    ) -> (Dnf, Dnf) {
        // `left - right` and `right - left`; over the integers every
        // comparison is one of these against zero.
        let forward = self.checked(left.sub(right), origin);
        let backward = self.checked(right.sub(left), origin);

        let ge =
            |polynomial: &Polynomial| vec![vec![Constraint::new(polynomial.clone(), Relation::Ge)]];
        let gt =
            |polynomial: &Polynomial| vec![vec![Constraint::new(polynomial.clone(), Relation::Gt)]];
        let eq = vec![vec![Constraint::new(forward.clone(), Relation::Eq)]];
        // `p != 0` is a genuine disjunction: `p > 0 \/ -p > 0`.
        let ne = vec![
            vec![Constraint::new(forward.clone(), Relation::Gt)],
            vec![Constraint::new(backward.clone(), Relation::Gt)],
        ];

        match op {
            CompareOp::Lt => (gt(&backward), ge(&forward)),
            CompareOp::Le => (ge(&backward), gt(&forward)),
            CompareOp::Gt => (gt(&forward), ge(&backward)),
            CompareOp::Ge => (ge(&forward), gt(&backward)),
            CompareOp::Eq => (eq, ne),
            CompareOp::Ne => (ne, eq),
        }
    }

    // -- emission -----------------------------------------------------------

    fn emit(
        &mut self,
        from: LocationId,
        to: LocationId,
        guard: Guard,
        update: Update,
        origin: Origin,
    ) {
        self.transitions
            .push(Transition::new(from, to, guard, update, origin));
    }

    /// One transition per clause of `dnf`.
    ///
    /// Clauses that are unsatisfiable on their face are dropped. That removes
    /// transitions no execution could take, so it cannot remove an execution -
    /// it is the one place this lowering discards something, and it discards
    /// only the empty.
    fn emit_dnf(
        &mut self,
        dnf: &Dnf,
        from: LocationId,
        to: LocationId,
        update: &Update,
        origin: &Origin,
    ) {
        for clause in dnf {
            let guard = Guard::new(clause.iter().cloned());
            if guard.is_trivially_unsatisfiable() {
                continue;
            }
            self.emit(from, to, guard, update.clone(), origin.clone());
        }
    }

    fn new_location(&mut self, label: &str) -> LocationId {
        match u32::try_from(self.locations.len()) {
            Ok(index) => {
                let id = LocationId(index);
                self.locations.push(Location::new(id, label));
                id
            }
            Err(_) => {
                self.mark_malformed("more control locations than can be numbered");
                LocationId(u32::MAX)
            }
        }
    }

    /// A variable name no source variable can collide with.
    fn fresh_var(&mut self, base: &str) -> ItsVar {
        // Bounded: each attempt produces a distinct candidate, so one more
        // attempt than there are used names is always enough.
        let attempts = self.used_names.len().saturating_add(2);
        for _ in 0..attempts {
            let candidate = format!("{base}#{}", self.fresh_counter);
            self.fresh_counter = self.fresh_counter.saturating_add(1);
            if !self.used_names.contains(&candidate) {
                self.used_names.insert(candidate.clone());
                return ItsVar::new(candidate);
            }
        }
        self.mark_malformed("could not find a fresh variable name");
        ItsVar::new(base)
    }

    // -- bookkeeping --------------------------------------------------------

    fn refuse(&mut self, construct: Construct, origin: Origin, detail: Option<Symbol>) {
        let record = match detail {
            Some(detail) => Unsupported::with_detail(construct, origin, detail),
            None => Unsupported::new(construct, origin),
        };
        match &mut self.refusals {
            Some(refusals) => refusals.insert(record),
            slot @ None => *slot = Some(Refusals::new(record)),
        }
    }

    /// A polynomial result, or zero with a refusal recorded.
    fn checked(&mut self, result: Result<Polynomial, Construct>, origin: &Origin) -> Polynomial {
        match result {
            Ok(polynomial) => polynomial,
            Err(construct) => {
                self.refuse(construct, origin.clone(), None);
                Polynomial::zero()
            }
        }
    }

    fn mark_malformed(&mut self, detail: &str) {
        if self.malformed.is_none() {
            self.malformed = Some(Symbol::from(detail));
        }
    }

    fn malformed_error(&self, detail: &str) -> LoweringError {
        LoweringError::Malformed {
            function: self.program.name().clone(),
            detail: Symbol::from(detail),
        }
    }

    fn expr_origin(&self, id: ExprId) -> Origin {
        self.program
            .expr_origin(id)
            .cloned()
            .unwrap_or_else(|| self.program.origin().clone())
    }

    fn cond_origin(&self, id: CondId) -> Origin {
        self.program
            .cond_origin(id)
            .cloned()
            .unwrap_or_else(|| self.program.origin().clone())
    }

    fn expr_slot(&mut self, id: ExprId) -> Option<usize> {
        let slot = usize::try_from(id.index()).ok()?;
        if slot >= self.expr_memo.len() {
            self.mark_malformed("expression handle names no node");
            return None;
        }
        Some(slot)
    }

    fn cond_slot(&mut self, id: CondId) -> Option<usize> {
        let slot = usize::try_from(id.index()).ok()?;
        if slot >= self.cond_memo.len() {
            self.mark_malformed("condition handle names no node");
            return None;
        }
        Some(slot)
    }

    /// Enforces that a child's arena index precedes its parent's.
    ///
    /// The builder guarantees it - a node's children exist before the node
    /// does - so a violation means a handle from a different program, and the
    /// check is what makes a cycle, and with it an unbounded traversal,
    /// impossible.
    fn expr_child_ok(&mut self, parent: ExprId, child: ExprId) -> bool {
        if child.index() >= parent.index() {
            self.mark_malformed("an expression operand refers to itself or a later node");
            return false;
        }
        true
    }

    fn cond_child_ok(&mut self, parent: CondId, child: CondId) -> bool {
        if child.index() >= parent.index() {
            self.mark_malformed("a condition operand refers to itself or a later node");
            return false;
        }
        true
    }

    fn store_expr(&mut self, slot: usize, polynomial: Polynomial) {
        if let Some(cell) = self.expr_memo.get_mut(slot) {
            *cell = Some(polynomial);
        }
    }

    fn store_cond(&mut self, slot: usize, dnf: (Dnf, Dnf)) {
        if let Some(cell) = self.cond_memo.get_mut(slot) {
            *cell = Some(dnf);
        }
    }

    fn recall_expr(&mut self, id: ExprId) -> Polynomial {
        let recalled = self
            .expr_slot(id)
            .and_then(|slot| self.expr_memo.get(slot).cloned().flatten());
        match recalled {
            Some(polynomial) => polynomial,
            None => {
                self.mark_malformed("expression operand was not evaluated before its parent");
                Polynomial::zero()
            }
        }
    }

    fn recall_cond(&mut self, id: CondId) -> (Dnf, Dnf) {
        let recalled = self
            .cond_slot(id)
            .and_then(|slot| self.cond_memo.get(slot).cloned().flatten());
        match recalled {
            Some(dnf) => dnf,
            None => {
                self.mark_malformed("condition operand was not evaluated before its parent");
                (dnf_true(), dnf_true())
            }
        }
    }
}

/// The origin recorded for arena entry `index`, falling back to the
/// program's own position.
fn origin_at(origins: &[Origin], index: usize, fallback: &Origin) -> Origin {
    origins.get(index).unwrap_or(fallback).clone()
}

/// The normal form that admits everything.
fn dnf_true() -> Dnf {
    vec![Vec::new()]
}

/// Conjunction of two normal forms: the pairwise concatenation of clauses.
///
/// Widens to `true` rather than exceeding [`MAX_DNF_CLAUSES`].
fn cross(left: &Dnf, right: &Dnf) -> Dnf {
    if left.len().saturating_mul(right.len()) > MAX_DNF_CLAUSES {
        return dnf_true();
    }
    let mut out = Vec::with_capacity(left.len().saturating_mul(right.len()));
    for left_clause in left {
        for right_clause in right {
            let mut clause = left_clause.clone();
            clause.extend(right_clause.iter().cloned());
            out.push(clause);
        }
    }
    out
}

/// Disjunction of two normal forms: the concatenation of their clauses.
///
/// Widens to `true` rather than exceeding [`MAX_DNF_CLAUSES`].
fn disjoin(left: &Dnf, right: &Dnf) -> Dnf {
    if left.len().saturating_add(right.len()) > MAX_DNF_CLAUSES {
        return dnf_true();
    }
    let mut out = left.clone();
    out.extend(right.iter().cloned());
    out
}

/// Unit tests for the parts of this module that no integration test can name.
///
/// # Why these live here rather than in `tests/`
///
/// `expr_child_ok`, `cond_child_ok`, `cross` and `disjoin` are private, and
/// three of the four are only reachable from outside through a whole lowering.
/// That indirection is what let their boundaries survive mutation testing:
/// the guards were killed by a 120-second clock rather than an assertion, and
/// the two [`MAX_DNF_CLAUSES`] comparisons were not killed at all, because
/// building a source condition whose normal form lands *exactly* on the cap is
/// far more work than the property deserves.
///
/// Being unit tests they also run in the library test binary, which cargo
/// executes **before** any integration target - so a broken guard reports here
/// first, in microseconds, and `tests/frozen_invariants.rs` explains at length
/// why that matters.
#[cfg(test)]
mod tests {
    // A test that cannot assert is not a test; the panic lints are relaxed
    // here exactly as they are in `tests/properties/main.rs`.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use landav_bound::Origin;

    use super::{Dnf, MAX_DNF_CLAUSES, cross, disjoin, dnf_true};
    use crate::{
        cond_id::CondId, constraint::Constraint, expr_id::ExprId, its_var::ItsVar,
        lowering::Lowering, polynomial::Polynomial, relation::Relation,
        source_program::SourceProgram, source_program_builder::SourceProgramBuilder,
    };

    fn empty_program() -> SourceProgram {
        SourceProgramBuilder::new("guarded", Origin::new("guard.rs:1:1"), vec![]).build(vec![])
    }

    /// `count` distinct one-constraint clauses.
    fn clauses(count: usize) -> Dnf {
        (0..count)
            .map(|index| {
                let polynomial = Polynomial::var(ItsVar::new(format!("v{index}")));
                vec![Constraint::new(polynomial, Relation::Ge)]
            })
            .collect()
    }

    /// Whether a normal form is the widened `true`.
    fn is_widened(dnf: &Dnf) -> bool {
        *dnf == dnf_true()
    }

    // -- the acyclicity guards ----------------------------------------------

    /// **A child's index must strictly precede its parent's.**
    ///
    /// This is the sole reason `expr_poly`'s worklist terminates: each edge it
    /// follows strictly decreases the arena index, so the chain of visits from
    /// a node at index `n` is at most `n` long. Relax the comparison to `>` -
    /// or let the function return `true` unconditionally, which is the mutant
    /// that was killed by the clock - and a program whose node 0 names node 1
    /// while node 1 names node 0 is walked forever.
    ///
    /// Asserted here directly, on a guard that never enters the loop, because
    /// an assertion inside a traversal that can hang cannot report.
    #[test]
    fn an_expression_operand_must_precede_the_node_that_names_it() {
        let program = empty_program();
        let mut lowering = Lowering::new(&program);

        assert!(
            lowering.expr_child_ok(ExprId(4), ExprId(3)),
            "an earlier operand is well-formed and must be followed"
        );
        assert!(
            lowering.expr_child_ok(ExprId(1), ExprId(0)),
            "index zero is a legitimate operand of index one"
        );
        assert!(
            lowering.malformed.is_none(),
            "a well-formed operand must not be blamed"
        );

        assert!(
            !lowering.expr_child_ok(ExprId(3), ExprId(3)),
            "a node naming itself is a cycle of length one"
        );
        assert!(
            !lowering.expr_child_ok(ExprId(3), ExprId(4)),
            "a node naming a later node can close a cycle"
        );
        assert!(
            !lowering.expr_child_ok(ExprId(0), ExprId(0)),
            "the first node has no legitimate operand at all"
        );
        assert!(
            lowering
                .malformed
                .as_ref()
                .is_some_and(|detail| !detail.as_str().is_empty()),
            "refusing an operand must record why"
        );
    }

    /// The same invariant, on the condition arena, whose traversal terminates
    /// for the same reason and whose guard survived mutation the same way.
    #[test]
    fn a_condition_operand_must_precede_the_node_that_names_it() {
        let program = empty_program();
        let mut lowering = Lowering::new(&program);

        assert!(lowering.cond_child_ok(CondId(2), CondId(1)));
        assert!(lowering.malformed.is_none());

        assert!(
            !lowering.cond_child_ok(CondId(2), CondId(2)),
            "a condition naming itself is a cycle"
        );
        assert!(
            !lowering.cond_child_ok(CondId(2), CondId(7)),
            "a condition naming a later node can close a cycle"
        );
        assert!(
            lowering
                .malformed
                .as_ref()
                .is_some_and(|detail| !detail.as_str().is_empty()),
            "refusing a condition operand must record why"
        );
    }

    // -- the DNF cap --------------------------------------------------------

    /// **[`MAX_DNF_CLAUSES`] is an inclusive limit.**
    ///
    /// `cross` widens to `true` rather than form a product bigger than the
    /// cap. Widening is *sound*, which is why getting the boundary wrong is
    /// invisible to the soundness suite: a `>=` here throws away a perfectly
    /// representable conjunction and silently makes both branches of an `if`
    /// available, and every soundness property still passes. The boundary is
    /// therefore stated as a boundary - at the cap, exact; one past it,
    /// widened.
    #[test]
    fn a_product_exactly_at_the_clause_cap_is_kept_and_one_past_it_widens() {
        assert_eq!(
            MAX_DNF_CLAUSES, 64,
            "the cases below are chosen to divide it"
        );

        let exact = cross(&clauses(8), &clauses(8));
        assert_eq!(
            exact.len(),
            MAX_DNF_CLAUSES,
            "a product landing exactly on the cap must be kept, not widened"
        );
        assert!(
            !is_widened(&exact),
            "an exact-cap product was widened to true"
        );
        assert!(
            exact.iter().all(|clause| clause.len() == 2),
            "a cross product's clauses are the concatenation of one from each side"
        );

        let over = cross(&clauses(8), &clauses(9));
        assert!(
            is_widened(&over),
            "a product of {} clauses exceeds the cap and must widen to true",
            8 * 9
        );

        // And the cheap end still behaves: nothing widens below the cap.
        let small = cross(&clauses(3), &clauses(4));
        assert_eq!(small.len(), 12);
        assert!(!is_widened(&small));
    }

    /// The same boundary for `disjoin`, whose budget is a sum rather than a
    /// product.
    #[test]
    fn a_disjunction_exactly_at_the_clause_cap_is_kept_and_one_past_it_widens() {
        let exact = disjoin(&clauses(32), &clauses(32));
        assert_eq!(
            exact.len(),
            MAX_DNF_CLAUSES,
            "a disjunction landing exactly on the cap must be kept"
        );
        assert!(!is_widened(&exact));

        let over = disjoin(&clauses(32), &clauses(33));
        assert!(
            is_widened(&over),
            "a disjunction of 65 clauses exceeds the cap and must widen to true"
        );

        let small = disjoin(&clauses(2), &clauses(3));
        assert_eq!(small.len(), 5);
        assert!(!is_widened(&small));
    }

    /// The widened value is the normal form that admits everything: one empty
    /// clause. An empty *disjunction* would be `false` and admit nothing,
    /// which is the unsound direction, so the two must not be confused.
    #[test]
    fn widening_produces_true_and_not_false() {
        let widened = dnf_true();
        assert_eq!(widened.len(), 1, "`true` is a single clause");
        assert!(
            widened.iter().all(Vec::is_empty),
            "`true`'s single clause is the empty conjunction"
        );
        assert!(
            !widened.is_empty(),
            "an empty disjunction is `false`, which admits no transition at all"
        );
    }
}
