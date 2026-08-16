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
    let mut build =
        SourceProgramBuilder::new("f", here(), vec![VarName::new("n"), VarName::new("m")]);
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
/// way to write because subtraction is not monotone. Not approximated with
/// `stop`, which is **unsound** when the start may be negative - the loop
/// becomes a hole instead, so the rest of the function still gets a bound.
#[test]
fn a_symbolic_start_becomes_a_hole_rather_than_an_approximation() {
    let mut build =
        SourceProgramBuilder::new("f", here(), vec![VarName::new("a"), VarName::new("b")]);
    let start = build.var(VarName::new("a"), here());
    let stop = build.var(VarName::new("b"), here());
    let range = RangeSpec::new(start, stop, NonZeroI64::new(1).expect("1 is non-zero"));
    let loop_stmt = build.for_range(VarName::new("i"), range, vec![], here());
    let program = build.build(vec![loop_stmt]);

    let result = cost(&program);
    assert_eq!(result.holes().len(), 1, "the loop is one unanalysed region");
    assert_eq!(result.holes()[0].construct(), "for");
}

/// A stride above one needs `ceil(n / k)`, and division is not expressible
/// either. Not silently widened to `n`.
#[test]
fn a_symbolic_range_with_a_wide_stride_becomes_a_hole() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let range = RangeSpec::new(start, stop, NonZeroI64::new(3).expect("3 is non-zero"));
    let loop_stmt = build.for_range(VarName::new("i"), range, vec![], here());
    let program = build.build(vec![loop_stmt]);

    let result = cost(&program);
    assert_eq!(result.holes().len(), 1);
    assert!(
        !result.is_complete(),
        "a hole means the bound is not standalone"
    );
}

/// `while` needs a ranking argument this engine does not have, so it becomes a
/// hole - named and carried - rather than a fabricated number or a dead end.
#[test]
fn a_while_loop_becomes_a_named_hole() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let left = build.var(VarName::new("i"), here());
    let right = build.var(VarName::new("n"), here());
    let cond = build.compare(landav_its::CompareOp::Lt, left, right, here());
    let loop_stmt = build.while_loop(cond, vec![], here());
    let program = build.build(vec![loop_stmt]);

    let result = cost(&program);
    assert_eq!(result.holes().len(), 1, "one `while` is one region");
    assert_eq!(
        result.holes()[0].construct(),
        "while",
        "the hole must name the construct that caused it, or the user has \
         nothing to act on"
    );
    assert!(
        result.bound().is_some(),
        "a hole still yields a bound to report"
    );
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

/// Subtraction in an endpoint is not monotone, so there is no bound for the
/// count. The loop becomes a hole rather than being given a count that would
/// be wrong in one direction.
#[test]
fn a_subtracting_endpoint_becomes_a_hole() {
    let mut build =
        SourceProgramBuilder::new("f", here(), vec![VarName::new("n"), VarName::new("m")]);
    let start = build.int(0, here());
    let n = build.var(VarName::new("n"), here());
    let m = build.var(VarName::new("m"), here());
    let stop = build.arith(ArithOp::Sub, n, m, here());
    let range = RangeSpec::new(start, stop, NonZeroI64::new(1).expect("1 is non-zero"));
    let loop_stmt = build.for_range(VarName::new("i"), range, vec![], here());
    let program = build.build(vec![loop_stmt]);

    assert_eq!(cost(&program).holes().len(), 1);
}

/// An unsupported construct is a hole, never skipped. A skipped statement would
/// make the total smaller than the truth, which is the one direction a resource
/// bound must never move.
#[test]
fn an_unsupported_statement_becomes_a_hole_naming_the_construct() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![]);
    let value = build.int(0, here());
    let assign = build.assign(VarName::new("x"), value, here());
    let refused = build.unsupported_stmt(landav_its::Construct::Call, here());
    let program = build.build(vec![assign, refused]);

    let result = cost(&program);
    assert_eq!(result.holes().len(), 1);
    assert_eq!(result.holes()[0].construct(), "call");
    // The assignment before it is still counted. Before holes, the refusal
    // swallowed it.
    assert!(result.bound().is_some());
}

/// `for i in range(n): for j in range(i): x = 0`
fn triangular() -> SourceProgram {
    let mut build = SourceProgramBuilder::new("tri", here(), vec![VarName::new("n")]);
    let one = NonZeroI64::new(1).expect("1 is non-zero");

    let inner_start = build.int(0, here());
    // The inner limit is the *outer* loop's counter. This is what makes the
    // nesting triangular, and what made the engine leak a bound variable.
    let inner_stop = build.var(VarName::new("i"), here());
    let value = build.int(0, here());
    let assign = build.assign(VarName::new("x"), value, here());
    let inner = build.for_range(
        VarName::new("j"),
        RangeSpec::new(inner_start, inner_stop, one),
        vec![assign],
        here(),
    );

    let outer_start = build.int(0, here());
    let outer_stop = build.var(VarName::new("n"), here());
    let outer = build.for_range(
        VarName::new("i"),
        RangeSpec::new(outer_start, outer_stop, one),
        vec![inner],
        here(),
    );
    build.build(vec![outer])
}

