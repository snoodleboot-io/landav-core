//! `LAN-102`: **`range(k, n)` with a literal start is counted.**
//!
//! `Walk::count_of` counted two shapes: both endpoints literal, and
//! `range(0, n)` with a unit step. `for i in range(1, n)` - the loop that
//! skips a header - was a hole whatever its body, and so was every generated
//! loop in the property suite whose start was a literal other than zero, which
//! is most of them.
//!
//! The count is `max(0, n - k)`. For `k >= 0` that is dominated by `n` and is
//! never an equality here, because `Bound` has no maximum and `n - k` is not a
//! value over the naturals when `n < k` - `LAN-88`'s rule, reached from the
//! other side. For `k < 0` it is `n + |k|`, a polynomial, and exact. The
//! descending mirror `range(n, k, -1)` is the same arithmetic.
//!
//! The reference interpreter in `landav-its/tests/properties` runs these
//! shapes already; what this file pins is the *kind* of claim each one makes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, num::NonZeroI64};

use landav_bound::{Nat, Origin, Symbol, Valuation, VarId};
use landav_engine::{TripCount, cost};
use landav_its::{RangeSpec, SourceProgramBuilder, VarName};

fn here() -> Origin {
    Origin::new("start.py:1")
}

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
    format!("{kind}({bound}) holes={}", result.holes().len())
}

/// `for i in range(start, stop, step): a = 0`, with `n` the one parameter.
/// A literal endpoint is written as `Some(k)`, `None` is the parameter.
fn subject(start: Option<i64>, stop: Option<i64>, step: i64) -> TripCount {
    let mut builder = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let mut endpoint = |literal: Option<i64>| match literal {
        Some(value) => builder.int(value, here()),
        None => builder.var(VarName::new("n"), here()),
    };
    let start = endpoint(start);
    let stop = endpoint(stop);
    let zero = builder.int(0, here());
    let body = builder.assign(VarName::new("a"), zero, here());
    let stride = NonZeroI64::new(step).expect("a non-zero stride");
    let looped = builder.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, stride),
        vec![body],
        here(),
    );
    cost(&builder.build(vec![looped]))
}

fn at(result: &TripCount, n: u64) -> u64 {
    match result
        .bound()
        .expect("a counted loop has a bound")
        .eval(&Bindings([(Symbol::from("n"), n)].into_iter().collect()))
    {
        Nat::Fin(value) => value,
        Nat::Omega => panic!("a counted loop evaluates finitely"),
    }
}

/// How many steps `for i in range(start, stop, step): a = 0` really costs in
/// the engine's unit: one per iteration for the loop's own test, one for the
/// body statement.
fn truth(start: i64, stop: i64, step: i64) -> u64 {
    let span = if step > 0 { stop - start } else { start - stop };
    let iterations = if span <= 0 {
        0
    } else {
        (span + step.abs() - 1) / step.abs()
    };
    u64::try_from(iterations * 2).unwrap()
}

/// **`range(1, n)` is counted, in `n`, and is not an equality.**
#[test]
fn a_positive_literal_start_is_an_upper_bound_in_the_parameter() {
    let result = subject(Some(1), None, 1);
    let shown = describe(&result);
    assert!(
        matches!(result, TripCount::AtMost(_)),
        "the count is max(0, n - 1), which `n` dominates and nothing here can \
         state exactly. Got {shown}"
    );
    for n in 0..8 {
        assert!(
            at(&result, n) >= truth(1, i64::try_from(n).unwrap(), 1),
            "at n = {n} the bound {shown} is below the truth"
        );
    }
}

/// **`range(-2, n)` is counted exactly: `n + 2` iterations.**
#[test]
fn a_negative_literal_start_is_exact() {
    let result = subject(Some(-2), None, 1);
    let shown = describe(&result);
    assert!(
        matches!(result, TripCount::Exact(_)),
        "the count is n + 2 for every natural n, a polynomial. Got {shown}"
    );
    for n in 0..8 {
        assert_eq!(
            at(&result, n),
            truth(-2, i64::try_from(n).unwrap(), 1),
            "at n = {n} the exact bound {shown} is not the truth"
        );
    }
}

/// **The descending mirror: `range(n, 1, -1)` and `range(n, -2, -1)`.**
#[test]
fn a_descending_loop_to_a_literal_is_the_same_arithmetic() {
    let upper = subject(None, Some(1), -1);
    let exact = subject(None, Some(-2), -1);
    assert!(
        matches!(upper, TripCount::AtMost(_)),
        "down to a non-negative literal is dominated by `n`. Got {}",
        describe(&upper)
    );
    assert!(
        matches!(exact, TripCount::Exact(_)),
        "down to -2 runs n + 2 times exactly. Got {}",
        describe(&exact)
    );
    for n in 0..8 {
        let at_n = i64::try_from(n).unwrap();
        assert!(at(&upper, n) >= truth(at_n, 1, -1));
        assert_eq!(at(&exact, n), truth(at_n, -2, -1));
    }
}

/// **A stride above one stays a hole.** `range(1, n, 2)` needs a ceiling
/// division the fragment has no rule for, and nothing here widens that.
#[test]
fn a_non_unit_stride_with_a_symbolic_endpoint_stays_a_hole() {
    for step in [2, -2] {
        let result = if step > 0 {
            subject(Some(1), None, step)
        } else {
            subject(None, Some(1), step)
        };
        assert!(
            result.holes().iter().any(|hole| hole.construct() == "for"),
            "a stride of {step} with a symbolic endpoint is not counted. Got {}",
            describe(&result)
        );
    }
}

/// **Two symbolic endpoints stay a hole.** `range(n, n)` is zero iterations
/// and `range(a, n)` is `n - a`; neither is a value over the naturals this
/// fragment can write, and the ticket does not reach for them.
#[test]
fn two_symbolic_endpoints_stay_a_hole() {
    let result = subject(None, None, 1);
    assert!(
        result.holes().iter().any(|hole| hole.construct() == "for"),
        "`stop - start` is not expressible. Got {}",
        describe(&result)
    );
}
