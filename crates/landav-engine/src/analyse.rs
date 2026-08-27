//! Walking a structured program and accumulating what it costs.

use std::collections::BTreeSet;

use landav_bound::{Bound, Origin, VarId};
use landav_its::{
    CondId, Construct, CostEffect, DeclaredEffect, ExprId, Extent, NodeId, RangeSpec, SourceCond,
    SourceExpr, SourceProgram, SourceStmt, StmtId, VarName,
};

use crate::{expr_bound, hole::Hole, summation, trip_count::TripCount};

/// What the program costs, in **source steps**: one per statement executed,
/// plus one per loop iteration for the loop's own test and increment.
///
/// # This is not the same unit the solver reports
///
/// Measured rather than assumed. For `for i in range(n): for j in range(m): x = 0`
/// this engine gives `n * (1 + 2m)`, and KoAT2 over the lowered system gives
/// `3mn + 3m + 4n + 2`. The gap is not precision - it is that the lowering
/// emits several transitions per source construct (a guard test, a body, a
/// counter increment) and the solver counts all of them.
///
/// Neither number is wrong; they answer different questions. This one is the
/// more stable of the two, because the ITS transition count is an artifact of
/// lowering choices that could change without the program changing.
///
/// **The consequence is that the two cannot currently be compared**, so the
/// "report tightness when upper and lower meet" plan does not work as written.
/// Reconciling them is what `Cost` on a transition is for: charging the
/// bookkeeping transitions nothing and the source-bearing ones one step would
/// put both engines in this unit. That is tracked separately and is not done
/// here.
///
/// # Total over [`SourceProgram`], and why that had to change
///
/// This used to be called only for programs [`landav_its::lower`] had already
/// accepted, so an `Unsupported` node that was not a whole statement never
/// reached it. `LAN-87` removes that filter: a call is the sole construct
/// blocking most of the corpus, and refusing the whole function for one is the
/// difference between a report a user can act on and no report at all.
///
/// So every `Unsupported` node is now charged as a [`Hole`] **at its position
/// in the control structure** - inside the loop that runs it, added to the
/// statement that contains it - and the walk is reconciled afterwards against
/// [`SourceProgram::unsupported_nodes`]. A node the walk did not charge fails
/// the whole result closed to [`TripCount::Unknown`], because there is no
/// position information left to place it with and a flat charge at the top
/// level *understates* the cost of anything that belonged inside a loop.
#[must_use]
pub fn cost(program: &SourceProgram) -> TripCount {
    let mut walk = Walk::new(program);
    let derived = walk.body_cost(program.body());
    walk.reconciled(derived)
}

/// The state one analysis carries: hole numbering, what it charged, and which
/// variable reads it is entitled to believe.
struct Walk<'a> {
    program: &'a SourceProgram,
    holes: Counter,
    /// Every `Unsupported` node this walk has accounted for.
    ///
    /// Compared against the program's own scan at the end. Charging is what
    /// makes a node accounted for; the scan is only ever used to *check*, never
    /// to charge, because a node the walk never reached has no position and no
    /// sound charge. See [`Walk::reconciled`].
    charged: BTreeSet<NodeId>,
    /// The variables whose value at this point is the value the caller supplied.
    ///
    /// A [`Bound`] is written in the caller's variables, so this is exactly the
    /// set of names a read may be turned into one of. It starts as the
    /// parameters, loses a name to every assignment, gains the counter of each
    /// loop whose body cannot change it, and is emptied entirely by an
    /// unanalysable region - which may assign anything.
    readable: BTreeSet<VarName>,
}

/// Hands out distinct indices so two regions never share a hole variable.
///
/// Threaded rather than global: two analyses of the same program must produce
/// the same bound, and a process-wide counter would make the variable names
/// depend on how much analysis happened first.
#[derive(Default)]
struct Counter(usize);

impl Counter {
    fn next(&mut self, construct: &'static str, origin: Origin) -> Hole {
        let hole = Hole::new(self.0, construct, origin);
        self.0 += 1;
        hole
    }
}

impl<'a> Walk<'a> {
    fn new(program: &'a SourceProgram) -> Self {
        Self {
            program,
            holes: Counter::default(),
            charged: BTreeSet::new(),
            // A parameter holds the caller's value until something assigns to
            // it. Nothing has yet.
            readable: program.params().iter().cloned().collect(),
        }
    }

    // -- reconciliation -----------------------------------------------------

    /// `derived`, unless the walk failed to account for an `Unsupported` node.
    ///
    /// # Why this fails closed rather than sweeping up
    ///
    /// The tempting repair is to charge the nodes the walk missed at the top
    /// level, one hole each. It is unsound, and measurably so: for a program
    /// that is an orphaned region beside `for i in range(n): pass`, a flat
    /// charge gives `n + #hole` while the truth - if the region belonged in the
    /// loop body - is `n * (1 + #hole)`. The flat charge **understates**, which
    /// is the one direction a resource bound must never move.
    ///
    /// There is no position information to recover, so there is no sound
    /// charge, so the honest answer is that nothing is known.
    fn reconciled(&self, derived: TripCount) -> TripCount {
        let accounted = self
            .program
            .unsupported_nodes()
            .all(|node| self.charged.contains(&node.id()));
        if accounted {
            derived
        } else {
            TripCount::Unknown
        }
    }

    // -- statements ---------------------------------------------------------

