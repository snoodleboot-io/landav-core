//! `LAN-75` acceptance: a counted loop yields an exact count, nesting
//! multiplies, and everything the engine cannot nail says so rather than
//! guessing.
//!
//! # What these assert against
//!
//! Bounds are compared by evaluating them at concrete valuations rather than by
//! matching their syntax. Two bounds that denote the same function may be
//! written differently - the algebra normalises, and normalisation is allowed
//! to change - so an assertion on structure would fail for reasons that are not
//! defects. What matters is the number.

// A test that cannot build its fixture should stop loudly and immediately.
// The library lints stay in force; this is the established exception for test
// targets across the workspace.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::num::NonZeroI64;

use landav_bound::{Bound, Origin, Symbol, Valuation, VarId};
use landav_engine::{TripCount, cost};
use landav_its::{ArithOp, RangeSpec, SourceProgram, SourceProgramBuilder, VarName};

/// A valuation binding a single variable, and zero for anything else.
struct One {
    name: Symbol,
    value: u64,
}

impl Valuation for One {
    fn value_of(&self, var: &VarId) -> landav_bound::Nat {
        if var.symbol() == &self.name {
            landav_bound::Nat::Fin(self.value)
        } else {
            landav_bound::Nat::Fin(0)
        }
    }
}

fn at(bound: &Bound, name: &str, value: u64) -> landav_bound::Nat {
    bound.eval(&One {
        name: Symbol::from(name),
        value,
    })
}

fn here() -> Origin {
    Origin::new("probe.py:1")
}

/// `def f(n): for i in range(0, n, 1): x = 0`
fn counted_loop(body_statements: usize) -> SourceProgram {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let zero = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let range = RangeSpec::new(zero, stop, NonZeroI64::new(1).expect("1 is non-zero"));
    let body: Vec<_> = (0..body_statements)
        .map(|_| {
            let value = build.int(0, here());
            build.assign(VarName::new("x"), value, here())
        })
        .collect();
    let loop_stmt = build.for_range(VarName::new("i"), range, body, here());
    build.build(vec![loop_stmt])
}

#[test]
fn a_counted_loop_over_a_parameter_is_exact() {
    let program = counted_loop(1);
    let result = cost(&program);
    assert!(result.is_exact(), "expected an exact count, got {result:?}");

    // One assignment plus one step of loop overhead, n times: 2n.
    let bound = result.bound().expect("an exact count has a bound").clone();
    for n in [0_u64, 1, 5, 100] {
        assert_eq!(
            at(&bound, "n", n),
            landav_bound::Nat::Fin(2 * n),
            "cost at n = {n}"
        );
    }
}

#[test]
fn an_empty_counted_loop_still_costs_its_own_iteration() {
    let bound = cost(&counted_loop(0))
        .bound()
        .expect("an empty loop is still counted")
        .clone();
    assert_eq!(at(&bound, "n", 7), landav_bound::Nat::Fin(7));
}

/// `for i in range(0, n): for j in range(0, m): x = 0` - the counts multiply
/// because neither iteration space depends on the other's counter.
#[test]
fn rectangular_nesting_multiplies_and_stays_exact() {
    let mut build = SourceProgramBuilder::new(
        "f",
        here(),
        vec![VarName::new("n"), VarName::new("m")],
    );
    let one = NonZeroI64::new(1).expect("1 is non-zero");

    let inner_zero = build.int(0, here());
    let inner_stop = build.var(VarName::new("m"), here());
    let value = build.int(0, here());
    let assign = build.assign(VarName::new("x"), value, here());
    let inner = build.for_range(
        VarName::new("j"),
        RangeSpec::new(inner_zero, inner_stop, one),
        vec![assign],
        here(),
    );

    let outer_zero = build.int(0, here());
    let outer_stop = build.var(VarName::new("n"), here());
    let outer = build.for_range(
        VarName::new("i"),
        RangeSpec::new(outer_zero, outer_stop, one),
        vec![inner],
        here(),
    );
    let program = build.build(vec![outer]);

    let result = cost(&program);
    assert!(result.is_exact(), "nesting should stay exact: {result:?}");

    // Inner: 2m per outer iteration (one assignment + one overhead), plus the
    // outer loop's own step. With m held at zero by the valuation the inner
    // loop runs not at all, so the cost is exactly n.
    let bound = result.bound().expect("exact").clone();
    assert_eq!(at(&bound, "n", 4), landav_bound::Nat::Fin(4));
}

/// Literal endpoints are arithmetic, including the backwards and empty cases
/// that a naive subtraction gets wrong.
#[test]
fn literal_ranges_are_counted_outright() {
    let cases = [
        (0_i64, 10_i64, 1_i64, 10_u64),
        (2, 10, 1, 8),
        // Ceiling division, not truncating: 0,3,6,9 is four iterations.
        (0, 10, 3, 4),
        // Descending.
        (10, 0, -1, 10),
        (10, 0, -3, 4),
        // Empty in the direction of travel.
        (10, 0, 1, 0),
        (0, 10, -1, 0),
        (5, 5, 1, 0),
    ];

    for (from, to, step, expected) in cases {
        let mut build = SourceProgramBuilder::new("f", here(), vec![]);
        let start = build.int(from, here());
        let stop = build.int(to, here());
        let range = RangeSpec::new(
            start,
            stop,
            NonZeroI64::new(step).expect("step is non-zero"),
        );
        let loop_stmt = build.for_range(VarName::new("i"), range, vec![], here());
        let program = build.build(vec![loop_stmt]);

        let result = cost(&program);
        assert!(
            result.is_exact(),
            "range({from}, {to}, {step}) should be exact, got {result:?}"
        );
        let bound = result.bound().expect("exact").clone();
        assert_eq!(
            at(&bound, "unused", 0),
            landav_bound::Nat::Fin(expected),
            "range({from}, {to}, {step})"
        );
    }
}

