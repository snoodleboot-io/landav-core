//! **Soundness has a zero target**, stated as a property.
//!
//! # The claim
//!
//! An integer transition system that admits *fewer* executions than the
//! program has is unsound: a solver's bound on it can be exceeded by the
//! program. So the property is one-directional, and it is the one that must
//! never fail:
//!
//! > for every program in the fragment and every initial valuation, if the
//! > program terminates in state `S` after `k` steps, then the emitted system
//! > has a run reaching its exit location in state `S` taking at least `k`
//! > transitions.
//!
//! Both halves matter. Matching `S` says the *state* was tracked faithfully;
//! `k` transitions says the *runtime* was not undercounted, which is the half
//! a bound is derived from.
//!
//! # And the sharper claim, which is not required but is true
//!
//! On the exact fragment the emitted system is additionally **deterministic**:
//! at every reachable configuration exactly one successor is available. That
//! is a stronger statement than over-approximation and a much more sensitive
//! test. A lowering whose `if` guards overlapped, or left a gap between them,
//! would still admit every real execution — and would still be caught here,
//! because it would branch or get stuck.
//!
//! Determinism is asserted only where it is claimed. Where the lowering widens
//! (see `fragment::widening`) the system branches on purpose, and the weaker
//! admission property is what holds.

use landav_its::lower;
use proptest::prelude::*;

use crate::{
    reference::{Ending, Simulation, interpret, simulate},
    support::{Materialiser, StmtSpec, arb_body, arb_state, vacuity},
};

/// How many source statements a generated program may execute.
const STEP_BUDGET: u64 = 20_000;

/// The transition budget for a source run that took `steps` statements.
///
/// Derived from the source run rather than fixed, and deliberately tight. A
/// correct lowering takes a small constant factor more transitions than the
/// program takes statements — a loop header costs one transition per test, an
/// empty block costs one — so eight times plus a constant is generous.
///
/// The reason not to pick a large fixed number: when the lowering is *wrong*
/// in the way that matters most, the emitted system does not terminate, and a
/// large budget means every one of the five hundred cases burns it before
/// failing. A tight budget turns a soundness failure from a several-minute
/// timeout into a fast, legible one — which matters for `cargo mutants`, where
/// a timeout and a caught mutant are recorded differently.
fn transition_budget(steps: u64) -> u64 {
    steps.saturating_mul(8).saturating_add(256)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        // No case is rejected: every generated program terminates by
        // construction (see `support`), so a reject budget would be
        // measuring nothing. If this ever needs raising, the generator has
        // stopped doing its job and that is the bug to fix.
        max_global_rejects: 0,
        ..ProptestConfig::default()
    })]

    /// **The soundness property.** Every execution the program has, the
    /// system admits — in the same final state, taking at least as many
    /// transitions.
    #[test]
    fn the_system_admits_every_execution_the_program_has(
        body in arb_body(),
        initial in arb_state(),
    ) {
        let program = Materialiser::new("generated").finish(&body);
        let its = match lower(&program) {
            Ok(its) => its,
            Err(error) => {
                // Generated programs contain no unsupported construct, so a
                // refusal here is a defect in the lowering rather than a
                // property of the program.
                prop_assert!(false, "generated program was refused: {error}");
                return Ok(());
            }
        };

        let run = interpret(&program, &initial, STEP_BUDGET);
        if run.ending != Ending::Terminated {
            // The reference computes in `i128`, and a generated program can
            // square a variable inside a nested loop, which leaves that range
            // in a handful of iterations. When it does, the reference cannot
            // say what the program computes, so there is nothing to compare
            // against and the case is skipped.
            //
            // This is a *skip*, not a proptest reject: rejects are budgeted
            // and a raised case count can turn a healthy rate into a CI
            // failure. The rate is instead measured directly, and asserted, by
            // `the_generator_mostly_produces_runs_the_reference_can_follow`
            // below — so the suite cannot go quietly vacuous.
            return Ok(());
        }

        match simulate(&its, &initial, transition_budget(run.steps)) {
            Simulation::Terminated { state, length } => {
                for (name, expected) in &run.state {
                    prop_assert_eq!(
                        state.get(name),
                        Some(expected),
                        "variable `{}` ended at {:?} in the system and {} in the program",
                        name,
                        state.get(name),
                        expected
                    );
                }
                prop_assert!(
                    length >= run.steps,
                    "the system reached its exit in {} transitions but the program took {} \
                     steps: a bound derived from this system would be too small",
                    length,
                    run.steps
                );
            }
            other => prop_assert!(
                false,
                "the system did not reproduce a terminating run: {other:?}"
            ),
        }
    }

    /// **Exactness.** On this fragment the emitted system never branches and
    /// never gets stuck.
    ///
    /// Redundant with the property above only in the sense that a *correct*
    /// lowering satisfies both. It is far more sensitive: an `if` whose two
    /// guards overlapped would branch here, and one that left a gap would get
    /// stuck, while both would still admit every real execution.
    #[test]
    fn the_system_is_deterministic_on_the_exact_fragment(
        body in arb_body(),
        initial in arb_state(),
    ) {
        let program = Materialiser::new("generated").finish(&body);
        let Ok(its) = lower(&program) else {
            prop_assert!(false, "generated program was refused");
            return Ok(());
        };
        let run = interpret(&program, &initial, STEP_BUDGET);
        if run.ending != Ending::Terminated {
            // As above: outside the reference's range, so nothing to compare.
            return Ok(());
        }

        match simulate(&its, &initial, transition_budget(run.steps)) {
            Simulation::Terminated { .. } => {}
            Simulation::Nondeterministic { location, options } => prop_assert!(
                false,
                "location l{location} offered {options} distinct successors; on the exact \
                 fragment the branch guards must partition the state space"
            ),
            Simulation::Stuck { location } => prop_assert!(
                false,
                "location l{location} offered no successor; the branch guards leave a gap, \
                 so some executions of the program have no run in the system"
            ),
            other => prop_assert!(false, "simulation did not terminate: {other:?}"),
        }
    }

    /// A loop body that assigns to the loop variable, or to a variable the
    /// endpoint mentions, must not change the trip count.
    ///
    /// This is the counted loop's whole soundness argument, and it fails in
    /// **both** directions if the desugaring gets it wrong: counting on the
    /// visible loop variable lets a body shorten the loop (unsound: fewer
    /// executions), and re-reading `stop` each iteration lets a body lengthen
    /// it (unsound the other way, and unbounded).
    #[test]
    fn a_body_that_writes_the_loop_variable_does_not_change_the_trip_count(
        stop in 0_i64..6,
        interference in prop::collection::vec(0_usize..3, 1..3),
        initial in arb_state(),
    ) {
        use crate::support::ExprSpec;

        // for a in range(0, stop): <assign to a, b, n-ish targets>
        let body: Vec<StmtSpec> = interference
            .iter()
            .map(|target| StmtSpec::Assign {
                target: *target,
                value: ExprSpec::Int(99),
            })
            .collect();

        let specs = vec![StmtSpec::For {
            target: 0,
            start: ExprSpec::Int(0),
            stop: ExprSpec::Int(stop),
            step: 1,
            body,
        }];

        let program = Materialiser::new("interfered").finish(&specs);
        let Ok(its) = lower(&program) else {
            prop_assert!(false, "lowering refused a program inside the fragment");
            return Ok(());
        };

        let run = interpret(&program, &initial, STEP_BUDGET);
        // This program only ever assigns the literal 99, so it stays in range.
        prop_assert_eq!(run.ending.clone(), Ending::Terminated);

        match simulate(&its, &initial, transition_budget(run.steps)) {
            Simulation::Terminated { state, length } => {
                for (name, expected) in &run.state {
                    prop_assert_eq!(state.get(name), Some(expected), "variable `{}`", name);
                }
                prop_assert!(length >= run.steps);
            }
            other => prop_assert!(false, "simulation did not terminate: {other:?}"),
        }
    }
}