    /// The cost of a statement list, in sequence.
    ///
    /// # An early exit in the middle is not an equality
    ///
    /// The sum charges every statement in the list. That is the whole cost only
    /// when every statement runs, and a `return` before the end of the block is
    /// exactly the case where they do not: everything after it is charged and
    /// never executed. The sum still **dominates** the truth - which is the
    /// direction that matters - but it is no longer attained, and `Exact` is a
    /// two-sided claim that says it is.
    ///
    /// So a body whose tail is reachable only past an early exit is relaxed.
    /// One as the last statement changes nothing and stays exact, which is the
    /// shape almost every real function has.
    ///
    /// A `return` is not the only such exit: a `raise`, and a `try` whose body
    /// or handler can raise, leave the same way. See [`exits_within`].
    fn body_cost(&mut self, body: &[StmtId]) -> TripCount {
        let mut total = TripCount::Exact(Bound::zero());
        let mut unreachable_tail = false;
        for (position, id) in body.iter().enumerate() {
            if position + 1 < body.len() && exits_within(self.program, *id) {
                unreachable_tail = true;
            }
            let next = self.stmt_cost(*id);
            total = total.then(next);
        }
        if unreachable_tail {
            total.relax()
        } else {
            total
        }
    }

    /// The cost of one statement.
    fn stmt_cost(&mut self, id: StmtId) -> TripCount {
        let Some(stmt) = self.program.stmt(id) else {
            return TripCount::Unknown;
        };
        match stmt {
            // One step for the statement itself, plus whatever its right-hand
            // side contains that could not be read. `Exact(1)` alone was the
            // whole-statement claim being made on the strength of the statement
            // *form*: `x = f(n)` costs one step **plus the call**.
            SourceStmt::Assign { target, value } => {
                let regions = self.expr_regions(*value);
                // The variable no longer holds the value the caller supplied.
                self.readable.remove(target);
                regions.then(TripCount::Exact(Bound::one()))
            }

            SourceStmt::Return => TripCount::Exact(Bound::one()),

            // One step, and nothing forgotten. A `raise` assigns to nothing, so
            // charging it as a region would erase the trip count of a loop in
            // the handler beside it for no gain. What it *does* do is leave
            // early, which is a fact about the count of an enclosing loop and
            // not about this statement's own cost: `exits_within` reports it and
            // `loop_cost` and `body_cost` both act on that.
            SourceStmt::Raise => TripCount::Exact(Bound::one()),

            SourceStmt::Protected {
                body,
                handler,
                cleanup,
            } => {
                let (body, handler, cleanup) = (body.clone(), handler.clone(), cleanup.clone());
                self.protected_cost(&body, &handler, &cleanup)
            }

            SourceStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                // The test is evaluated whichever way the branch goes, so its
                // cost is additive and unconditional rather than something to
                // maximise over the arms.
                let test = self.cond_regions(*cond);
                let entry = self.readable.clone();
                let then_cost = self.body_cost(then_body);
                let after_then = core::mem::replace(&mut self.readable, entry);
                let else_cost = self.body_cost(else_body);
                // A name survives the join only if both arms left it alone.
                self.readable.retain(|name| after_then.contains(name));

                let taken = then_cost.branching(else_cost);
                // One step for the test itself, whichever way it goes.
                test.then(taken).then(TripCount::Exact(Bound::one()))
            }

            // A `while` loop needs a ranking argument, which this engine does
            // not have. It becomes a **hole** rather than a guess or a refusal:
            // the cost is real and unknown, so it is named and carried.
            // Everything around it still gets derived, which is the whole point
            // - before holes, one `while` erased every exact bound in the
            // function.
            SourceStmt::While { cond, body } => {
                // The hole is the cost of the whole loop, condition and body
                // included, so the regions inside it are accounted for by it
                // rather than charged again beside it.
                self.absorb_cond(*cond);
                let body = body.clone();
                self.absorb_body(&body);
                // How many times the body ran, and therefore what it assigned,
                // is exactly what is not known.
                self.readable.clear();
                TripCount::opaque(self.holes.next("while", self.stmt_origin(id)))
            }

            SourceStmt::ForRange {
                target,
                range,
                body,
            } => {
                let (target, range, body) = (target.clone(), *range, body.clone());
                self.for_range_cost(id, &target, range, &body)
            }

            // Something the frontend could not translate, charged as a hole
            // where it stands. Before `LAN-87` this was the only path in this
            // function that charged anything.
            //
            // Whether the statement's own step is charged as well is the node's
            // to say, and the two answers are not interchangeable. A bare `f(n)`
            // is a statement: it costs a step **plus** the call, and a hole
            // filled later with what the callee costs then reconstructs exactly
            // the bound the engine would have derived had it known the callee -
            // which is the composition guarantee `Hole` and
            // `TripCount::iterating` both promise. The `f(n)` in `return f(n)`
            // is a fragment: the `Return` beside it is already charging that
            // step, and charging it twice would report `2 + call` for one line.
            // See `landav_its::Extent`.
            SourceStmt::Unsupported {
                construct,
                extent,
                declared,
                ..
            } => {
                let (extent, declared) = (*extent, *declared);
                self.charged.insert(NodeId::Stmt(id));
                // `Walk::region` is what forgets every readable value here; see
                // the invariant recorded there.
                let region = self.region(*construct, declared, self.stmt_origin(id));
                match extent {
                    Extent::Statement => region.then(TripCount::Exact(Bound::one())),
                    Extent::Fragment => region,
                }
            }
        }
    }

    /// The cost of a body that may be abandoned partway through.
    ///
    /// # The arithmetic
    ///
    /// `body + handler + cleanup`, and every term is deliberate. See
    /// [`landav_its::SourceStmt::Protected`], where the argument for each one
    /// lives; in short, the handler *adds to* a prefix of the body rather than
    /// replacing it, the maximum over prefixes is the whole body, and the
    /// cleanup is outside the choice because it runs whichever way the body
    /// went.
    ///
    /// # Why it is never an equality
    ///
    /// The whole body is charged and only a prefix of it runs on the
    /// exceptional path, so the sum dominates without being attained. That is
    /// the same thing [`Walk::body_cost`] says about a statement list whose tail
    /// sits past a `return`, and it is answered the same way. Relaxing here also
    /// relaxes every enclosing count, because exactness is conjunctive through
    /// `TripCount::combine`.
    ///
    /// # The obligation taken on by walking in
    ///
    /// A `try` used to reach this engine as one `Unsupported` statement, and
    /// `Walk::region` cleared `readable` at it. Nothing routes through `region`
    /// any more, so the rule is restated here: **which statements of the body
    /// ran at all depends on where the exception hit**, so neither the entry
    /// value nor the post-body value of anything is the one that holds
    /// afterwards, and the caller's values do not survive.
    ///
    /// The clearing happens *after* the three walks, which is the entire point
    /// of the ticket: a loop inside the body still reads the endpoints it was
    /// entered with, and a handler still reads the ones the body left. Both are
    /// sound - a prefix of the body writes no more names than the whole of it,
    /// and `readable` only ever shrinks - and both are what makes the derived
    /// bound a function of `n` rather than a number that has never heard of it.
    fn protected_cost(
        &mut self,
        body: &[StmtId],
        handler: &[StmtId],
        cleanup: &[StmtId],
    ) -> TripCount {
        let body = self.body_cost(body);
        let handler = self.body_cost(handler);
        let cleanup = self.body_cost(cleanup);
        // No value the caller supplied survives a region that may have run in
        // part. See the doc comment: this is the enforcement point that
        // `Walk::region` used to be for this construct.
        self.readable.clear();
        body.then(handler).then(cleanup).relax()
    }

    /// The whole cost of a counted loop, endpoints included.
    fn for_range_cost(
        &mut self,
        id: StmtId,
        target: &VarName,
        range: RangeSpec,
        body: &[StmtId],
    ) -> TripCount {
        // Both endpoints are evaluated **once**, before the loop begins, so a
        // region in one of them is charged once and outside the multiplication.
        let endpoints = {
            let start = self.expr_regions(range.start);
            let stop = self.expr_regions(range.stop);
            start.then(stop)
        };
        // Read before the body can change anything: `RangeSpec` is evaluated
        // before the first iteration, so a body that assigns to a name in the
        // endpoint does not change the trip count. The ceiling is read here for
        // the same reason, and *only* here: a `while` in the body empties
        // `readable`, so reading it afterwards would lose the endpoint of a loop
        // that had one.
        let count = self.count_of(range);
        let ceiling = self.counter_ceiling(range);

        if matches!(count, TripCount::Unknown) {
            // The body is understood and how many times it runs is not.
            // Multiplying by an unknown count is the same as knowing nothing
            // about the whole loop, so the loop is itself one region and every
            // region inside it is accounted for by that one.
            self.absorb_body(body);
            self.readable.clear();
            let whole = TripCount::opaque(self.holes.next("for", self.stmt_origin(id)));
            return endpoints.then(whole);
        }

        // Entering the body: a name the body assigns does not hold the caller's
        // value on the second iteration, so it does not hold it on the first
        // either as far as this analysis may claim.
        let writes = writes_of(self.program, body);
        match &writes {
            Some(names) => self.readable.retain(|name| !names.contains(name)),
            // A region inside the body may assign anything.
            None => self.readable.clear(),
        }
        // The counter is a value this analysis *does* know: it ascends through
        // the iteration space, and `close_over_counter` substitutes something
        // that dominates it. That is only true while the body leaves it alone.
        let counter_is_stable = writes.as_ref().is_some_and(|names| !names.contains(target));
        if counter_is_stable {
            self.readable.insert(target.clone());
        }

        let derived = self.loop_cost(target, range, body, count, ceiling.as_ref());

        // The counter is bound by its loop. After the loop it holds whatever the
        // last iteration left, which is not a value the caller supplied.
        if counter_is_stable {
            self.readable.remove(target);
        }
        endpoints.then(derived)
    }

    /// The cost of a counted loop whose trip count is known, summed exactly
    /// where that is possible.
    ///
    /// # Two routes, and why the exact one is tried first
    ///
    /// The cost of the loop is the definite sum of the body's cost over the
    /// values the counter takes. Where that sum has a closed form the bound
    /// algebra can hold, it is the answer and it is exact.
    ///
    /// Where it does not - the body is not a polynomial in the counter, or the
    /// closed form keeps a denominator or a negative coefficient - the fallback
    /// substitutes an endpoint that dominates the counter (see
    /// [`Walk::counter_ceiling`]) and multiplies, which over-approximates and
    /// says so.
    ///
    /// The difference is not small. Triangular nesting sums to `n^2` and
    /// approximates to `2n^2 + n`.
    ///
    /// # Both routes stay sound
    ///
    /// The summation is only taken when it is exact, so it never trades
    /// soundness for tightness. The fallback is sound because the counter of an
    /// ascending loop from zero is dominated by its trip count and [`Bound`] is
    /// weakly monotone by construction.
    ///
    /// # An early exit in the body is an edge out of the count
    ///
    /// The trip count is arithmetic rather than inference *because* nothing can
    /// leave the loop early - the fragment refuses `break` and `continue`. Two
    /// things it does not refuse can: a `return`, and an exceptional exit -
    /// either a `raise` or a `try` whose body, handler or cleanup can raise. A
    /// body containing one makes the count an over-estimate rather than an
    /// equality, and the result is relaxed. Sound either way; the change is to
    /// the claim, not to the number.
    ///
    /// Charging the exit its step without also relaxing here is the whole of the
    /// hazard: `for i in range(n): raise` executes two steps for every `n`, and
    /// `Theta(2n)` for it is complete, exact, carries no holes, and is a bound
    /// the program does not meet.
    fn loop_cost(
        &mut self,
        target: &VarName,
        range: RangeSpec,
        body: &[StmtId],
        count: TripCount,
        ceiling: Option<&Bound>,
    ) -> TripCount {
        let leaves_early = body.iter().any(|id| exits_within(self.program, *id));
        let body = self.body_cost(body);
        let counter = VarId::new(target.symbol().clone());

        // The summation is only valid for the counter this engine can reason
        // about - ascending from zero in unit steps - because that is the
        // progression Faulhaber's formulae are over. A descending or strided
        // loop has the same trip count but a different sequence of counter
        // values.
        let summable = range.ascending()
            && range.step.get() == 1
            && matches!(
                self.program.expr(range.start),
                Some(SourceExpr::Int { value: 0 })
            );

        if let (true, TripCount::Exact(trip), TripCount::Exact(per_iteration)) =
            (summable, &count, &body)
        {
            // One step of loop overhead per iteration, inside the sum.
            let charged = Bound::sum([per_iteration.clone(), Bound::one()]);
            if let Some(total) = summation::sum_over_counter(&charged, &counter, trip) {
                let summed = TripCount::Exact(total);
                return if leaves_early { summed.relax() } else { summed };
            }
        }

        let approximated = close_over_counter(target, ceiling, body);
        let whole = count.iterating(approximated);
        if leaves_early { whole.relax() } else { whole }
    }

    /// How many times a counted loop runs.
    ///
    /// # Where exactness comes from, and where it stops
    ///
    /// The iteration space is fixed before the loop starts, so this is
    /// arithmetic rather than inference. What limits it is not knowledge but
    /// *expressibility*: the bound algebra is weakly monotone by construction
    /// and therefore has no subtraction and no division, so `max(0, stop -
    /// start)` and `ceil(n / k)` cannot be written down even when both are
    /// perfectly well understood.
    ///
    /// The cases that survive:
    ///
    /// * `range(0, e)` with unit step - the count is `e` itself, and `e` is
    ///   already a bound whenever it is monotone **and written in variables the
    ///   caller supplies**; see [`expr_bound::read`].
    /// * any range whose endpoints are both literals - the count is arithmetic
    ///   on two numbers, done here in `i128` so it cannot overflow.
    ///
    /// Everything else is an over-approximation, and says so.
    fn count_of(&self, range: RangeSpec) -> TripCount {
        let start = self.program.expr(range.start);
        let stop = self.program.expr(range.stop);
        let step = range.step.get();

        // Both endpoints literal: compute the count outright. `i128` because
        // `stop - start` can exceed `i64` when the two straddle the range, and
        // a wrapped subtraction here would be a silently wrong trip count.
        if let (Some(SourceExpr::Int { value: from }), Some(SourceExpr::Int { value: to })) =
            (start, stop)
        {
            let (from, to, step) = (i128::from(*from), i128::from(*to), i128::from(step));
            let span = if step > 0 { to - from } else { from - to };
            let stride = step.abs();
            let count = if span <= 0 {
                0
            } else {
                // Ceiling division, done without floats.
                (span + stride - 1) / stride
            };
            return u64::try_from(count)
                .map_or(TripCount::Unknown, |n| TripCount::Exact(Bound::constant(n)));
        }

        // Symbolic stop, unit ascending step, and a start pinned to zero: the
        // count *is* the stop expression. This is `for i in range(n)`, which is
        // the overwhelming majority of counted loops in real Python.
        //
        // *Is*, and only when the reading is the value. An endpoint like
        // `n // 2` reads as `n`, which dominates it; a loop counted by that
        // runs fewer times than the number says, so it is an `AtMost` and not
        // a `Theta`. Wrapping every reading in `Exact` was sound only while
        // every reading was exact, and `LAN-91` is where that stopped being
        // true. See [`expr_bound::Reading`].
        if step == 1 && matches!(start, Some(SourceExpr::Int { value: 0 })) {
            return expr_bound::read(self.program, range.stop, &self.readable).map_or(
                TripCount::Unknown,
                |reading| {
                    if reading.is_exact() {
                        TripCount::Exact(reading.into_bound())
                    } else {
                        TripCount::AtMost(reading.into_bound())
                    }
                },
            );
        }

        // A symbolic start would need `stop - start`, and a stride above one
        // would need division. Neither is expressible, and neither has a sound
        // over-approximation that does not first require knowing the start is
        // non-negative - which nothing here establishes.
        TripCount::Unknown
    }

    /// A bound on **every value the counter takes**, for `close_over_counter`.
    ///
    /// An ascending `range(start, stop, step)` never reaches `stop`; a
    /// descending one starts at `start` and only falls. So the endpoint the loop
    /// moves *away from* dominates the counter, whatever the stride, and that is
    /// the number a body's cost may have the counter replaced by.
    ///
    /// # Why not the trip count
    ///
    /// It used to be the trip count, and the two are the same number only for
    /// `range(0, e)` in unit steps - the shape `loop_cost` already guards its
    /// *summation* route with, and the shape the fallback never checked for.
    /// `for i in range(0, 1000, 100)` runs ten times and its counter reaches
    /// 900, so substituting the count into a body linear in the counter gave a
    /// **complete** bound - `bound_kind: "upper"`, no holes, offered for
    /// comparison against a budget - that the program exceeds by a factor
    /// growing with the stride. Measured: 212 reported against 9012 executed.
    ///
    /// Nothing is given up on the common shape. For `range(0, e)` with unit step
    /// the count *is* `e`, so this reads the same expression the old code did.
    fn counter_ceiling(&self, range: RangeSpec) -> Option<Bound> {
        let endpoint = if range.ascending() {
            range.stop
        } else {
            range.start
        };
        // Only the magnitude matters here - this dominates the counter, and an
        // over-approximation of the endpoint still does.
        expr_bound::read(self.program, endpoint, &self.readable)
            .map(expr_bound::Reading::into_bound)
    }

    // -- regions ------------------------------------------------------------

    /// One unanalysable region, charged as a hole that names it.
    ///
    /// # The invariant this is the single enforcement point for
    ///
    /// An unanalysable region may assign to anything, so **no earlier value
    /// survives it**. The engine keeps no assignment environment, and holing a
    /// call is sound *because* of that absence: nothing downstream may be
    /// derived from a value the region could have changed.
    ///
    /// That rule used to be written at one of the three callers - the statement
    /// arm - which made it true for statements and silently false for
    /// expressions and conditions. Python has exactly one expression that
    /// rebinds a local, and it is refused as a region: `if (n := 100) > 0: pass`
    /// followed by `for i in range(n)` was reported `Theta(2 + #hole0 + n)` for a
    /// loop that runs a hundred times whatever the caller passed. Charging is
    /// the only place a region is recognised, so it is the only place the rule
    /// can be stated once and hold everywhere.
    ///
    /// `for_range_cost` reads its endpoints *before* the body walk for the
    /// converse reason: a `RangeSpec` really is evaluated before the first
    /// iteration, so a region inside the loop does not change the trip count.
    ///
    /// # A node that declared its own answer
    ///
    /// `declared` is the per-**node** answer to the two questions
    /// `Construct::may_rebind_locals` and `cost_effect` answer per *kind*. A
    /// call is holed and forgets everything because `Construct::Call` has to
    /// speak for every callee at once; a node the frontend could name a bounded
    /// cost for is charged that cost and forgets only what it says it may
    /// change. See [`landav_its::DeclaredEffect`].
    ///
    /// Nothing here knows why the frontend was able to say that, and nothing
    /// here should: a table of callee names in this crate would be a Python
    /// fact in a language-agnostic layer. What this crate owns is the rule that
    /// an *undeclared* node is unchanged - still a named hole, still forgetting
    /// every readable value - which is what keeps the pack an allowlist rather
    /// than a relaxation.
    fn region(
        &mut self,
        construct: Construct,
        declared: Option<DeclaredEffect>,
        origin: Origin,
    ) -> TripCount {
        let rebinds = declared.map_or_else(
            || construct.may_rebind_locals(),
            DeclaredEffect::rebinds_locals,
        );
        // A declaration that *also* claims not to mutate an argument keeps
        // everything, volatile names included. Nothing admitted so far claims
        // that - an `__instancecheck__` is user code and may call
        // `items.append(...)` - and the field exists so that a signature which
        // can claim it has somewhere to say so.
        let mutates = declared.is_none_or(DeclaredEffect::mutates_arguments);
        if rebinds {
            self.readable.clear();
        } else if mutates {
            // A read runs foreign code, and foreign code cannot rebind a name in
            // *this* frame - see `Construct::may_rebind_locals`, where that
            // argument lives. What it can do is mutate an object, so anything
            // standing for a property of one goes: a `property` getter is free
            // to call `items.append(...)`, and `len(items)` on entry is then not
            // the length the loop below runs over. Which names are of that kind
            // is the frontend's to say, and it says so in the program.
            let program = self.program;
            self.readable.retain(|name| !program.is_volatile(name));
        }
        if let Some(effect) = declared {
            // Not a hole: the cost is known, so there is nothing to name and
            // nothing for `Bound::subst` to fill later. `cost_effect` is not
            // consulted either - a declared node is bounded work in the middle
            // of a region, never an edge out of one, and a frontend wanting to
            // declare an early exit would be declaring something this type
            // cannot express.
            return TripCount::Exact(Bound::constant(u64::from(effect.steps())));
        }
        let hole = self.holes.next(construct.tag(), origin);
        let charged = TripCount::opaque(hole);
        match construct.cost_effect() {
            CostEffect::Region => charged,
            // An edge *out of* the region being counted. A loop containing one
            // may stop before its counter is exhausted, so nothing enclosing it
            // was derived exactly - and `exact_elsewhere` is precisely the claim
            // that it was. `relax` is conjunctive through `TripCount::combine`,
            // so weakening here weakens every enclosing count.
            CostEffect::ControlFlow => charged.relax(),
        }
    }

    /// The cost of the regions inside an expression, in arena order.
    ///
    /// Zero when there are none, which is the overwhelmingly common case and
    /// leaves the caller's arithmetic exactly as it was.
    fn expr_regions(&mut self, root: ExprId) -> TripCount {
        let mut total = TripCount::Exact(Bound::zero());
        for id in expr_nodes(self.program, root) {
            let Some(SourceExpr::Unsupported {
                construct,
                bounded_by,
                declared,
                ..
            }) = self.program.expr(id)
            else {
                continue;
            };
            // A refusal that names a dominating expression is **not** a region.
            // It is arithmetic this engine cannot write down - `n // 2` - not
            // code it cannot see into: it costs nothing beyond the statement
            // evaluating it and it assigns to nothing, so no hole is charged
            // and `readable` survives. It is still accounted for, because
            // `reconciled` checks every `Unsupported` node whatever its shape,
            // and the operand it names has already been walked above so any
            // real region inside it was charged where it stands.
            if bounded_by.is_some() {
                self.charged.insert(NodeId::Expr(id));
                continue;
            }
            let (construct, declared, origin) = (*construct, *declared, self.expr_origin(id));
            self.charged.insert(NodeId::Expr(id));
            let region = self.region(construct, declared, origin);
            total = total.then(region);
        }
        total
    }

    /// The cost of the regions inside a condition, in arena order.
    fn cond_regions(&mut self, root: CondId) -> TripCount {
        let mut total = TripCount::Exact(Bound::zero());
        for id in cond_nodes(self.program, root) {
            match self.program.cond(id) {
                Some(SourceCond::Unsupported {
                    construct,
                    declared,
                    ..
                }) => {
                    let (construct, declared, origin) =
                        (*construct, *declared, self.cond_origin(id));
                    self.charged.insert(NodeId::Cond(id));
                    let region = self.region(construct, declared, origin);
                    total = total.then(region);
                }
                Some(SourceCond::Compare { left, right, .. }) => {
                    let (left, right) = (*left, *right);
                    total = total.then(self.expr_regions(left));
                    total = total.then(self.expr_regions(right));
                }
                _ => {}
            }
        }
        total
    }

    // -- absorption ---------------------------------------------------------
    //
    // Marking a node accounted for *without* charging it, because an enclosing
    // region already stands for the whole subtree it lies in. A `while` costs
    // one hole covering condition and body alike; charging the regions inside
    // it again would name them twice and put them in the bound beside the hole
    // that already covers them.

    /// Account for every region in `body` against an enclosing region.
    fn absorb_body(&mut self, body: &[StmtId]) {
        let mut work: Vec<StmtId> = body.iter().rev().copied().collect();
        let mut seen: BTreeSet<StmtId> = BTreeSet::new();
        while let Some(id) = work.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(stmt) = self.program.stmt(id) else {
                continue;
            };
            match stmt {
                SourceStmt::Assign { value, .. } => {
                    let value = *value;
                    self.absorb_expr(value);
                }
                SourceStmt::Return | SourceStmt::Raise => {}
                SourceStmt::Protected {
                    body,
                    handler,
                    cleanup,
                } => {
                    work.extend(body.iter().chain(handler).chain(cleanup).copied());
                }
                SourceStmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    let cond = *cond;
                    work.extend(then_body.iter().chain(else_body).copied());
                    self.absorb_cond(cond);
                }
                SourceStmt::While { cond, body } => {
                    let cond = *cond;
                    work.extend(body.iter().copied());
                    self.absorb_cond(cond);
                }
                SourceStmt::ForRange { range, body, .. } => {
                    let range = *range;
                    work.extend(body.iter().copied());
                    self.absorb_expr(range.start);
                    self.absorb_expr(range.stop);
                }
                SourceStmt::Unsupported { .. } => {
                    self.charged.insert(NodeId::Stmt(id));
                }
            }
        }
    }

    /// Account for every region in an expression against an enclosing region.
    fn absorb_expr(&mut self, root: ExprId) {
        for id in expr_nodes(self.program, root) {
            if matches!(self.program.expr(id), Some(SourceExpr::Unsupported { .. })) {
                self.charged.insert(NodeId::Expr(id));
            }
        }
    }

    /// Account for every region in a condition against an enclosing region.
    fn absorb_cond(&mut self, root: CondId) {
        for id in cond_nodes(self.program, root) {
            match self.program.cond(id) {
                Some(SourceCond::Unsupported { .. }) => {
                    self.charged.insert(NodeId::Cond(id));
                }
                Some(SourceCond::Compare { left, right, .. }) => {
                    let (left, right) = (*left, *right);
                    self.absorb_expr(left);
                    self.absorb_expr(right);
                }
                _ => {}
            }
        }
    }

    // -- positions ----------------------------------------------------------

    fn stmt_origin(&self, id: StmtId) -> Origin {
        self.program
            .stmt_origin(id)
            .cloned()
            .unwrap_or_else(|| self.program.origin().clone())
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
}