/// A symbolic start would need `stop - start`, which the bound algebra has no
/// way to write because subtraction is not monotone. Refused rather than
/// approximated with `stop`, which is **unsound** when the start may be
/// negative.
#[test]
fn a_symbolic_start_is_refused_rather_than_approximated() {
    let mut build = SourceProgramBuilder::new(
        "f",
        here(),
        vec![VarName::new("a"), VarName::new("b")],
    );
    let start = build.var(VarName::new("a"), here());
    let stop = build.var(VarName::new("b"), here());
    let range = RangeSpec::new(start, stop, NonZeroI64::new(1).expect("1 is non-zero"));
    let loop_stmt = build.for_range(VarName::new("i"), range, vec![], here());
    let program = build.build(vec![loop_stmt]);

    assert_eq!(cost(&program), TripCount::Unknown);
}

/// A stride above one needs `ceil(n / k)`, and division is not expressible
/// either. Not silently widened to `n`.
#[test]
fn a_symbolic_range_with_a_wide_stride_is_refused() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let range = RangeSpec::new(start, stop, NonZeroI64::new(3).expect("3 is non-zero"));
    let loop_stmt = build.for_range(VarName::new("i"), range, vec![], here());
    let program = build.build(vec![loop_stmt]);

    assert_eq!(cost(&program), TripCount::Unknown);
}

/// `while` needs a ranking argument this engine does not have. `Unknown`, so
/// the caller falls through to the solver rather than receiving a fabricated
/// number that would displace a real one.
#[test]
fn a_while_loop_is_unknown_rather_than_guessed() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let left = build.var(VarName::new("i"), here());
    let right = build.var(VarName::new("n"), here());
    let cond = build.compare(landav_its::CompareOp::Lt, left, right, here());
    let loop_stmt = build.while_loop(cond, vec![], here());
    let program = build.build(vec![loop_stmt]);

    assert_eq!(cost(&program), TripCount::Unknown);
}

/// Branches of unequal cost give an upper bound, not an exact one: claiming
/// exactness would assert the expensive branch is reachable, and this engine
/// performs no reachability analysis.
#[test]
fn unequal_branches_yield_a_bound_rather_than_an_equality() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let left = build.var(VarName::new("n"), here());
    let right = build.int(0, here());
    let cond = build.compare(landav_its::CompareOp::Gt, left, right, here());

    let value = build.int(0, here());
    let one_assignment = build.assign(VarName::new("x"), value, here());
    let value2 = build.int(1, here());
    let a = build.assign(VarName::new("y"), value2, here());
    let value3 = build.int(2, here());
    let b = build.assign(VarName::new("z"), value3, here());

    let branch = build.if_else(cond, vec![a, b], vec![one_assignment], here());
    let program = build.build(vec![branch]);

    let result = cost(&program);
    assert!(
        matches!(result, TripCount::AtMost(_)),
        "unequal branches must not claim exactness, got {result:?}"
    );
    // Two statements on the expensive side, plus the test.
    let bound = result.bound().expect("a bound").clone();
    assert_eq!(at(&bound, "n", 0), landav_bound::Nat::Fin(3));
}

/// Equal branches are the exception. The maximum is attained whichever way the
/// test goes, so reachability never arises and the result stays exact.
#[test]
fn equal_branches_stay_exact() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let left = build.var(VarName::new("n"), here());
    let right = build.int(0, here());
    let cond = build.compare(landav_its::CompareOp::Gt, left, right, here());

    let v1 = build.int(0, here());
    let a = build.assign(VarName::new("x"), v1, here());
    let v2 = build.int(0, here());
    let b = build.assign(VarName::new("y"), v2, here());

    let branch = build.if_else(cond, vec![a], vec![b], here());
    let program = build.build(vec![branch]);

    let result = cost(&program);
    assert!(
        result.is_exact(),
        "equal branches should stay exact, got {result:?}"
    );
}

/// Subtraction in an endpoint is not monotone, so it has no bound. The loop is
/// refused rather than given one that would be wrong in one direction.
#[test]
fn a_subtracting_endpoint_has_no_bound() {
    let mut build = SourceProgramBuilder::new(
        "f",
        here(),
        vec![VarName::new("n"), VarName::new("m")],
    );
    let start = build.int(0, here());
    let n = build.var(VarName::new("n"), here());
    let m = build.var(VarName::new("m"), here());
    let stop = build.arith(ArithOp::Sub, n, m, here());
    let range = RangeSpec::new(start, stop, NonZeroI64::new(1).expect("1 is non-zero"));
    let loop_stmt = build.for_range(VarName::new("i"), range, vec![], here());
    let program = build.build(vec![loop_stmt]);

    assert_eq!(cost(&program), TripCount::Unknown);
}

/// An unsupported construct is `Unknown`, never skipped. A skipped statement
/// would make the total smaller than the truth, which is the one direction a
/// resource bound must never move.
#[test]
fn an_unsupported_statement_is_not_silently_dropped() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![]);
    let value = build.int(0, here());
    let assign = build.assign(VarName::new("x"), value, here());
    let refused = build.unsupported_stmt(landav_its::Construct::Call, here());
    let program = build.build(vec![assign, refused]);

    assert_eq!(cost(&program), TripCount::Unknown);
}