/// The generated corpus is not vacuous.
///
/// A soundness suite over programs that contained no loops would pass and mean
/// nothing, and nothing in a green CI run would say so. This samples the
/// generator directly and asserts the shape of what it produces, so that a
/// change to `arb_body`'s weights which quietly stopped emitting nested loops
/// fails here rather than silently weakening every property above.
#[test]
fn the_generator_produces_loops_conditionals_and_nesting() {
    use proptest::{
        strategy::{Strategy, ValueTree},
        test_runner::{Config, TestRunner},
    };

    let mut runner = TestRunner::new(Config {
        cases: 512,
        ..Config::default()
    });
    let strategy = arb_body();
    let mut sampled = Vec::new();
    for _ in 0..512 {
        match strategy.new_tree(&mut runner) {
            Ok(tree) => sampled.push(tree.current()),
            Err(error) => panic!("generator failed to produce a value: {error}"),
        }
    }

    let measured = vacuity(&sampled);
    assert_eq!(measured.programs, 512);
    assert!(
        measured.with_loops * 100 >= measured.programs * 50,
        "only {} of {} generated programs contain a loop; the soundness properties are \
         mostly running over straight-line code",
        measured.with_loops,
        measured.programs
    );
    assert!(
        measured.with_nested_loops * 100 >= measured.programs * 5,
        "only {} of {} generated programs contain a nested loop, and nested loops are \
         acceptance criterion 2",
        measured.with_nested_loops,
        measured.programs
    );
    assert!(
        measured.with_conditionals * 100 >= measured.programs * 30,
        "only {} of {} generated programs contain a conditional",
        measured.with_conditionals,
        measured.programs
    );
}

/// **The skip rate is measured, not assumed.**
///
/// `the_system_admits_every_execution_the_program_has` skips a case whose
/// source run leaves the reference's `i128` range. A skip is invisible: a
/// generator change that pushed every program out of range would leave the
/// soundness property passing on nothing at all, and a green CI run would say
/// so in no way whatsoever.
///
/// So the rate is asserted here instead. This is deliberately *not* a proptest
/// reject budget: a reject budget is a threshold on a quantity nobody looks at
/// until it trips, and raising the case count changes when it trips. A
/// measured fraction with an explicit floor says the same thing and says it
/// out loud.
#[test]
fn the_generator_mostly_produces_runs_the_reference_can_follow() {
    use proptest::{
        strategy::{Strategy, ValueTree},
        test_runner::{Config, TestRunner},
    };

    let mut runner = TestRunner::new(Config {
        cases: 512,
        ..Config::default()
    });
    let bodies = arb_body();
    let states = arb_state();

    let total = 512;
    let mut followed = 0_usize;
    for _ in 0..total {
        let (Ok(body), Ok(state)) = (bodies.new_tree(&mut runner), states.new_tree(&mut runner))
        else {
            panic!("generator failed to produce a value");
        };
        let program = Materialiser::new("sampled").finish(&body.current());
        if interpret(&program, &state.current(), STEP_BUDGET).ending == Ending::Terminated {
            followed += 1;
        }
    }

    assert!(
        followed * 100 >= total * 70,
        "only {followed} of {total} generated runs stayed inside the reference's range, so \
         the soundness property is skipping most of its cases"
    );
}
