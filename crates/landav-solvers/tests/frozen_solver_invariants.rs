//! The invariants that stand between this crate and a **hang**, an **abort**,
//! or a silently misattributed bound.
//!
//! # Why this is a separate, fast test target
//!
//! Three of this crate's loops have no independent bound of their own. They
//! terminate, or stay inside a sane allocation, because a limit is enforced
//! somewhere else:
//!
//! | loop | stays finite because |
//! |---|---|
//! | `koat_answer`'s expansion of `Arg_i^k` into `k` factors | [`MAX_EXPONENT`] rejects `k` *before* a single factor is built |
//! | `run`'s `try_wait` poll of the child process | the loop counts to [`poll_budget`], a pure function of the timeout, rather than testing a clock |
//! | the answer tokeniser | [`MAX_ANSWER_BYTES`] caps how much of the solver's output is ever read |
//!
//! `landav-bound`'s own `frozen_invariants.rs` records why this shape exists:
//! a mutant that deletes a guard produces **non-termination**, and a hang is
//! invisible to the panic lints and indistinguishable from slow CI. Every
//! assertion below is therefore about the *limit*, and this file deliberately
//! **never runs the loop the limit protects**. Nothing here spawns a process,
//! and nothing here asks the parser to expand a large exponent — it asks only
//! whether the parser refuses it.
//!
//! Cargo runs integration targets in name order, so `frozen_solver_invariants`
//! completes before `invocation`, `koat_answers` and `loat_answers` start.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// The whole file asserts on constants; that is its purpose, not an oversight.
#![allow(clippy::assertions_on_constants)]

use std::time::Duration;

use landav_solvers::{
    ArgMap, Direction, Growth, KOAT_TIMEOUT_GRACE_SECS, MAX_ANSWER_BYTES, MAX_ANSWER_TOKENS,
    MAX_ARGS, MAX_EXPONENT, MAX_MEASURE_STEPS, MAX_NESTING, MAX_TIMEOUT_SECS, MIN_TIMEOUT_SECS,
    POLL_INTERVAL_MILLIS, Solver, SolverError, Timeout, koat_answer, poll_budget,
};

/// A map of `arity` positional arguments named `a0..`, all of them parameters.
fn args(arity: usize) -> ArgMap {
    let names: Vec<String> = (0..arity).map(|i| format!("a{i}")).collect();
    ArgMap::new(names.clone(), names).unwrap_or_else(|_| ArgMap::empty())
}

// ---------------------------------------------------------------------------
// MAX_EXPONENT: the only bound on the `Arg_i^k` expansion loop
// ---------------------------------------------------------------------------

/// `Arg_0^k` is expanded into a product of `k` factors, because
/// [`landav_bound::Bound::pow`] raises a *constant* base to a symbolic power
/// and is therefore the wrong operator for a monomial. The expansion loop has
/// no cap of its own: `k` comes from the solver's output, which is text this
/// crate does not control. At `k = u32::MAX` the loop asks for a `Vec` of four
/// billion handles, and a `Vec` that cannot grow calls `handle_alloc_error`,
/// which **aborts** — past the reach of `unwrap_used`, `panic` and
/// `#![forbid(unsafe_code)]` alike.
///
/// The guard runs *before* the first factor is built, which is what lets this
/// assertion be cheap: with the guard in place nothing is allocated, and with
/// the guard removed the parse succeeds and this test fails on the assertion
/// rather than on the clock.
#[test]
fn an_exponent_past_the_limit_is_refused_before_anything_is_expanded() {
    let map = args(1);
    let over = u64::from(MAX_EXPONENT) + 1;
    let refused = koat_answer::parse(&format!("Arg_0^{over} {{O(n^{over})}}"), &map);
    assert!(
        matches!(refused, Err(SolverError::ExponentTooLarge { got, limit })
            if got == over && limit == MAX_EXPONENT),
        "Arg_0^{over} must be ExponentTooLarge; the expansion loop has no other bound, \
         got {refused:?}"
    );
}

/// The limit itself must stay small enough that the *permitted* expansion is
/// harmless. A limit of `u32::MAX` is a guard that is present and useless.
#[test]
fn the_exponent_limit_is_small_enough_that_the_permitted_expansion_is_cheap() {
    assert!(
        (1..=64).contains(&MAX_EXPONENT),
        "MAX_EXPONENT is {MAX_EXPONENT}; at least 1 or `Arg_0^1` is unparsable, at most 64 \
         or the expansion it permits is no longer cheap"
    );
}

/// The boundary itself is accepted, so the guard is an upper limit rather than
/// an off-by-one that quietly narrows what this crate can read.
#[test]
fn the_exponent_limit_is_inclusive() {
    let map = args(1);
    let at = MAX_EXPONENT;
    let parsed = koat_answer::parse(&format!("Arg_0^{at} {{O(n^{at})}}"), &map);
    assert!(
        parsed.is_ok(),
        "Arg_0^{at} is exactly at the limit and must be accepted, got {parsed:?}"
    );
}