/// Remove the loop counter from a body's cost, so the result is a function of
/// the enclosing scope alone.
///
/// # The bug this exists to prevent
///
/// A loop counter is **bound by its loop**. A body whose cost mentions it -
/// `for i in range(n): for j in range(i): ...` - produces a cost in terms of
/// `i`, and multiplying that by the trip count leaves `i` free in the answer.
/// The caller can supply `n`; it has nothing to supply for `i`. Worse, the
/// result would be labelled exact, because every step that produced it was.
///
/// # Why substituting a ceiling is sound
///
/// `ceiling` dominates every value the counter holds - see
/// [`Walk::counter_ceiling`], which is where that argument lives, because it is
/// an argument about the *range* and this function never sees one. [`Bound`] is
/// **weakly monotone by construction**: replacing a variable by something that
/// dominates it can only increase the result. So the substitution is an
/// over-approximation, and one that needs no argument beyond the type's own
/// guarantee.
///
/// The result is relaxed, because it genuinely is looser - by
/// [`TripCount::substituting`], which relaxes *and keeps the hole ledger*. A
/// bound with a hole variable in it is not a complete answer no matter which
/// variant carries it, and rebuilding an `AtMost` from a bare [`Bound`] here is
/// what once published one as if it were.
///
/// # What this gives up, and what would recover it
///
/// The exact answer is the definite sum `sum over k in the range of body(k)`,
/// not `count * body(ceiling)`. For triangular nesting the sum is `n^2` and this
/// approximation gives `2n^2 + n` - the right shape, twice too large.
///
/// Recovering it means extracting the body's cost as a polynomial in the
/// counter and summing it in closed form. That is the tracked recurrence-
/// extraction work; it is deliberately not attempted here, because a loose
/// bound that says it is loose is worth shipping and a wrong exact one is not.
fn close_over_counter(target: &VarName, ceiling: Option<&Bound>, body: TripCount) -> TripCount {
    let counter = VarId::new(target.symbol().clone());
    let Some(body_bound) = body.bound() else {
        return TripCount::Unknown;
    };
    if !body_bound.may_contain_var(&counter) {
        return body;
    }
    // The counter escapes. Without an endpoint there is nothing to dominate it
    // with, so there is no sound bound to give.
    let Some(ceiling) = ceiling else {
        return TripCount::Unknown;
    };
    body.substituting(&counter, ceiling)
}