/// The regression. A loop counter is bound by its loop, so it must not appear
/// in the loop's own cost - the caller can supply `n` and has nothing to supply
/// for `i`.
///
/// This shipped broken: the engine returned `Exact(n * (1 + 2i))`, which is
/// both unusable and mislabelled, because every step that produced it was
/// individually exact.
#[test]
fn a_loop_counter_never_escapes_into_the_bound() {
    let program = triangular();
    let result = cost(&program);
    let bound = result.bound().expect("triangular nesting has a bound");
    let named: Vec<String> = bound.vars().iter().map(ToString::to_string).collect();
    assert_eq!(
        named,
        vec!["n".to_owned()],
        "the bound must name only the function's parameters; `i` and `j` are \
         bound by their loops. Got {bound}"
    );
}

/// Triangular nesting is **exact**, and the exactness comes from summing the
/// body's cost over the counter rather than multiplying by a dominating value.
///
/// `sum over i < n of (1 + 2i)` = `n^2`. The approximation this replaced gave
/// `n * (1 + 2n)` = `2n^2 + n` - right shape, about twice too large.
#[test]
fn triangular_nesting_is_exact() {
    assert!(
        matches!(cost(&triangular()), TripCount::Exact(_)),
        "the definite sum closes for triangular nesting, so the answer is exact"
    );
}

/// Checked against the truth rather than against the implementation: the
/// engine's bound must equal the brute-force cost at every point, not merely
/// dominate it.
///
/// Worth noting that `n^2` needs neither subtraction nor division to write
/// down. The intermediate Faulhaber sum has both - `n(n-1)/2` - but the
/// coefficient `2` clears the denominator and the negative term cancels
/// against the constant one. The rationals never leave the summation.
#[test]
fn the_triangular_bound_equals_the_truth() {
    let bound = cost(&triangular())
        .bound()
        .expect("triangular nesting has a bound")
        .clone();
    for n in [0_u64, 1, 2, 3, 4, 8, 32] {
        let truth: u64 = (0..n).map(|i| 1 + 2 * i).sum();
        assert_eq!(
            truth,
            n * n,
            "the arithmetic in this test is wrong, not the engine"
        );
        assert_eq!(
            at(&bound, "n", n),
            landav_bound::Nat::Fin(truth),
            "the engine disagreed with the true cost at n = {n}"
        );
    }
}

/// A descending or strided loop has the same *trip count* as an ascending unit
/// one but a different *sequence* of counter values, and Faulhaber's formulae
/// are over `0, 1, 2, ...`. Summing one as if it were the other would be
/// silently wrong, so those fall back to the approximation.
#[test]
fn a_strided_loop_body_that_reads_its_counter_is_not_summed() {
    let mut build = SourceProgramBuilder::new("strided", here(), vec![VarName::new("n")]);
    let three = NonZeroI64::new(3).expect("3 is non-zero");
    let one = NonZeroI64::new(1).expect("1 is non-zero");

    let inner_start = build.int(0, here());
    let inner_stop = build.var(VarName::new("i"), here());
    let value = build.int(0, here());
    let assign = build.assign(VarName::new("x"), value, here());
    let inner = build.for_range(
        VarName::new("j"),
        RangeSpec::new(inner_start, inner_stop, one),
        vec![assign],
        here(),
    );

    // The outer loop strides by three, so its counter is 0, 3, 6, ... and the
    // trip count alone does not determine the sum.
    let outer_start = build.int(0, here());
    let outer_stop = build.int(30, here());
    let outer = build.for_range(
        VarName::new("i"),
        RangeSpec::new(outer_start, outer_stop, three),
        vec![inner],
        here(),
    );
    let program = build.build(vec![outer]);

    let result = cost(&program);
    assert!(
        matches!(result, TripCount::AtMost(_)),
        "a strided counter must not be summed as if it stepped by one, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// LAN-81: a hole is not a lost bound
// ---------------------------------------------------------------------------

/// `for i in range(n): x = 0` followed by `while n > 0: n = n - 1`.
///
/// The shape this story exists for, and the shape most real Python has.
fn mixed() -> SourceProgram {
    let mut build = SourceProgramBuilder::new("mixed", here(), vec![VarName::new("n")]);
    let one = NonZeroI64::new(1).expect("1 is non-zero");

    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let value = build.int(0, here());
    let assign = build.assign(VarName::new("x"), value, here());
    let counted = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one),
        vec![assign],
        here(),
    );

    let read = build.var(VarName::new("n"), here());
    let zero = build.int(0, here());
    let cond = build.compare(landav_its::CompareOp::Gt, read, zero, here());
    let decrement_read = build.var(VarName::new("n"), here());
    let step = build.int(1, here());
    let decremented = build.arith(ArithOp::Sub, decrement_read, step, here());
    let decrement = build.assign(VarName::new("n"), decremented, here());
    let unbounded = build.while_loop(cond, vec![decrement], here());

    build.build(vec![counted, unbounded])
}

