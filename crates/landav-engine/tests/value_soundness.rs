//! `LAN-87a` acceptance: a bound is a function of the values the caller
//! actually supplies.
//!
//! # The defect, stated once
//!
//! `expr_bound::read` maps `SourceExpr::Var { name }` to `Bound::var(name)`
//! with no check that `name` is a parameter and no reaching-definition
//! discipline. The bound it returns therefore denotes *the variable's value on
//! entry*, and its only caller, `count_of`, uses it as *the variable's value at
//! the loop*. Those are the same thing exactly when nothing assigns to the
//! variable in between.
//!
//! Two consequences, both live at 100% coverage and with no call involved:
//!
//! * **An exceedable bound.** `n = n * n` followed by `for i in range(n)` is
//!   reported as `Theta(1 + 2n)` when the loop runs `n^2` times. It is marked
//!   exact and carries no holes, so every consumer — a CI budget gate, an agent
//!   choosing what to optimise — is entitled to act on it.
//! * **A bound in terms of something the caller has never heard of.**
//!   `m = n * n` followed by `for i in range(m)` reports a bound mentioning
//!   `m`, which is a local. `Bound::eval` needs a value for it and the caller
//!   has none to give; worse, `m = 5` reports a symbolic `m` for a loop whose
//!   trip count is the literal 5.
//!
//! # Why this file comes before `hole_totality.rs`
//!
//! `LAN-87` makes the engine total, which means programs that never reached it
//! before will now reach it — including every one of the 441 stdlib functions
//! whose only obstacle was a call. Shipping the reach before the fix multiplies
//! the audience for a wrong number. This is the build-order gate, encoded.
//!
//! # What "sound" is asserted to mean here
//!
//! Only **complete** results are checked numerically. A `Partial` result's
//! unfilled hole denotes `omega`, so it makes no finite claim and cannot be
//! exceeded; that is the whole reason a hole is an acceptable answer. The
//! corresponding risk — that the fix degrades into "return a hole for
//! everything" — is what the last test in this file exists to catch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, num::NonZeroI64};

use landav_bound::{Nat, Origin, Symbol, Valuation, VarId};
use landav_engine::{Hole, TripCount, cost};
use landav_its::{ArithOp, RangeSpec, SourceProgram, SourceProgramBuilder, VarName};

fn here() -> Origin {
    Origin::new("soundness.py:3")
}

fn one() -> NonZeroI64 {
    NonZeroI64::new(1).expect("1 is non-zero")
}

/// A valuation over an explicit table, zero elsewhere.
struct Bindings(BTreeMap<Symbol, u64>);

impl Valuation for Bindings {
    fn value_of(&self, var: &VarId) -> Nat {
        Nat::Fin(self.0.get(var.symbol()).copied().unwrap_or(0))
    }
}

fn describe(result: &TripCount) -> String {
    let kind = match result {
        TripCount::Exact(_) => "Theta",
        TripCount::AtMost(_) => "O",
        TripCount::Partial { .. } => "Partial",
        TripCount::Unknown => "Unknown",
    };
    let bound = result
        .bound()
        .map_or_else(|| "-".to_owned(), ToString::to_string);
    format!("{kind}({bound}) holes={:?}", result.holes().len())
}

/// The two obligations a **complete** bound carries, checked together.
///
/// Together because they fail together in practice and separately in principle:
/// a bound mentioning a local is not merely unhelpful, it is the mechanism by
/// which the number comes out small — the local reads as zero, or as whatever
/// the caller happens to pass for a parameter of the same name.
fn assert_complete_bound_is_usable_and_sound(
    program: &SourceProgram,
    label: &str,
    n: u64,
    truth: u64,
) {
    let result = cost(program);
    let shown = describe(&result);
    if !result.is_complete() {
        // A hole denotes omega: no finite claim, nothing to exceed.
        return;
    }
    let bound = result.bound().expect("a complete result carries a bound");

    for var in bound.vars() {
        let name = var.symbol().as_str().to_owned();
        assert!(
            program
                .params()
                .iter()
                .any(|param| param.symbol().as_str() == name)
                || Hole::is_hole(&var),
            "{label}: the bound mentions `{name}`, which is neither a parameter \
             of `{}` nor a hole, so the caller has nothing to supply for it and \
             the number is not a function of the inputs: {shown}",
            program.name(),
        );
    }

    let mut table = BTreeMap::new();
    table.insert(Symbol::from("n"), n);
    let reported = bound.eval(&Bindings(table));
    assert!(
        reported.magnitude_cmp(Nat::Fin(truth)) != core::cmp::Ordering::Less,
        "{label}: at n = {n} the program executes {truth} steps and the \
         reported bound evaluates to {reported:?}. A complete bound is offered \
         for comparison against a budget, so one the program exceeds is worse \
         than no bound at all.\n  got: {shown}"
    );
}

// ---------------------------------------------------------------------------
// the reported bound must not be exceedable
// ---------------------------------------------------------------------------