/// Every expression node reachable from `root`, parents before children.
///
/// Worklist-driven rather than recursive: expression depth is bounded only by
/// what a frontend built, and a recursive walk of a long chain is a stack
/// overflow - an abort, which takes the blame path with it and cannot be
/// caught. Visited nodes are skipped, so a shared subterm costs one visit
/// rather than one per reference, which also makes termination independent of
/// the builder's smaller-index guarantee.
fn expr_nodes(program: &SourceProgram, root: ExprId) -> Vec<ExprId> {
    let mut ordered = Vec::new();
    let mut seen: BTreeSet<ExprId> = BTreeSet::new();
    let mut work = vec![root];
    while let Some(id) = work.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = program.expr(id) else {
            continue;
        };
        ordered.push(id);
        match node {
            SourceExpr::Arith { left, right, .. } => work.extend([*right, *left]),
            SourceExpr::Neg { operand } => work.push(*operand),
            SourceExpr::Pow { base, .. } => work.push(*base),
            // A refusal that names an expression bounding its magnitude, or one
            // it evaluated on the way in, keeps that expression in the program,
            // so the walk must reach it: the regions inside `g(n) // 2` and the
            // inner call of `fetch(g(n))` are charged because the walk descends
            // here. See [`SourceExpr::Unsupported`].
            SourceExpr::Unsupported {
                bounded_by,
                evaluates,
                ..
            } => {
                work.extend(evaluates.iter().rev().copied());
                work.extend(*bounded_by);
            }
            SourceExpr::Int { .. } | SourceExpr::Var { .. } => {}
        }
    }
    ordered
}