/// **The regression this story is about.** Before holes, the `while` made the
/// whole function `Unknown` - the counted loop above it was derived correctly
/// and then discarded.
#[test]
fn a_while_no_longer_erases_the_bound_around_it() {
    let result = cost(&mixed());
    let bound = result
        .bound()
        .expect("the counted half is still derivable and must be reported");
    assert_eq!(result.holes().len(), 1, "only the `while` is unanalysed");
    assert_eq!(result.holes()[0].construct(), "while");

    // With the hole at zero the remainder is exactly the counted loop's cost:
    // 2n for the loop, plus nothing else in the function.
    let hole = result.holes()[0].var();
    let without = bound.subst(&hole, &landav_bound::Bound::zero());
    for n in [0_u64, 1, 4, 9] {
        assert_eq!(
            at(&without, "n", n),
            landav_bound::Nat::Fin(2 * n),
            "the counted half must survive the `while` beside it, at n = {n}"
        );
    }
}

/// A partial result is not a complete one, and must not be mistaken for a
/// bound that stands on its own - it cannot be compared against a budget or
/// against another engine until its holes are filled.
#[test]
fn a_partial_result_does_not_claim_to_be_complete() {
    let result = cost(&mixed());
    assert!(!result.is_complete());
    assert!(
        !result.is_exact(),
        "a bound containing a hole is never a plain equality"
    );
    // But what *was* derived was derived exactly, and saying so is the point.
    assert!(
        result.exact_outside_holes(),
        "the counted loop was exact, and that is the actionable half"
    );
}

/// Filling a hole yields what the engine would have produced had it known the
/// region all along. This is the property that makes holes worth carrying
/// rather than merely reporting.
#[test]
fn filling_a_hole_recovers_the_whole_bound() {
    let result = cost(&mixed());
    let bound = result.bound().expect("a bound").clone();
    let hole = result.holes()[0].var();

    // Suppose the `while` is later bounded at 3n by some other means.
    let filled = bound.subst(
        &hole,
        &landav_bound::Bound::prod([
            landav_bound::Bound::constant(3),
            landav_bound::Bound::var(Symbol::from("n")),
        ]),
    );
    for n in [0_u64, 1, 5, 12] {
        assert_eq!(
            at(&filled, "n", n),
            landav_bound::Nat::Fin(2 * n + 3 * n),
            "the filled bound must be the sum of both halves at n = {n}"
        );
    }
}

/// A hole is not a parameter. A consumer walking the bound's variables must be
/// able to tell them apart, or it will ask the user for a value for a region.
#[test]
fn a_hole_is_distinguishable_from_a_parameter() {
    let result = cost(&mixed());
    let bound = result.bound().expect("a bound");
    let named: Vec<String> = bound
        .vars()
        .iter()
        .filter(|var| !landav_engine::Hole::is_hole(var))
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        named,
        vec!["n".to_owned()],
        "only the parameter should survive the filter; got {named:?}"
    );
    assert!(
        bound.vars().len() > named.len(),
        "the hole must actually be present in the bound"
    );
}

/// Two unanalysable regions are two holes. Sharing one variable would mean
/// filling either fills both, which would be wrong in general and silently so.
#[test]
fn two_regions_get_two_holes() {
    let mut build = SourceProgramBuilder::new("twice", here(), vec![VarName::new("n")]);
    let make_while = |build: &mut SourceProgramBuilder| {
        let read = build.var(VarName::new("n"), here());
        let zero = build.int(0, here());
        let cond = build.compare(landav_its::CompareOp::Gt, read, zero, here());
        build.while_loop(cond, vec![], here())
    };
    let first = make_while(&mut build);
    let second = make_while(&mut build);
    let program = build.build(vec![first, second]);

    let result = cost(&program);
    assert_eq!(result.holes().len(), 2);
    assert_ne!(
        result.holes()[0].var(),
        result.holes()[1].var(),
        "two regions must not share a variable"
    );
}
