//! `LAN-87` Phase 3: **the native engine, checked against a semantics it did
//! not write.**
//!
//! # Why this replaces the differential check rather than joining it
//!
//! `landav-engine`'s `differential.rs` compares the engine against KoAT2 over
//! the *lowered* system, so it can only look at programs that lower. `LAN-87`
//! makes the engine total over [`SourceProgram`] - it now answers for programs
//! that will never become a transition system, which is most of the corpus -
//! and there is structurally no solver to compare against on that territory.
//!
//! The comparison that still works is against the reference semantics in
//! `reference`: an operational interpreter written from the doc comments on
//! [`landav_its::SourceStmt`] and [`landav_its::RangeSpec`], which calls neither
//! `lower` nor `landav_engine::cost`. It already counted statements for the
//! soundness property; `LAN-87` added a second counter in the engine's own
//! declared unit - "one per statement executed, plus one per loop iteration for
//! the loop's own test and increment" - so the two numbers can be compared at
//! all. See [`crate::reference::Run::charged`].
//!
//! This is the same discipline the lowering's widenings already get, applied to
//! the other engine.
//!
//! # The two claims, and why they are separate tests
//!
//! * A **complete** result may never be smaller than the truth. That is the
//!   soundness half, and it is the one with a zero target: a bound offered for
//!   comparison against a budget that the program exceeds is worse than no
//!   bound at all.
//! * An **exact** result must equal the truth. That is the tightness half, and
//!   it is what makes `Theta` a different claim from `O`. An engine that
//!   labelled every over-approximation exact would pass the first property and
//!   fail this one.
//!
//! A `Partial` result is skipped by both: its unfilled hole denotes `omega`, so
//! it makes no finite claim and there is nothing to exceed. `Unknown` likewise.
//! That makes vacuity the live risk, and
//! [`the_corpus_produces_complete_bounds_to_compare`] measures it directly
//! rather than assuming it away.
//!
//! # Why the parameter is held non-negative
//!
//! A [`landav_bound::Bound`] denotes a value in `N u {omega}` and is evaluated
//! under a [`landav_bound::Valuation`] over naturals. A negative input has no
//! image there, so it is not a case the engine claims anything about, and
//! feeding one would be testing the harness rather than the engine.

use landav_bound::{Bound, Nat, Valuation, VarId};
use landav_engine::{Hole, cost};
use proptest::{prelude::*, strategy::ValueTree as _, test_runner::TestRunner};

use crate::{
    reference::{Ending, State, interpret, interpret_with},
    support::{
        ExprSpec, MUTABLE, Materialiser, PARAMS, StmtSpec, arb_body, arb_havoc_body,
        arb_raising_body, arb_state, raises_inside_a_loop,
    },
};

/// How many source statements a generated program may execute.
const STEP_BUDGET: u64 = 20_000;

/// The generated state, as a valuation over naturals.
///
/// Anything the state does not name reads as zero - which covers the mutable
/// locals, none of which a sound bound may mention anyway.
struct Inputs(State);

impl Valuation for Inputs {
    fn value_of(&self, var: &VarId) -> Nat {
        let named = self.0.get(var.symbol().as_str()).copied().unwrap_or(0);
        u64::try_from(named).map_or(Nat::Fin(0), Nat::Fin)
    }
}

/// Whether every parameter in `state` has a value a bound can be evaluated at.
fn is_natural(state: &State) -> bool {
    PARAMS
        .iter()
        .all(|param| state.get(*param).copied().unwrap_or(0) >= 0)
}

/// The reported cost at `state`, or `None` when the result makes no finite
/// claim.
fn reported(program: &landav_its::SourceProgram, state: &State) -> Option<(bool, Nat)> {
    let derived = cost(program);
    if !derived.is_complete() {
        return None;
    }
    let bound = derived.bound()?;
    Some((derived.is_exact(), bound.eval(&Inputs(state.clone()))))
}