/// Every condition node reachable from `root`, parents before children.
///
/// See [`expr_nodes`] for why this is a worklist.
fn cond_nodes(program: &SourceProgram, root: CondId) -> Vec<CondId> {
    let mut ordered = Vec::new();
    let mut seen: BTreeSet<CondId> = BTreeSet::new();
    let mut work = vec![root];
    while let Some(id) = work.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = program.cond(id) else {
            continue;
        };
        ordered.push(id);
        match node {
            SourceCond::And { left, right } | SourceCond::Or { left, right } => {
                work.extend([*right, *left]);
            }
            SourceCond::Not { operand } => work.push(*operand),
            SourceCond::Compare { .. } | SourceCond::Unsupported { .. } => {}
        }
    }
    ordered
}

/// Every variable the statements in `body` may change, or `None` if they may
/// change **anything**.
///
/// # Why an unanalysable region answers `None`
///
/// A region is code this engine cannot read. It may assign to any variable in
/// scope, and there is no over-approximation of "an unknown effect on the
/// integer state" short of "every name". Reporting a smaller set would let a
/// trip count be derived from a value the region had already changed, which is
/// the exact shape of the unsound bound `LAN-87a` removed.
fn writes_of(program: &SourceProgram, body: &[StmtId]) -> Option<BTreeSet<VarName>> {
    let mut names = BTreeSet::new();
    let mut seen: BTreeSet<StmtId> = BTreeSet::new();
    let mut work: Vec<StmtId> = body.iter().rev().copied().collect();
    while let Some(id) = work.pop() {
        if !seen.insert(id) {
            continue;
        }
        // A handle naming no node is a malformed program, and this analysis
        // declines to say what it writes.
        let stmt = program.stmt(id)?;
        match stmt {
            SourceStmt::Assign { target, value } => {
                if expr_has_region(program, *value) {
                    return None;
                }
                names.insert(target.clone());
            }
            // Neither leaving the function nor abandoning a body assigns to
            // anything; what a `Protected` writes is what the statements inside
            // it write, so the walk descends rather than giving up. Answering
            // `None` here would be sound and would cost the ticket its point:
            // it clears `readable` before the loop body is walked, so a nested
            // counted loop inside a `try` would lose the endpoint it is counted
            // by.
            SourceStmt::Return | SourceStmt::Raise => {}
            SourceStmt::Protected {
                body,
                handler,
                cleanup,
            } => {
                work.extend(body.iter().chain(handler).chain(cleanup).copied());
            }
            SourceStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                if cond_has_region(program, *cond) {
                    return None;
                }
                work.extend(then_body.iter().chain(else_body).copied());
            }
            SourceStmt::While { cond, body } => {
                if cond_has_region(program, *cond) {
                    return None;
                }
                work.extend(body.iter().copied());
            }
            SourceStmt::ForRange {
                target,
                range,
                body,
            } => {
                if expr_has_region(program, range.start) || expr_has_region(program, range.stop) {
                    return None;
                }
                names.insert(target.clone());
                work.extend(body.iter().copied());
            }
            // A declared statement writes nothing: the frontend said it cannot
            // rebind a local of this frame, and that is exactly the question
            // this walk asks. Answering `None` here would clear `readable`
            // before a loop body containing one is walked, which costs the loop
            // its own endpoint - the failure this declaration exists to remove,
            // one nesting level down.
            SourceStmt::Unsupported { declared, .. } => match declared {
                Some(effect) if !effect.rebinds_locals() => {}
                _ => return None,
            },
        }
    }
    Some(names)
}

