//! `LAN-102`, the Python half: **`range(k, n)` with a literal start is counted.**
//!
//! `landav-engine/tests/literal_start.rs` pins the arithmetic on programs built
//! by hand. This file pins that the shapes a Python programmer actually writes
//! reach it: `range(1, n)` to skip a header, `range(n, -1, -1)` to count down,
//! and `range(-3, 4)` with both ends in hand - which was a hole too, because
//! `-3` is the negation of `3` in Python's grammar and the engine only
//! recognised an unnegated literal.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, path::Path};

use landav_bound::{Nat, Symbol, Valuation, VarId};
use landav_engine::{TripCount, cost};

struct Bindings(BTreeMap<Symbol, u64>);

impl Valuation for Bindings {
    fn value_of(&self, var: &VarId) -> Nat {
        Nat::Fin(self.0.get(var.symbol()).copied().unwrap_or(0))
    }
}

fn result_of(source: &str) -> TripCount {
    let functions = landav_python::lower_module(Path::new("start.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    let function = functions
        .into_iter()
        .next_back()
        .unwrap_or_else(|| panic!("expected a function in:\n{source}"));
    cost(function.program())
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
    let holes: Vec<String> = result.holes().iter().map(ToString::to_string).collect();
    format!("{kind}({bound}) holes={holes:?}")
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

/// `2 + iterations * 2`: the two statements outside the loop, then the loop's
/// own step and its one-statement body per iteration.
fn truth(iterations: u64) -> u64 {
    2 + iterations * 2
}

/// **Skipping a header: `range(1, n)` is a complete upper bound in `n`.**
#[test]
fn skipping_the_first_element_is_an_upper_bound_in_the_parameter() {
    let result = result_of(
        "\
def skip_header(n: int) -> int:
    total = 0
    for i in range(1, n):
        total = total + 1
    return total
",
    );
    let shown = describe(&result);
    assert!(
        matches!(result, TripCount::AtMost(_)),
        "`range(1, n)` runs max(0, n - 1) times, which `n` dominates and the \
         fragment cannot state exactly. Got {shown}"
    );
    for n in 0..6 {
        assert!(
            at(&result, n) >= truth(n.saturating_sub(1)),
            "at n = {n}: {shown}"
        );
    }
}

/// **Counting down to a negative literal is exact: `range(n, -1, -1)` runs
/// `n + 1` times.**
#[test]
fn counting_down_to_a_negative_literal_is_exact() {
    let result = result_of(
        "\
def countdown(n: int) -> int:
    total = 0
    for i in range(n, -1, -1):
        total = total + 1
    return total
",
    );
    let shown = describe(&result);
    assert!(
        matches!(result, TripCount::Exact(_)),
        "`range(n, -1, -1)` runs n + 1 times for every natural n. Got {shown}"
    );
    for n in 0..6 {
        assert_eq!(at(&result, n), truth(n + 1), "at n = {n}: {shown}");
    }
}

/// **Both endpoints literal, one of them negative, is a constant.** This was a
/// hole before `LAN-102` for the grammar reason in the module doc, not for any
/// reason of arithmetic.
#[test]
fn a_negative_literal_endpoint_is_still_a_literal() {
    let result = result_of(
        "\
def fixed() -> int:
    total = 0
    for i in range(-3, 4):
        total = total + 1
    return total
",
    );
    let shown = describe(&result);
    assert!(
        matches!(result, TripCount::Exact(_)),
        "seven iterations, known in full. Got {shown}"
    );
    assert_eq!(at(&result, 0), truth(7), "{shown}");
}

/// **A body whose cost depends on the counter is still an upper bound, never
/// exact.** The count of `range(-2, n)` is exact, but the summation route is
/// reserved for `range(0, n)`, so the inner loop's cost is closed over the
/// counter's ceiling `n` - an over-approximation, and labelled as one.
#[test]
fn a_body_costing_the_counter_stays_an_upper_bound() {
    let result = result_of(
        "\
def nested(n: int) -> int:
    total = 0
    for i in range(-2, n):
        for j in range(i):
            total = total + 1
    return total
",
    );
    let shown = describe(&result);
    assert!(
        matches!(result, TripCount::AtMost(_)),
        "the outer count is exact but the inner loop costs `i` per iteration, \
         closed over `n`, which is not an equality. Got {shown}"
    );
    // Truth at n = 4: i runs -2..3, inner runs max(0, i) times: 0+0+0+1+2+3 = 6
    // inner iterations at 2 steps each, plus 6 outer iterations at 2 steps
    // each (the loop step and the inner loop statement), plus 2 outside.
    assert!(at(&result, 4) >= 2 + 6 * 2 + 6 * 2, "at n = 4: {shown}");
}