/// The reported cost at `state` once every refused **binding** is filled in,
/// or `None` when something else is still unknown.
///
/// # Why a refused binding's hole is filled with zero
///
/// The engine charges a refused statement as its own step plus a hole standing
/// for the cost of the region - and a refused binding, as the reference
/// interprets it, *is* one step: it binds a name and does nothing else. So the
/// region's cost is nothing beyond the step already charged, and zero is the
/// truth rather than a convenient value. Every other hole - a `while`, a
/// counted loop that lost its endpoint - stands for real unknown work, and a
/// bound still mentioning one makes no finite claim, so the case is skipped as
/// the properties above skip a `Partial` result.
///
/// What is left is a bound in the caller's parameters that the engine derived
/// **past** the refused bindings, and that is exactly the claim `LAN-100`
/// makes: the values it kept reading across them were values the refusals
/// could not have changed.
fn reported_past_refused_bindings(
    program: &landav_its::SourceProgram,
    state: &State,
) -> Option<Nat> {
    let derived = cost(program);
    let mut bound = derived.bound()?.clone();
    for hole in derived.holes() {
        if hole.construct() == "non-integer-value" {
            bound = bound.subst(&hole.var(), &Bound::zero());
        }
    }
    if bound.vars().iter().any(Hole::is_hole) {
        return None;
    }
    assert!(
        mentions_only_supplied(&bound, true),
        "filled past its refused bindings, the bound {bound} mentions a name the \
         caller cannot supply"
    );
    Some(bound.eval(&Inputs(state.clone())))
}