/// Whether executing the statement `id` can leave the region it stands in
/// early - by returning, or by raising.
///
/// A `return` ends the run, so anything charged after it - the rest of a block,
/// the remaining iterations of a loop - is charged and not executed. That keeps
/// the number an over-approximation and stops it being an equality, which is
/// the difference between `Theta` and `O`.
///
/// # A `raise` is the same edge, and it is the one that can go wrong quietly
///
/// The trip count of a counted loop is arithmetic rather than inference
/// *because* nothing can leave the iteration space early. An exceptional exit
/// can. `for i in range(n): raise ValueError` executes two steps for every `n`,
/// and reporting `Theta(2n)` for it - complete, exact, no holes, offered to a
/// budget gate - is a bound the program does not meet in the one direction a
/// bound may not move. Charging the `raise` its step without also reporting the
/// edge is exactly how that answer is produced.
///
/// A [`landav_its::SourceStmt::Protected`] answers `true` unconditionally, and
/// deliberately without inspecting its handlers. Nothing in the fragment says
/// which exceptions a handler catches, a handler may raise on its own account,
/// and so may a cleanup - so there is no evidence that the enclosing loop runs
/// to completion, whatever the source text looks like.
fn exits_within(program: &SourceProgram, id: StmtId) -> bool {
    let mut seen: BTreeSet<StmtId> = BTreeSet::new();
    let mut work = vec![id];
    while let Some(id) = work.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(stmt) = program.stmt(id) else {
            continue;
        };
        match stmt {
            SourceStmt::Return | SourceStmt::Raise | SourceStmt::Protected { .. } => return true,
            SourceStmt::If {
                then_body,
                else_body,
                ..
            } => work.extend(then_body.iter().chain(else_body).copied()),
            SourceStmt::While { body, .. } | SourceStmt::ForRange { body, .. } => {
                work.extend(body.iter().copied());
            }
            SourceStmt::Assign { .. } | SourceStmt::Unsupported { .. } => {}
        }
    }
    false
}