// ---------------------------------------------------------------------------
// poll_budget: the only bound on the child-process wait loop
// ---------------------------------------------------------------------------

/// The wait loop counts to `poll_budget(timeout)` instead of asking a clock
/// whether it should stop. A loop whose only exit is `Instant::now() >=
/// deadline` hangs forever the moment a mutant weakens the comparison, and a
/// hung `cargo test` reports nothing at all about its siblings. Counting to a
/// pure, testable number means the loop is finite *by construction* and the
/// clock is only there to make it stop early.
#[test]
fn the_wait_loop_counts_to_a_finite_number_of_polls() {
    for secs in [MIN_TIMEOUT_SECS, 2, 30, MAX_TIMEOUT_SECS] {
        let budget = Timeout::new(secs).map(poll_budget);
        assert!(
            matches!(budget, Ok(polls) if polls >= 1),
            "poll_budget for a {secs}s timeout must be at least one poll, got {budget:?}"
        );
    }
}

/// A budget of one poll would kill every child that did not finish inside a
/// single poll interval, which turns a working solver into a timeout. The
/// default must leave real room.
#[test]
fn the_default_timeout_buys_a_useful_number_of_polls() {
    let polls = poll_budget(Timeout::DEFAULT);
    assert!(
        polls >= 100,
        "the default timeout buys only {polls} polls; the child would be killed long \
         before its deadline"
    );
}

/// More time must never buy fewer polls. A non-monotone budget is a timeout
/// that gets shorter as the caller asks for longer.
#[test]
fn a_longer_timeout_never_buys_fewer_polls() {
    let mut previous = 0;
    for secs in [MIN_TIMEOUT_SECS, 2, 5, 30, 600, MAX_TIMEOUT_SECS] {
        let Ok(timeout) = Timeout::new(secs) else {
            panic!("{secs} is inside the permitted range and must be accepted");
        };
        let polls = poll_budget(timeout);
        assert!(
            polls >= previous,
            "poll_budget fell from {previous} to {polls} when the timeout grew to {secs}s"
        );
        previous = polls;
    }
}

// ---------------------------------------------------------------------------
// Timeout: the value the whole wait loop is derived from
// ---------------------------------------------------------------------------

/// A timeout of zero makes `poll_budget` zero, which kills every child before
/// it starts; an unbounded timeout is not a timeout. Both are refused at the
/// type, so no call site can construct one.
#[test]
fn a_timeout_outside_the_permitted_range_is_refused() {
    for bad in [0, MAX_TIMEOUT_SECS + 1, u64::MAX] {
        let refused = Timeout::new(bad);
        assert!(
            matches!(refused, Err(SolverError::TimeoutOutOfRange { got, .. }) if got == bad),
            "Timeout::new({bad}) must be TimeoutOutOfRange, got {refused:?}"
        );
    }
}

/// Validation must not *alter* the value it accepts: `poll_budget` and the
/// solver's own `--timeout` flag are both derived from what `Timeout` reports
/// back, so a getter that disagrees with the constructor re-opens the hole the
/// constructor closes.
#[test]
fn a_validated_timeout_reports_the_value_it_was_validated_from() {
    for good in [MIN_TIMEOUT_SECS, 1, 25, 30, MAX_TIMEOUT_SECS] {
        assert_eq!(
            Timeout::new(good).ok().map(Timeout::seconds),
            Some(good),
            "Timeout::new({good}) must be accepted and report {good} back"
        );
        assert_eq!(
            Timeout::new(good).ok().map(Timeout::duration),
            Some(Duration::from_secs(good)),
            "Timeout::duration must agree with Timeout::seconds for {good}"
        );
    }
}

/// The default bypasses [`Timeout::new`], so it is a second, unchecked way to
/// inhabit the type and must agree with the checked path.
#[test]
fn the_default_timeout_agrees_with_the_checked_path() {
    let secs = Timeout::DEFAULT.seconds();
    assert!(
        (MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&secs),
        "the default timeout of {secs}s is outside the range Timeout::new enforces"
    );
    assert_eq!(Timeout::new(secs).ok(), Some(Timeout::DEFAULT));
}

/// The range must be a range. `MIN > MAX` makes `Timeout::new` total in the
/// wrong direction — it would refuse everything.
#[test]
fn the_timeout_range_is_non_empty() {
    assert!(
        MIN_TIMEOUT_SECS >= 1,
        "a minimum of zero permits a timeout that kills every child immediately"
    );
    assert!(
        MIN_TIMEOUT_SECS < MAX_TIMEOUT_SECS,
        "the permitted timeout range {MIN_TIMEOUT_SECS}..={MAX_TIMEOUT_SECS} is empty"
    );
}