/// Whether a bound mentions anything the caller cannot supply.
///
/// A hole variable is exempt only while the result is **partial**. A complete
/// result is one `derived::cost_of` publishes and a consumer may compare against
/// a budget; an unfilled hole denotes omega, so a hole variable in one is a
/// finite-looking claim with an infinite term in it, which every caller
/// supplying the parameters and nothing else reads as zero. Exempting holes
/// unconditionally is what let `close_over_counter` publish
/// `O(2 + n * (1 + #hole0 + 2n))` as a complete upper bound with an empty hole
/// ledger.
fn mentions_only_supplied(bound: &Bound, complete: bool) -> bool {
    bound
        .vars()
        .iter()
        .all(|var| PARAMS.contains(&var.symbol().as_str()) || (!complete && Hole::is_hole(var)))
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        // Every generated program terminates by construction - see `support` -
        // so nothing here is rejected. Cases the reference cannot follow are
        // *skipped* and counted separately, for the reason `soundness` gives.
        max_global_rejects: 0,
        ..ProptestConfig::default()
    })]

    /// **A complete bound is never smaller than the truth.**
    ///
    /// The soundness property for the second engine, stated in the same shape
    /// as the one for the lowering: one-directional, and the one that must
    /// never fail.
    #[test]
    fn a_complete_bound_is_never_exceeded(
        body in arb_body(),
        initial in arb_state(),
    ) {
        let program = Materialiser::new("scored").finish(&body);
        if !is_natural(&initial) {
            return Ok(());
        }
        let run = interpret(&program, &initial, STEP_BUDGET);
        if run.ending != Ending::Terminated {
            // The reference computes in `i128` and a generated program can
            // square a variable inside a nested loop. When it cannot say what
            // the program does there is nothing to compare against.
            return Ok(());
        }
        let Some((_, reported)) = reported(&program, &initial) else {
            return Ok(());
        };

        prop_assert!(
            reported.magnitude_cmp(Nat::Fin(run.charged)) != core::cmp::Ordering::Less,
            "the engine reported {reported:?} for a run that costs {} steps at {initial:?}",
            run.charged
        );
    }

    /// **An exact bound equals the truth.**
    ///
    /// `Theta` and `O` are different claims and the difference is the product.
    /// An engine that relabelled every over-approximation as exact would satisfy
    /// the property above and fail this one, which is why they are separate.
    #[test]
    fn an_exact_bound_equals_the_truth(
        body in arb_body(),
        initial in arb_state(),
    ) {
        let program = Materialiser::new("scored").finish(&body);
        if !is_natural(&initial) {
            return Ok(());
        }
        let run = interpret(&program, &initial, STEP_BUDGET);
        if run.ending != Ending::Terminated {
            return Ok(());
        }
        let Some((exact, reported)) = reported(&program, &initial) else {
            return Ok(());
        };
        if !exact {
            return Ok(());
        }

        prop_assert_eq!(
            reported,
            Nat::Fin(run.charged),
            "an exact bound must be the cost, not merely above it, at {:?}",
            initial
        );
    }

    /// **No bound mentions anything the caller cannot supply.**
    ///
    /// A local, a loop counter or a name from an enclosing scope has no referent
    /// outside the function body: `Bound::eval` needs a value for it and a
    /// caller who supplies the parameters and nothing else gets zero, which
    /// turns a quadratic cost into a constant. Asserted over every result,
    /// complete or partial, because a partial bound is reported too - and a
    /// *complete* one may not mention a hole either; see
    /// [`mentions_only_supplied`].
    #[test]
    fn no_bound_mentions_a_variable_the_caller_cannot_supply(
        body in arb_body(),
    ) {
        let program = Materialiser::new("scored").finish(&body);
        let derived = cost(&program);
        if let Some(bound) = derived.bound() {
            prop_assert!(
                mentions_only_supplied(bound, derived.is_complete()),
                "the bound {bound} mentions something the caller cannot supply"
            );
        }
    }

    /// **A run that ends by raising is not under-reported either.**
    ///
    /// The soundness half, extended to the corpus that raises. It was
    /// unreachable while `raise` was a whole-statement refusal: every program
    /// containing one came back `Partial`, whose unfilled hole denotes `omega`
    /// and makes no finite claim, so both properties above skipped it. Now that
    /// a `raise` costs one step and carries no hole, a raising program can
    /// produce a *complete* bound - and a complete bound is exactly the thing a
    /// budget gate compares against.
    ///
    /// [`Ending::Raised`] is a completed run: every step it performed is known,
    /// so there is a number to dominate. It is admitted here alongside
    /// [`Ending::Terminated`] for that reason, and kept apart from it in the
    /// reference for the reason the next property gives.
    #[test]
    fn a_run_that_raises_is_not_under_reported(
        body in arb_raising_body(),
        initial in arb_state(),
    ) {
        let program = Materialiser::new("scored").finish(&body);
        if !is_natural(&initial) {
            return Ok(());
        }
        let run = interpret(&program, &initial, STEP_BUDGET);
        if !matches!(run.ending, Ending::Terminated | Ending::Raised) {
            return Ok(());
        }
        let Some((_, reported)) = reported(&program, &initial) else {
            return Ok(());
        };

        prop_assert!(
            reported.magnitude_cmp(Nat::Fin(run.charged)) != core::cmp::Ordering::Less,
            "the engine reported {reported:?} for a run that costs {} steps at \
             {initial:?} and ended {:?}",
            run.charged,
            run.ending
        );
    }

    /// **A `raise` a loop can reach is never reported as an equality.**
    ///
    /// The claim is the *absence of the label*, not the value of the number,
    /// and that is deliberate. `for i in range(n): raise` executes two steps for
    /// every `n`; a bound of `1 + 2n` is perfectly sound and this property does
    /// not object to it. What it forbids is `Theta(1 + 2n)`, which asserts the
    /// program costs that much rather than at most that much, and at `n = 100`
    /// it costs two. Complete, exact, no holes, offered to a budget gate: the
    /// `LAN-88` failure class reached through a different construct.
    ///
    /// Extending [`an_exact_bound_equals_the_truth`] to raising programs would
    /// be the wrong test. An early exit makes exactness *unattainable*, so
    /// there is no number for the equality to hold at; what has to be checked
    /// is that the engine stops claiming one.
    ///
    /// # Why `exact_outside_holes` is not asserted here
    ///
    /// It is asserted in
    /// [`a_countable_loop_that_can_raise_is_not_exact_outside_its_holes`], over
    /// a generator that guarantees the loop was *counted*. This corpus does
    /// not: a `for` whose start is a mutable local has no expressible trip
    /// count, so the engine holes the whole loop - body, `raise` and all - and
    /// "exact outside the holes" is then a true statement about the three
    /// prologue assignments beside it. Asserting the flag here would be
    /// asserting that the engine failed to hole a loop it cannot count.
    #[test]
    fn a_raise_a_loop_can_reach_is_never_an_equality(
        body in arb_raising_body(),
    ) {
        if !raises_inside_a_loop(&body) {
            return Ok(());
        }
        let program = Materialiser::new("scored").finish(&body);
        let derived = cost(&program);

        prop_assert!(
            !derived.is_exact(),
            "a loop whose body can raise may stop before its counter is \
             exhausted, so its trip count is an over-estimate rather than an \
             equality: {:?}",
            derived.bound().map(ToString::to_string)
        );
    }

    /// **A loop the engine *counted* whose body can raise is not exact outside
    /// its holes either.**
    ///
    /// `exact_elsewhere` is the same equality claim as `Exact`, made about the
    /// derived part of a partial result, and a report renders it as "exact
    /// except for the region at line N". A loop an exception can leave is not
    /// among the things derived exactly, so the flag has to come down - and
    /// `TripCount::relax` is what clears it.
    ///
    /// The generator pins the loop to `range(0, e)` in unit steps with `e`
    /// literal or the parameter, which is the shape `count_of` can actually
    /// count. That is the whole difference from the property above: here the
    /// trip count really is in the bound, so there is an equality to withdraw.
    #[test]
    fn a_countable_loop_that_can_raise_is_not_exact_outside_its_holes(
        body in arb_countable_raising_loop(),
    ) {
        let program = Materialiser::new("scored").finish(&body);
        let derived = cost(&program);

        prop_assert!(
            !derived.is_exact() && !derived.exact_outside_holes(),
            "the trip count of this loop is in the bound and an exception can \
             end the loop before it is exhausted, so nothing enclosing it was \
             derived exactly: {:?}",
            derived.bound().map(ToString::to_string)
        );
    }

    /// **A strided or offset loop whose body reads its counter is not
    /// under-approximated.**
    ///
    /// The shared generator draws range literals from `-4..=6` and strides from
    /// `{+-1, +-2}`, which caps the gap between a counter's largest value and
    /// the loop's trip count at about two. `close_over_counter` substitutes one
    /// for the other, so a body linear in the counter only falls below the truth
    /// once that gap grows with the count - and inside that alphabet the joint
    /// probability of drawing a witness is around `1e-5`. The property was green
    /// at 40 000 cases against a live 42x counterexample.
    ///
    /// This generator is aimed at exactly that shape: wide literal endpoints, a
    /// stride that is not one, and an inner loop bounded by the outer counter.
    #[test]
    fn a_strided_loop_reading_its_counter_is_not_under_approximated(
        nest in arb_strided_nest(),
        initial in arb_state(),
    ) {
        let program = Materialiser::new("scored").finish(&nest);
        if !is_natural(&initial) {
            return Ok(());
        }
        let run = interpret(&program, &initial, STEP_BUDGET);
        if run.ending != Ending::Terminated {
            return Ok(());
        }
        let Some((_, reported)) = reported(&program, &initial) else {
            return Ok(());
        };

        prop_assert!(
            reported.magnitude_cmp(Nat::Fin(run.charged)) != core::cmp::Ordering::Less,
            "the engine reported {reported:?} for a strided nest costing {} \
             steps at {initial:?}. The counter of a loop is dominated by the \
             endpoint it moves away from, not by the trip count; the two are \
             the same number only for `range(0, e)` in unit steps.",
            run.charged
        );
    }
}