/// Whether an expression contains a node this engine cannot read.
///
/// A refusal that names an expression dominating its magnitude does not count.
/// It is an arithmetic operator with no representation here, not an unknown
/// *effect*: it cannot assign to anything, so an assignment whose value
/// contains one still writes exactly its own target. See
/// [`SourceExpr::Unsupported`] and `Walk::expr_regions`, which draws the same
/// line for the same reason.
fn expr_has_region(program: &SourceProgram, root: ExprId) -> bool {
    expr_nodes(program, root).into_iter().any(|id| {
        matches!(
            program.expr(id),
            Some(SourceExpr::Unsupported {
                bounded_by: None,
                declared: None,
                ..
            })
        )
    })
}

/// Whether a condition contains a node this engine cannot read.
fn cond_has_region(program: &SourceProgram, root: CondId) -> bool {
    cond_nodes(program, root).into_iter().any(|id| {
        match program.cond(id) {
            // A declared node is not a region for the same reason a `bounded_by`
            // refusal is not: its *effect* is known. It costs a constant and
            // assigns to nothing, so a name read across it still holds the value
            // the caller supplied - which is the entire point of declaring it.
            Some(SourceCond::Unsupported { declared, .. }) => declared.is_none(),
            Some(SourceCond::Compare { left, right, .. }) => {
                expr_has_region(program, *left) || expr_has_region(program, *right)
            }
            // `And`, `Or` and `Not` are already in the walk's output.
            _ => false,
        }
    })
}