// ---------------------------------------------------------------------------
// MAX_ANSWER_BYTES / MAX_ANSWER_TOKENS: what caps the tokeniser
// ---------------------------------------------------------------------------

/// The tokeniser walks the solver's output once, so it terminates on the
/// length of that output alone — but the output comes from a process this
/// crate does not control and can be arbitrarily long. Both caps must be
/// present and neither may be zero, or every answer is refused.
#[test]
fn the_answer_size_caps_are_present_and_usable() {
    assert!(
        (1024..=16 * 1024 * 1024).contains(&MAX_ANSWER_BYTES),
        "MAX_ANSWER_BYTES is {MAX_ANSWER_BYTES}; too small refuses ordinary output, too \
         large is not a cap"
    );
    assert!(
        (64..=1 << 20).contains(&MAX_ANSWER_TOKENS),
        "MAX_ANSWER_TOKENS is {MAX_ANSWER_TOKENS}"
    );
    assert!(
        (1..=1024).contains(&MAX_NESTING),
        "MAX_NESTING is {MAX_NESTING}; the shunting-yard stack grows one entry per open \
         `log(`"
    );
}

/// An output past the cap is refused rather than truncated. Truncating it
/// would hand the parser the *prefix* of a bound, and the prefix of
/// `Arg_0^2+3` is `Arg_0`, which is a smaller upper bound than the solver
/// stated — the one failure class with a zero target.
#[test]
fn output_past_the_size_cap_is_refused_rather_than_truncated() {
    let map = args(1);
    let padding = " ".repeat(MAX_ANSWER_BYTES + 1);
    let refused = koat_answer::parse(&format!("{padding}Arg_0+2 {{O(n)}}"), &map);
    assert!(
        matches!(refused, Err(SolverError::OutputTooLarge { limit, .. }) if limit == MAX_ANSWER_BYTES),
        "an over-long answer must be OutputTooLarge, never a parse of its prefix, got \
         {refused:?}"
    );
}

// ---------------------------------------------------------------------------
// MAX_ARGS: the positional map the whole `Arg_i` mapping rests on
// ---------------------------------------------------------------------------

/// `ArgMap` is indexed by a `u32` read out of solver output. The declared list
/// is what makes an out-of-range index detectable, so it must itself be
/// bounded and non-empty at construction.
#[test]
fn a_variable_list_past_the_limit_is_refused() {
    let names: Vec<String> = (0..=MAX_ARGS).map(|i| format!("a{i}")).collect();
    let refused = ArgMap::new(names, Vec::new());
    assert!(
        matches!(refused, Err(SolverError::TooManyVariables { limit, .. }) if limit == MAX_ARGS),
        "a variable list past MAX_ARGS must be refused, got {refused:?}"
    );
}

/// An index the declaration does not cover is a **refusal**, never a guess.
/// This is the assertion that stands between the crate and the worst failure
/// available to it: attributing a bound to the wrong variable, which is a
/// wrong answer that looks right.
#[test]
fn an_argument_index_past_the_declaration_is_refused() {
    let map = args(2);
    let refused = map.name(2);
    assert!(
        matches!(refused, Err(SolverError::ArgIndexOutOfRange { index, declared, .. })
            if index == 2 && declared == 2),
        "Arg_2 against a two-variable system must be refused, got {refused:?}"
    );
    assert!(map.name(0).is_ok() && map.name(1).is_ok());
}

// ---------------------------------------------------------------------------
// the frozen direction of each solver
// ---------------------------------------------------------------------------

/// KoAT bounds above and LoAT bounds below. Swapping them turns a lower bound
/// into a reported upper bound, which is a bound the program exceeds by
/// construction. Nothing about this pair is configurable and nothing infers it
/// from the output.
#[test]
fn each_solver_bounds_in_exactly_one_frozen_direction() {
    assert_eq!(Solver::Koat.direction(), Direction::Upper);
    assert_eq!(Solver::Loat.direction(), Direction::Lower);
    assert_ne!(Direction::Upper, Direction::Lower);
}

/// The growth lattice is the only thing an upper and a lower answer are ever
/// compared through, so its order must be the asymptotic one.
#[test]
fn the_growth_order_is_the_asymptotic_order() {
    let ascending = [
        Growth::Constant,
        Growth::Logarithmic,
        Growth::Polynomial(1),
        Growth::Polynomial(2),
        Growth::Polynomial(3),
        Growth::Exponential,
        Growth::Unbounded,
    ];
    for pair in ascending.windows(2) {
        assert!(
            pair[0] < pair[1],
            "{:?} must rank below {:?}",
            pair[0],
            pair[1]
        );
    }
}