/// `for a in range(0, e): <assigns> raise` — a loop the engine can count whose
/// body can raise.
///
/// `range(0, e)` in unit steps with `e` a literal or the parameter is exactly
/// the shape `count_of` reads as a trip count, so the loop reaches the bound
/// instead of becoming one opaque hole. The `raise` is placed under a
/// condition half the time, because a guarded raise is what the corpus
/// actually contains and the engine performs no reachability analysis: it has
/// no evidence the loop runs to completion either way.
fn arb_countable_raising_loop() -> impl Strategy<Value = Vec<StmtSpec>> {
    (
        prop_oneof![
            (0_i64..=6).prop_map(ExprSpec::Int),
            // `READABLE` ends with the parameter.
            Just(ExprSpec::Var(3)),
        ],
        prop::collection::vec(
            (0_usize..MUTABLE.len(), 0_i64..4).prop_map(|(target, value)| StmtSpec::Assign {
                target,
                value: ExprSpec::Int(value),
            }),
            0..2,
        ),
        prop::bool::ANY,
        crate::support::arb_cond(),
    )
        .prop_map(|(stop, mut body, guarded, cond)| {
            let raise = if guarded {
                StmtSpec::If {
                    cond,
                    then_body: vec![StmtSpec::Raise],
                    else_body: Vec::new(),
                }
            } else {
                StmtSpec::Raise
            };
            body.push(raise);
            vec![StmtSpec::For {
                target: 0,
                start: ExprSpec::Int(0),
                stop,
                step: 1,
                body,
            }]
        })
}