/// `def f(n): n = n * n; for i in range(n): x = 0`
///
/// The loop runs `n^2` times because the range is evaluated *after* the
/// assignment. Cost: one step for the assignment, then two per iteration (the
/// body and the loop's own step) — `1 + 2n^2`.
#[test]
fn a_loop_over_a_reassigned_parameter_is_not_reported_at_the_entry_value() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let left = build.var(VarName::new("n"), here());
    let right = build.var(VarName::new("n"), here());
    let square = build.arith(ArithOp::Mul, left, right, here());
    let square_it = build.assign(VarName::new("n"), square, here());

    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let zero = build.int(0, here());
    let body = build.assign(VarName::new("x"), zero, here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        vec![body],
        here(),
    );
    let program = build.build(vec![square_it, loop_stmt]);

    for n in [3_u64, 5, 10] {
        assert_complete_bound_is_usable_and_sound(
            &program,
            "n = n * n; for i in range(n)",
            n,
            1 + 2 * n * n,
        );
    }
}

/// `def f(n): m = n * n; for i in range(m): x = 0`
///
/// Same trip count, reached through a local. The bound today is written in
/// terms of `m`, which no caller can supply — and which evaluates to zero for
/// anyone who supplies only the parameters, turning a quadratic cost into a
/// constant.
#[test]
fn a_loop_over_a_local_is_not_reported_in_terms_of_that_local() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let left = build.var(VarName::new("n"), here());
    let right = build.var(VarName::new("n"), here());
    let square = build.arith(ArithOp::Mul, left, right, here());
    let define_m = build.assign(VarName::new("m"), square, here());

    let start = build.int(0, here());
    let stop = build.var(VarName::new("m"), here());
    let zero = build.int(0, here());
    let body = build.assign(VarName::new("x"), zero, here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        vec![body],
        here(),
    );
    let program = build.build(vec![define_m, loop_stmt]);

    for n in [3_u64, 7] {
        assert_complete_bound_is_usable_and_sound(
            &program,
            "m = n * n; for i in range(m)",
            n,
            1 + 2 * n * n,
        );
    }
}

/// `def f(n): m = 5; for i in range(m): x = 0`
///
/// The starkest form: a loop whose trip count is a literal, reported as a
/// symbolic bound over a variable that does not exist outside the function.
/// True cost is `1 + 2 * 5 = 11` for every `n`.
#[test]
fn a_loop_over_a_local_constant_is_not_reported_as_a_symbolic_bound() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let five = build.int(5, here());
    let define_m = build.assign(VarName::new("m"), five, here());

    let start = build.int(0, here());
    let stop = build.var(VarName::new("m"), here());
    let zero = build.int(0, here());
    let body = build.assign(VarName::new("x"), zero, here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        vec![body],
        here(),
    );
    let program = build.build(vec![define_m, loop_stmt]);

    assert_complete_bound_is_usable_and_sound(&program, "m = 5; for i in range(m)", 0, 11);
}

// ---------------------------------------------------------------------------
// the control: the fix must be a fix, not a retreat
// ---------------------------------------------------------------------------

/// **A parameter that nothing assigns still yields an exact bound.**
///
/// The positive control, and it is not decoration: every test above passes
/// trivially if the fix is "stop reading variables", because a `Partial` result
/// makes no finite claim and is skipped by the soundness check. This is the
/// assertion that says the repair has to be *reaching definitions*, not
/// surrender.
///
/// Two shapes, because they exercise different halves of the rule:
///
/// * `for i in range(n)` with `n` a plain parameter — the overwhelmingly common
///   case in real Python, and the one the engine exists for;
/// * `x = n * n; for i in range(n)` — an assignment is present in the arena but
///   targets something else, so the parameter's entry value still holds at the
///   loop.
#[test]
fn a_loop_over_an_unassigned_parameter_stays_exact() {
    // `def f(n): for i in range(n): x = 0` -> exactly 2n.
    let mut build = SourceProgramBuilder::new("plain", here(), vec![VarName::new("n")]);
    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let zero = build.int(0, here());
    let body = build.assign(VarName::new("x"), zero, here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        vec![body],
        here(),
    );
    let plain = build.build(vec![loop_stmt]);

    let result = cost(&plain);
    assert!(
        result.is_exact(),
        "`for i in range(n)` over a parameter nothing assigns is the case this \
         engine exists for, and its trip count is still exactly `n`. Losing it \
         to the value-soundness fix trades one wrong answer for no answers: {}",
        describe(&result)
    );
    let mut table = BTreeMap::new();
    table.insert(Symbol::from("n"), 6_u64);
    assert_eq!(
        result.bound().expect("exact").eval(&Bindings(table)),
        Nat::Fin(12),
        "the body and the loop's own step, six times over"
    );

    // `def f(n): x = n * n; for i in range(n): y = 0` -> exactly 1 + 2n.
    let mut build = SourceProgramBuilder::new("elsewhere", here(), vec![VarName::new("n")]);
    let left = build.var(VarName::new("n"), here());
    let right = build.var(VarName::new("n"), here());
    let square = build.arith(ArithOp::Mul, left, right, here());
    let define_x = build.assign(VarName::new("x"), square, here());
    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let zero = build.int(0, here());
    let body = build.assign(VarName::new("y"), zero, here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        vec![body],
        here(),
    );
    let elsewhere = build.build(vec![define_x, loop_stmt]);

    let result = cost(&elsewhere);
    assert!(
        result.is_exact(),
        "the assignment targets `x`, so `n` still holds its entry value at the \
         loop and the count is still exact. A rule that gives up on any \
         program containing an assignment gives up on almost every program: {}",
        describe(&result)
    );
    let mut table = BTreeMap::new();
    table.insert(Symbol::from("n"), 6_u64);
    assert_eq!(
        result.bound().expect("exact").eval(&Bindings(table)),
        Nat::Fin(13),
        "one assignment, then two steps per iteration"
    );
}