/// Degree zero is not a polynomial class of its own — `O(n^0)` is `O(1)` — and
/// leaving it representable would put a value below `Logarithmic` in the
/// derived order while spelling itself as a polynomial.
#[test]
fn a_polynomial_of_degree_zero_is_the_constant_class() {
    assert_eq!(Growth::polynomial(0), Growth::Constant);
    assert_eq!(Growth::polynomial(1), Growth::Polynomial(1));
    assert!(Growth::polynomial(0) < Growth::Logarithmic);
}

// ---------------------------------------------------------------------------
// MAX_MEASURE_STEPS: the only bound on the growth measurement's worklist
// ---------------------------------------------------------------------------

/// The growth measurement walks the parsed bound's DAG with an explicit
/// worklist whose own exit condition is "it emptied" — a property of the
/// two-phase push being exactly right, not an independent bound. Weaken that
/// push and the loop re-expands the same node forever.
///
/// [`MAX_MEASURE_STEPS`] makes it finite whatever the body does. This test
/// asserts the *limit*, and — like everything else in this file — never runs
/// the walk it protects: the measurement itself is exercised by every row of
/// `koat_answers.rs`, which is what turns an exhausted budget into a named
/// refusal there rather than a hang here.
#[test]
fn the_growth_measurement_has_a_step_budget_that_no_real_answer_reaches() {
    // Four steps per token, not one. Each node of the term is popped once
    // unexpanded and once expanded, and is pushed once more for every parent
    // that names it, so a budget merely equal to the node count would refuse
    // ordinary answers.
    assert!(
        MAX_MEASURE_STEPS >= MAX_ANSWER_TOKENS * 4,
        "a bound parsed from {MAX_ANSWER_TOKENS} tokens is walked in several steps per \
         node; a budget of {MAX_MEASURE_STEPS} steps would refuse ordinary answers"
    );
    assert!(
        MAX_MEASURE_STEPS <= MAX_ANSWER_TOKENS * 64,
        "a budget of {MAX_MEASURE_STEPS} steps is not a budget"
    );
}

/// The poll budget must not exceed the number of milliseconds in the timeout:
/// more polls than milliseconds means the loop's counter, not the clock, is
/// what ends the wait — and the counter would end it early.
#[test]
fn the_poll_budget_never_exceeds_the_milliseconds_it_is_derived_from() {
    for secs in [MIN_TIMEOUT_SECS, 2, 30, 600, MAX_TIMEOUT_SECS] {
        let Ok(timeout) = Timeout::new(secs) else {
            panic!("{secs} is inside the permitted range and must be accepted");
        };
        let polls = u64::from(poll_budget(timeout));
        let millis = secs.saturating_mul(1000);
        assert!(
            polls <= millis,
            "a {secs}s timeout buys {polls} polls, more than the {millis} milliseconds \
             it lasts; the counter would end the wait before the clock does"
        );
    }
}

/// The poll interval is what the budget is derived from, and it has to be a
/// real interval: zero would divide by zero, and a second would make the
/// shortest permitted timeout a single poll.
#[test]
fn the_poll_interval_is_short_enough_to_divide_the_shortest_timeout() {
    assert!(
        (1..=100).contains(&POLL_INTERVAL_MILLIS),
        "POLL_INTERVAL_MILLIS is {POLL_INTERVAL_MILLIS}"
    );
    assert!(
        MIN_TIMEOUT_SECS * 1000 / POLL_INTERVAL_MILLIS >= 10,
        "the shortest permitted timeout buys fewer than ten polls"
    );
}

/// KoAT's own clock is set below this crate's, and the grace must be smaller
/// than the default budget or the default would drive it to its floor.
#[test]
fn the_solver_grace_leaves_the_default_budget_intact() {
    assert!(
        KOAT_TIMEOUT_GRACE_SECS >= 1,
        "a grace of zero makes the two clocks fire together, so a slow analysis is as \
         likely to be killed as to decline"
    );
    assert!(
        KOAT_TIMEOUT_GRACE_SECS < Timeout::DEFAULT.seconds(),
        "a grace of {KOAT_TIMEOUT_GRACE_SECS}s against a default of {}s leaves KoAT no \
         time at all",
        Timeout::DEFAULT.seconds()
    );
}

/// A timeout says how long it is. The number reaches a user in the sentence
/// that tells them a solver was stopped, and "the solver did not finish
/// within s" is not that sentence.
#[test]
fn a_timeout_says_how_long_it_is() {
    assert_eq!(Timeout::DEFAULT.to_string(), "30s");
    for secs in [MIN_TIMEOUT_SECS, 25, MAX_TIMEOUT_SECS] {
        let Ok(timeout) = Timeout::new(secs) else {
            panic!("{secs} is inside the permitted range and must be accepted");
        };
        assert_eq!(timeout.to_string(), format!("{secs}s"));
    }
}