/// `for i in range(start, stop, step): for j in range(0, i): a = 0`
///
/// The shape `close_over_counter` gets wrong when the counter is not bounded by
/// the trip count. Endpoints are drawn wide and strides go past one, which is
/// the whole difference from [`arb_body`].
fn arb_strided_nest() -> impl Strategy<Value = Vec<StmtSpec>> {
    (
        0_usize..MUTABLE.len(),
        -6_i64..=26,
        -6_i64..=26,
        prop_oneof![
            Just(1_i64),
            Just(2),
            Just(3),
            Just(5),
            Just(7),
            Just(-1),
            Just(-3),
            Just(-5),
        ],
        0_usize..MUTABLE.len(),
    )
        .prop_filter(
            "the two loops must count on different variables",
            |(outer, _, _, _, inner)| outer != inner,
        )
        .prop_map(|(outer, start, stop, step, inner)| {
            let assign = StmtSpec::Assign {
                target: (outer + 1) % MUTABLE.len(),
                value: ExprSpec::Int(0),
            };
            let inner_loop = StmtSpec::For {
                target: inner,
                start: ExprSpec::Int(0),
                // `READABLE` begins with `MUTABLE`, so this reads the outer
                // loop's counter.
                stop: ExprSpec::Var(outer),
                step: 1,
                body: vec![assign],
            };
            vec![StmtSpec::For {
                target: outer,
                start: ExprSpec::Int(start),
                stop: ExprSpec::Int(stop),
                step,
                body: vec![inner_loop],
            }]
        })
}

/// **The corpus produces complete bounds for the properties to hold over.**
///
/// The vacuity guard. Both properties above skip a result that is `Partial` or
/// `Unknown`, and a generator that produced nothing else would leave them green
/// and meaningless - the failure mode a raised case count exposes and a passing
/// CI run hides.
///
/// Measured rather than assumed, following `soundness`'s
/// `the_generator_mostly_produces_runs_the_reference_can_follow`.
#[test]
fn the_corpus_produces_complete_bounds_to_compare() {
    let mut runner = TestRunner::deterministic();
    let strategy = (arb_body(), arb_state());

    let mut examined = 0_usize;
    let mut compared = 0_usize;
    let mut exact = 0_usize;
    for _ in 0..512 {
        let Ok(case) = strategy.new_tree(&mut runner) else {
            continue;
        };
        let (body, initial): (Vec<StmtSpec>, State) = case.current();
        let program = Materialiser::new("scored").finish(&body);
        if !is_natural(&initial) {
            continue;
        }
        let run = interpret(&program, &initial, STEP_BUDGET);
        if run.ending != Ending::Terminated {
            continue;
        }
        examined += 1;
        if let Some((is_exact, _)) = reported(&program, &initial) {
            compared += 1;
            if is_exact {
                exact += 1;
            }
        }
    }

    assert!(
        examined > 0,
        "no generated program ran to completion, so nothing was scored at all"
    );
    // The bar is a tenth, against a measured rate of about one in seven at the
    // time of writing (45 of 327). Two things hold it down and both are
    // intended: the shared generator spends three tenths of its statement
    // weight on `while`, which this engine holes by design, and a range
    // endpoint reading one of the mutable locals is not a value the engine may
    // read - `LAN-87a` - so that loop becomes a region too. What must not
    // happen is the rate going to nothing, which is what this catches.
    assert!(
        compared * 10 >= examined,
        "only {compared} of {examined} scored program(s) produced a complete \
         bound, so the soundness and tightness properties above are close to \
         vacuous. Either the generator stopped producing analysable loops or \
         the engine stopped analysing them."
    );
    assert!(
        exact >= 5,
        "only {exact} of {compared} complete bound(s) was exact, so \
         `an_exact_bound_equals_the_truth` asserted almost nothing - and it is \
         the only test that distinguishes `Theta` from `O`"
    );
}

/// **The havoc corpus derives bounds past refused bindings, in the parameter.**
///
/// The vacuity guard for `a_bound_derived_past_a_refused_binding_is_never_exceeded`.
/// A generator whose refused bindings always cleared the frame - the behaviour
/// before `LAN-100` - would leave every case skipped at the first counted loop
/// past one, and the property green and empty. Measured rather than assumed.
#[test]
fn the_havoc_corpus_reads_the_parameter_past_refused_bindings() {
    let mut runner = TestRunner::deterministic();
    let strategy = (arb_havoc_body(), arb_state());

    let mut examined = 0_usize;
    let mut compared = 0_usize;
    let mut in_the_parameter = 0_usize;
    for _ in 0..512 {
        let Ok(case) = strategy.new_tree(&mut runner) else {
            continue;
        };
        let (body, initial): (Vec<StmtSpec>, State) = case.current();
        let (program, oracle) = Materialiser::new("havoc").finish_with_oracle(&body);
        if !is_natural(&initial) {
            continue;
        }
        let run = interpret_with(&program, &initial, STEP_BUDGET, &oracle);
        if run.ending != Ending::Terminated {
            continue;
        }
        examined += 1;
        if reported_past_refused_bindings(&program, &initial).is_some() {
            compared += 1;
            let derived = cost(&program);
            let mentions_parameter = derived.bound().is_some_and(|bound| {
                bound
                    .vars()
                    .iter()
                    .any(|var| PARAMS.contains(&var.symbol().as_str()))
            });
            if mentions_parameter {
                in_the_parameter += 1;
            }
        }
    }

    assert!(examined > 0, "no generated program ran to completion");
    assert!(
        compared * 10 >= examined,
        "only {compared} of {examined} program(s) produced a bound past their \
         refused bindings, so the property above is close to vacuous"
    );
    assert!(
        in_the_parameter >= 10,
        "only {in_the_parameter} of {compared} bound(s) derived past a refused \
         binding mention the parameter - the case `LAN-100` exists for, and the \
         only one where forgetting too little would show"
    );
}

/// **The witness: a refused binding of a local keeps the parameter readable.**
///
/// `x = <unreadable>; for i in range(0, n): a = 0`, spelled out rather than
/// generated, so that the thing `LAN-100` buys is asserted once in the plainest
/// possible terms: the bound is a function of `n`, and the only hole is the
/// refusal itself.
#[test]
fn a_refused_binding_of_a_local_keeps_the_parameter_readable() {
    let body = vec![
        StmtSpec::Havoc {
            target: 0,
            value: 7,
        },
        StmtSpec::For {
            target: 1,
            start: ExprSpec::Int(0),
            stop: ExprSpec::Var(3),
            step: 1,
            body: vec![StmtSpec::Assign {
                target: 2,
                value: ExprSpec::Int(0),
            }],
        },
    ];
    let (program, oracle) = Materialiser::new("witness").finish_with_oracle(&body);
    let derived = cost(&program);
    let bound = derived
        .bound()
        .expect("a loop over the parameter has a bound");
    assert!(
        bound.vars().iter().any(|var| var.symbol().as_str() == "n"),
        "the bound {bound} must be a function of `n`: the refused binding of `a` \
         cannot have changed it"
    );
    let holes: Vec<&str> = derived.holes().iter().map(Hole::construct).collect();
    assert_eq!(
        holes,
        vec!["non-integer-value"],
        "the refusal is the only region; the loop must be counted, not holed"
    );

    let initial: State = [("n".to_owned(), 5_i128)].into_iter().collect();
    let run = interpret_with(&program, &initial, STEP_BUDGET, &oracle);
    assert_eq!(run.ending, Ending::Terminated);
    let reported = reported_past_refused_bindings(&program, &initial)
        .expect("nothing but the refusal is unknown");
    assert!(
        reported.magnitude_cmp(Nat::Fin(run.charged)) != core::cmp::Ordering::Less,
        "reported {reported:?} for a run costing {}",
        run.charged
    );
}

/// **The witness's converse: a refused binding of the parameter forgets it.**
///
/// `n = <unreadable>; for i in range(0, n): a = 0`. The environment chooses
/// forty, the caller passed two, and a bound that still read `n` would be
/// exceeded twentyfold. The engine must lose the endpoint instead: the loop is
/// a hole, and the bound mentions no `n` at all.
#[test]
fn a_refused_binding_of_the_parameter_forgets_it() {
    let body = vec![
        StmtSpec::Havoc {
            target: 3,
            value: 40,
        },
        StmtSpec::For {
            target: 1,
            start: ExprSpec::Int(0),
            stop: ExprSpec::Var(3),
            step: 1,
            body: vec![StmtSpec::Assign {
                target: 2,
                value: ExprSpec::Int(0),
            }],
        },
    ];
    let (program, oracle) = Materialiser::new("witness").finish_with_oracle(&body);
    let derived = cost(&program);
    let bound = derived.bound().expect("the engine is total");
    assert!(
        !bound.vars().iter().any(|var| var.symbol().as_str() == "n"),
        "the bound {bound} reads `n` past a statement that rebound it"
    );
    assert!(
        derived.holes().iter().any(|hole| hole.construct() == "for"),
        "the loop lost its endpoint and must be a hole; holes are {:?}",
        derived.holes()
    );

    let initial: State = [("n".to_owned(), 2_i128)].into_iter().collect();
    let run = interpret_with(&program, &initial, STEP_BUDGET, &oracle);
    assert_eq!(run.ending, Ending::Terminated);
    assert!(
        reported_past_refused_bindings(&program, &initial).is_none(),
        "with the loop holed there is no finite claim to make"
    );
}

/// **The two units are genuinely different numbers.**
///
/// [`crate::reference::Run::charged`] exists because the engine counts a loop
/// differently from this interpreter, and a scoring function that had
/// accidentally been written to equal `steps` would make the properties above
/// compare the engine against the wrong semantics - and pass, because the
/// difference is a small additive one on the shapes that matter least.
///
/// `for i in range(0, 3): a = 0` is the smallest witness: two statements per
/// iteration in the engine's unit, and the same plus the loop statement and the
/// test that ends it in this one.
#[test]
fn the_engine_unit_is_not_the_interpreters_unit() {
    let body = vec![StmtSpec::For {
        target: 0,
        start: crate::support::ExprSpec::Int(0),
        stop: crate::support::ExprSpec::Int(3),
        step: 1,
        body: vec![StmtSpec::Assign {
            target: 1,
            value: crate::support::ExprSpec::Int(0),
        }],
    }];
    let program = Materialiser::new("units").finish(&body);
    let run = interpret(&program, &State::new(), STEP_BUDGET);

    // Three prologue assignments, then the loop: 3 + 3 * (1 + 1) = 9.
    assert_eq!(
        run.charged, 9,
        "the engine's unit charges one per statement and one per iteration"
    );
    assert!(
        run.steps > run.charged,
        "this interpreter additionally counts the loop statement and the test \
         that ends the loop, so it must be the larger of the two: steps={} \
         charged={}",
        run.steps,
        run.charged
    );
    assert_eq!(
        cost(&program).bound().map(ToString::to_string),
        Some("9".to_owned()),
        "and the engine agrees with the scoring, exactly"
    );
}

/// **The raising corpus contains the shapes the two new properties need.**
///
/// The vacuity guard for [`a_run_that_raises_is_not_under_reported`] and
/// [`a_raise_a_loop_can_reach_is_never_an_equality`]. The first skips any run
/// the reference cannot follow; the second skips any body with no `raise`
/// inside a loop. A generator drifting away from either shape would leave both
/// green and measuring nothing, which is the failure mode a raised case count
/// exposes and a passing CI run hides.
#[test]
fn the_raising_corpus_contains_raises_a_loop_can_reach() {
    let mut runner = TestRunner::deterministic();
    let strategy = (arb_raising_body(), arb_state());

    let mut programs = 0_usize;
    let mut in_a_loop = 0_usize;
    let mut raised = 0_usize;
    let mut complete = 0_usize;
    for _ in 0..512 {
        let Ok(case) = strategy.new_tree(&mut runner) else {
            continue;
        };
        let (body, initial): (Vec<StmtSpec>, State) = case.current();
        if !is_natural(&initial) {
            continue;
        }
        programs += 1;
        if raises_inside_a_loop(&body) {
            in_a_loop += 1;
        }
        let program = Materialiser::new("scored").finish(&body);
        let run = interpret(&program, &initial, STEP_BUDGET);
        if run.ending == Ending::Raised {
            raised += 1;
            if reported(&program, &initial).is_some() {
                complete += 1;
            }
        }
    }

    assert!(
        programs > 0,
        "the raising generator produced nothing at all"
    );
    assert!(
        in_a_loop * 10 >= programs,
        "only {in_a_loop} of {programs} generated bodies put a `raise` where a \
         loop can reach it, so `a_raise_a_loop_can_reach_is_never_an_equality` \
         asserted almost nothing"
    );
    assert!(
        raised * 10 >= programs,
        "only {raised} of {programs} generated runs actually ended by raising, \
         so `a_run_that_raises_is_not_under_reported` was close to vacuous"
    );
    assert!(
        complete > 0,
        "no run that ended by raising produced a *complete* bound, so nothing \
         was compared: a partial result makes no finite claim and is skipped"
    );
}
