//! `LAN-91` acceptance: **`//`, `%`, `<<`, `>>`, `&`, `|`, `^` - which of the
//! seven can be accepted without inventing a number.**
//!
//! # Why this file is about values and not about coverage
//!
//! The ticket counts refusals: `integer-division` blocks 13 functions on its
//! own across 191 occurrences, `bitwise-operator` 14 across 109. Those are
//! coverage numbers, and coverage is the cheapest thing in this project to buy
//! badly. Every one of these seven operators can be made to stop refusing in an
//! afternoon; the question this file exists to answer is which of them can stop
//! refusing while the number that comes out the other side is still true.
//!
//! `LAN-88` is the precedent and the reason for the shape of these assertions.
//! A **complete** result - `Exact` or `AtMost`, no holes - is offered to a CI
//! budget gate for comparison. A complete result the program exceeds is worse
//! than no result. Nothing in the suite compared a derived bound against real
//! Python semantics, so an unsound one shipped. Every numeric assertion below
//! compares against a truth computed by hand from Python's own rules.
//!
//! # The two facts the bound algebra imposes
//!
//! **There is no division and no subtraction.** [`landav_bound::Bound`] has
//! exactly six constructors - `Const`, `Var`, `Sum`, `Max`, `Prod`,
//! `Trans{Pow, Log}` - and `landav_its::ArithOp` has exactly three, `Add`,
//! `Sub`, `Mul`. So `n // 2` has **no exact representation anywhere in this
//! system**. Whatever an implementer writes for it is an over-approximation,
//! and the only over-approximations available are built from the operands
//! themselves.
//!
//! **Division is anti-monotone in its divisor.** `100 // 1` is `100` and
//! `100 // 50` is `2`: a *bigger* divisor gives a *smaller* quotient. A `Bound`
//! is weakly monotone by construction, so a bound written in terms of the
//! divisor takes its **smallest** value exactly where the true quotient takes
//! its **largest** - at divisor `1`. That is the direction that under-reports,
//! and `a_quotient_bound_must_survive_the_divisor_being_one` is the assertion
//! that catches it. The escape is to write the bound over the **dividend
//! only**: `|a // b| <= |a|` for every `b != 0`, and that inequality is
//! monotone in `a` and mentions `b` nowhere.
//!
//! # Python floors, and where that changes the answer
//!
//! `-7 // 2 == -4` and `-7 % 2 == 1`. C-style truncation gives `-3` and `-1`.
//! For `//` the two agree closely enough that a `range` endpoint cannot tell
//! them apart (both are negative, both run zero times), so the negative-operand
//! test for `//` pins a **refusal**. For `%` they differ where it counts:
//! `range(-7 % 2)` runs **once** under Python and **zero** times under
//! truncation, which is an under-report. That is
//! `a_negative_remainder_is_not_reported_as_zero_iterations`.
//!
//! # `Exact` is a two-sided claim, and `count_of` hands it out for free
//!
//! `Walk::count_of` answers `for i in range(0, e)` with
//! `TripCount::Exact(expr_bound::read(e))`. `read` is total in the sense that
//! whatever it returns is used **as the exact trip count**. Add an
//! over-approximating operator to `read` and every loop over it is silently
//! labelled `Theta` - a claim the program also *achieves* - when the loop runs
//! half as often. Every soundness helper here therefore checks two things: the
//! reported number is never below the truth, and if the result calls itself
//! `Exact` it is equal to it. See [`TripCount::Exact`]'s own doc comment, which
//! already names `ceil(n / k)` as the reason `AtMost` exists.
//!
//! # Why the tests read the libraries rather than driving the binary
//!
//! The assertions are about the derived number and the quality label attached
//! to it. The process boundary offers only a rendered string, which these tests
//! are forbidden to pin.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use core::cmp::Ordering;
use std::{collections::BTreeMap, path::Path};

use landav_bound::{Nat, Symbol, Valuation, VarId};
use landav_engine::{TripCount, cost};
use landav_its::Construct;
use landav_python::LoweredFunction;

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

/// A valuation over an explicit table, zero elsewhere.
struct Bindings(BTreeMap<Symbol, u64>);

impl Valuation for Bindings {
    fn value_of(&self, var: &VarId) -> Nat {
        Nat::Fin(self.0.get(var.symbol()).copied().unwrap_or(0))
    }
}

fn bindings(pairs: &[(&str, u64)]) -> Bindings {
    Bindings(
        pairs
            .iter()
            .map(|(name, value)| (Symbol::from(*name), *value))
            .collect(),
    )
}

fn only_function(source: &str) -> LoweredFunction {
    let mut functions = landav_python::lower_module(Path::new("integers.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    assert_eq!(
        functions.len(),
        1,
        "expected exactly one function in:\n{source}"
    );
    functions.remove(0)
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

/// The one program shape every test in this file uses, with `endpoint`
/// substituted into the `range`.
///
/// One shape so that the reference program below differs from the program
/// under test in **exactly** the endpoint, and so the difference between their
/// costs is entirely the arithmetic being tested. Both parameters are always
/// declared; an unread parameter costs nothing.
fn loop_over(endpoint: &str) -> String {
    format!(
        "\
def counted(n: int, m: int) -> int:
    total = 0
    for i in range({endpoint}):
        total = total + 1
    return total
"
    )
}

/// The cost the engine derives for the *same* program with the endpoint
/// replaced by a literal - the truth, taken from the engine's own statement
/// model rather than hardcoded.
///
/// Model-independent on purpose. Under today's model this is `2 + 2k` - one
/// step for `total = 0`, one for `return total`, and two per iteration - so
/// `range(3)` costs `8`. Hardcoding `2 + 2k` would make every test in this file
/// fail the day a statement's cost changes, for a reason that has nothing to do
/// with division. Deriving it from a literal loop keeps the assertions about
/// the *arithmetic* and nothing else.
///
/// Asserts exactness, because a literal endpoint is the one case the engine has
/// always counted outright; if that stops being exact the truth this file
/// compares against is no longer a truth and every assertion below is void.
fn cost_of_a_loop_running(iterations: u64) -> Nat {
    let source = loop_over(&iterations.to_string());
    let function = only_function(&source);
    let result = cost(function.program());
    assert!(
        result.is_exact(),
        "the reference loop `range({iterations})` has a literal endpoint and \
         must be counted outright - without it there is no truth to compare \
         against and every assertion in this file is vacuous. Got {} for:\n{source}",
        describe(&result)
    );
    result
        .bound()
        .expect("an exact result carries a bound")
        .eval(&bindings(&[]))
}

/// **The soundness assertion of this file.**
///
/// `endpoint` is evaluated at `at`; the loop truly runs `iterations` times, a
/// number the caller has worked out from Python's rules by hand.
///
/// Three obligations, and they fail for three different reasons:
///
/// * every name in the bound is a parameter, so the caller has something to
///   supply for it - a bound mentioning an internal name evaluates to zero and
///   turns a real cost into a small one;
/// * the reported number is **never below** the truth - the `LAN-88` failure;
/// * a result calling itself `Exact` is **equal** to the truth - `Exact` is a
///   `Theta` claim, and `count_of` attaches it to whatever `expr_bound::read`
///   returns without asking whether the read was approximate.
///
/// A **partial** result is skipped: an unfilled hole denotes `omega`, so it
/// makes no finite claim and cannot be exceeded. That is why every test that
/// skips here also asserts, separately, that a complete bound was derived at
/// all - otherwise "refuse everything" would pass this file.
fn assert_endpoint_never_under_reports(endpoint: &str, at: &[(&str, u64)], iterations: u64) {
    let source = loop_over(endpoint);
    let function = only_function(&source);
    let result = cost(function.program());
    let shown = describe(&result);
    if !result.is_complete() {
        return;
    }
    let bound = result.bound().expect("a complete result carries a bound");

    let params: Vec<String> = function
        .program()
        .params()
        .iter()
        .map(|param| param.symbol().as_str().to_owned())
        .collect();
    for var in bound.vars() {
        let name = var.symbol().as_str().to_owned();
        assert!(
            params.contains(&name),
            "the bound for `range({endpoint})` mentions `{name}`, which is not \
             a parameter - the caller has nothing to supply for it, so \
             `Bound::eval` reads it as zero and the cost collapses. Parameters \
             are {params:?}, got {shown}"
        );
    }

    let reported = bound.eval(&bindings(at));
    let truth = cost_of_a_loop_running(iterations);
    assert!(
        reported.magnitude_cmp(truth) != Ordering::Less,
        "UNSOUND: at {at:?} the endpoint `{endpoint}` evaluates to \
         {iterations} under Python, so the loop executes {truth:?} steps, and \
         the reported bound evaluates to {reported:?}. A complete bound is \
         offered for comparison against a budget; one the program exceeds is \
         worse than no bound at all.\n  got: {shown}"
    );
    if result.is_exact() {
        assert_eq!(
            reported, truth,
            "MISLABELLED: `range({endpoint})` at {at:?} is reported `Exact`, \
             which is a two-sided `Theta` claim - the program must also \
             *achieve* it - but the loop executes {truth:?} steps and the bound \
             says {reported:?}. `Walk::count_of` wraps whatever \
             `expr_bound::read` returns in `TripCount::Exact`, so an \
             over-approximating read must arrive as `AtMost`, not as `Exact`.\
             \n  got: {shown}"
        );
    }
}

/// The driver half: the endpoint must stop being a hole.
///
/// Separate from the soundness helper because the two must be able to fail
/// independently. Passing this and failing that is an unsound acceptance;
/// failing this and passing that is the status quo, in which every assertion
/// about the number is vacuous.
fn assert_endpoint_is_derived(endpoint: &str) {
    let source = loop_over(endpoint);
    let result = cost(only_function(&source).program());
    assert!(
        result.is_complete(),
        "`range({endpoint})` is holed, so this function makes no finite claim \
         about its own cost. This is `LAN-91`'s coverage half: the arithmetic \
         is decidable and the loop should be counted. Got {} for:\n{source}",
        describe(&result)
    );
}

/// Every refusal the program carries, as constructs.
fn refusals_of(function: &LoweredFunction) -> Vec<Construct> {
    function
        .program()
        .unsupported_nodes()
        .map(|node| node.construct())
        .collect()
}

// ---------------------------------------------------------------------------
// 1. floor division by a literal - the soundness-critical case
// ---------------------------------------------------------------------------

/// **`for i in range(n // 2)` runs `n // 2` times, and the reported bound must
/// never be below that.**
///
/// The arithmetic, by hand, and deliberately including odd `n` because that is
/// where floor division stops being "half":
///
/// | `n` | `n // 2` | steps (`2 + 2 * iterations`) |
/// |----:|---------:|-----------------------------:|
/// | 0   | 0        | 2                            |
/// | 1   | 0        | 2                            |
/// | 6   | 3        | 8                            |
/// | 7   | 3        | 8                            |
/// | 8   | 4        | 10                           |
/// | 9   | 4        | 10                           |
/// | 101 | 50       | 102                          |
///
/// There is no `n // 2` in the bound algebra, so the tightest thing an
/// implementer can write here is `n` itself: `AtMost(2 + 2n)`, twice the truth
/// and sound. The trap this catches is the operand mix-up - returning the
/// *divisor* rather than the dividend gives `Const(2)`, so `range(101 // 2)`
/// would be reported at `6` steps for a loop that executes `102`.
#[test]
fn a_quotient_range_endpoint_is_never_under_reported() {
    for (n, iterations) in [
        (0_u64, 0_u64),
        (1, 0),
        (6, 3),
        (7, 3),
        (8, 4),
        (9, 4),
        (101, 50),
    ] {
        assert_endpoint_never_under_reports("n // 2", &[("n", n)], iterations);
    }
    assert_endpoint_is_derived("n // 2");
}

/// **A quotient endpoint may not be called `Exact` while it over-approximates.**
///
/// The stark case, pulled out of the table above because it is the one a reader
/// should see: `101 // 2 == 50`, so the loop executes `102` steps. The only
/// bound available is `n`, giving `202`. That is sound as an `AtMost` and false
/// as a `Theta` - `Theta` says the program also reaches it, and this program
/// reaches half of it.
///
/// This fails the moment someone teaches `expr_bound::read` about `//` without
/// also teaching `Walk::count_of` that the read was approximate, which is the
/// single most likely way to get this ticket wrong.
#[test]
fn a_quotient_range_endpoint_is_not_reported_as_a_two_sided_claim() {
    let source = loop_over("n // 2");
    let result = cost(only_function(&source).program());
    if !result.is_complete() {
        return;
    }
    let reported = result
        .bound()
        .expect("complete")
        .eval(&bindings(&[("n", 101)]));
    let truth = cost_of_a_loop_running(50);
    assert!(
        !result.is_exact() || reported == truth,
        "`range(n // 2)` at n = 101 runs 50 times ({truth:?} steps) and the \
         bound reports {reported:?}. Reporting that as `Exact` claims the \
         program achieves it. `ceil(n / k)` is named in `TripCount::AtMost`'s \
         own doc comment as the reason that variant exists; use it. Got {}",
        describe(&result)
    );
}

// ---------------------------------------------------------------------------
// 2. remainder
// ---------------------------------------------------------------------------

/// **`for i in range(n % 5)` runs `n % 5` times - at most `4`, whatever `n`.**
///
/// By hand: `3 % 5 == 3`, `4 % 5 == 4`, `5 % 5 == 0`, `7 % 5 == 2`,
/// `9 % 5 == 4`, `1000 % 5 == 0`.
///
/// This is the most valuable acceptance in the lane and the only one that gets
/// *tighter* than the operands: for a positive literal divisor `k` the result
/// lies in `0 ..= k - 1`, so `Const(k - 1)` bounds it, and a loop whose
/// endpoint is `n % 5` costs a **constant** however large `n` grows. Better
/// still, that inequality survives Python's floored modulo for a negative
/// dividend too - `-7 % 5 == 3`, still in `0 ..= 4` - because the result takes
/// the sign of the divisor.
///
/// The trap: `n % k` is **not** bounded by `n` alone in any useful sense and is
/// **not** bounded by `k` if `k` is read as the whole range rather than
/// `k - 1`; both of those are sound, so this test cannot distinguish them and
/// does not try. What it does catch is a bound of `0`, which is what "modulo is
/// small, call it nothing" produces.
#[test]
fn a_remainder_range_endpoint_is_never_under_reported() {
    for (n, iterations) in [
        (0_u64, 0_u64),
        (3, 3),
        (4, 4),
        (5, 0),
        (7, 2),
        (9, 4),
        (1000, 0),
    ] {
        assert_endpoint_never_under_reports("n % 5", &[("n", n)], iterations);
    }
    assert_endpoint_is_derived("n % 5");
}

// ---------------------------------------------------------------------------
// 3. negative operands - Python floors
// ---------------------------------------------------------------------------

/// **`range(-7 % 2)` runs once, and must never be reported as running zero
/// times.**
///
/// This is the whole floor-versus-truncate question in one number. Python's
/// `%` takes the sign of the **divisor**, so `-7 % 2 == 1` and the loop
/// executes one iteration - `4` steps. C-style truncation gives `-1`, and
/// `range(-1)` is empty - `2` steps. An implementer who lowers `%` to Rust's
/// `%` on `i64`, or who reasons about it in C, derives `2` for a loop that
/// executes `4`. That is an under-report of exactly the class this project
/// targets at zero.
///
/// A refusal is an acceptable answer and is what happens today: `expr_bound`
/// declines negative literals and `SourceExpr::Neg` outright, because bounds
/// live in `N u {omega}` and there is no natural number to map `-7` to. So this
/// test is a **fence, not a driver** - it passes now and must keep passing
/// whichever way the implementer goes.
#[test]
fn a_negative_remainder_is_not_reported_as_zero_iterations() {
    assert_endpoint_never_under_reports("-7 % 2", &[], 1);
}

/// **A negative dividend acquires no bound.**
///
/// `-7 // 2 == -4` under Python and `-3` under truncation. Neither is a natural
/// number, so neither has a `Bound`, and the honest answer is to refuse - which
/// is what `expr_bound::read` already does for `SourceExpr::Neg` and for any
/// literal that fails `u64::try_from`.
///
/// Stated as a fence rather than a `//`-specific rule: a *complete* bound for a
/// program whose endpoint is negative would mean something in the chain decided
/// a negative integer had a magnitude in `N`, and that decision is wrong
/// wherever it was made. `(0 - n) // 2` reaches the same place through
/// subtraction, which `read` also refuses.
#[test]
fn a_negative_dividend_does_not_acquire_a_complete_bound() {
    for endpoint in ["-7 // 2", "(0 - n) // 2", "(0 - n) % 2"] {
        let source = loop_over(endpoint);
        let result = cost(only_function(&source).program());
        assert!(
            !result.is_complete(),
            "`range({endpoint})` has a negative dividend. Python floors, so \
             `-7 // 2 == -4`, and there is no natural number for that - a \
             complete bound here means something mapped a negative integer into \
             `N`. Got {} for:\n{source}",
            describe(&result)
        );
    }
}

// ---------------------------------------------------------------------------
// 4. shifts
// ---------------------------------------------------------------------------

/// **`n << 1` is `2n` exactly, and a bound of `n` for it is unsound.**
///
/// The one operator in this lane that is *exactly* representable:
/// `a << k == a * 2^k` for every integer `a` and every non-negative literal
/// `k`, and `Prod[Var(a), Const(2^k)]` is that product. No approximation, so
/// the loop stays genuinely `Exact`.
///
/// By hand: `range(7 << 1)` is `range(14)`, `14` iterations, `30` steps;
/// `range(101 << 1)` is `range(202)`, `406` steps.
///
/// The trap is the reason this test carries numbers rather than just checking
/// for a bound. `<<` sits in the same `Construct::BitwiseOperator` bucket as
/// `>>`, and `>>` **is** bounded by its shifted operand. An implementer who
/// handles the bucket uniformly and bounds `n << 1` by `n` reports `16` steps
/// for a loop that executes `30`. Shifting left is the only one of the seven
/// where the result is **larger** than the operand.
#[test]
fn a_left_shift_range_endpoint_is_never_under_reported() {
    for (n, iterations) in [(0_u64, 0_u64), (1, 2), (7, 14), (101, 202)] {
        assert_endpoint_never_under_reports("n << 1", &[("n", n)], iterations);
    }
    assert_endpoint_is_derived("n << 1");
}

/// **`n >> 1` is floor division by two.**
///
/// `a >> k == a // 2^k`, so this is the quotient case wearing a different
/// operator, with the same consequence: no exact representation, the dividend
/// as the only available bound, and therefore `AtMost` rather than `Exact`.
///
/// By hand: `7 >> 1 == 3` (`8` steps), `8 >> 1 == 4` (`10`), `101 >> 1 == 50`
/// (`102`), `101 >> 3 == 12` (`26`).
#[test]
fn a_right_shift_range_endpoint_is_never_under_reported() {
    for (n, iterations) in [(0_u64, 0_u64), (1, 0), (7, 3), (8, 4), (101, 50)] {
        assert_endpoint_never_under_reports("n >> 1", &[("n", n)], iterations);
    }
    assert_endpoint_never_under_reports("n >> 3", &[("n", 101)], 12);
    assert_endpoint_is_derived("n >> 1");

    // A symbolic shift amount is `n // 2^m`, still bounded by `n`, so it is
    // sound to accept and is *not* demanded here - only checked if accepted.
    // `m = 0` is the sharp valuation: `100 >> 0 == 100`, the full count.
    assert_endpoint_never_under_reports("n >> m", &[("n", 100), ("m", 0)], 100);
    assert_endpoint_never_under_reports("n >> m", &[("n", 100), ("m", 1)], 50);
    assert_endpoint_never_under_reports("n >> m", &[("n", 100), ("m", 6)], 1);
}

/// **A symbolic or oversized shift amount does not produce a finite bound.**
///
/// Two reasons, and neither is "it would be hard":
///
/// * `n << m` is `n * 2^m`. The bound algebra *can* hold that - `Bound::pow`
///   raises a constant base to a bound exponent - but `SourceExpr` cannot:
///   its doc comment states that every variant but `Unsupported` denotes a
///   polynomial, and `2^m` is not one. The ITS is the other consumer of this
///   program, and a `SourceExpr` it cannot lower breaks a documented invariant
///   for every consumer at once.
/// * `n << 200` needs `2^200`, which is not a `u64`. Saturating to `omega` is
///   sound and is why the assertion below permits an infinite bound; wrapping
///   is the `LAN-88` failure again, and the workspace denies truncating casts
///   for exactly this reason. A *finite* bound here is the wrong answer.
#[test]
fn a_symbolic_or_oversized_shift_does_not_produce_a_finite_bound() {
    for endpoint in ["n << m", "n << 200", "n << 64"] {
        let source = loop_over(endpoint);
        let result = cost(only_function(&source).program());
        let finite = result.bound().is_some_and(landav_bound::Bound::is_finite);
        assert!(
            !result.is_complete() || !finite,
            "`range({endpoint})` must not yield a complete *finite* bound: a \
             symbolic exponent is not a polynomial and `2^200` is not a `u64`. \
             Refusing is fine and `omega` is fine; a finite number is a claim \
             that a wrapped or truncated constant would satisfy. Got {} \
             for:\n{source}",
            describe(&result)
        );
    }
}

/// **A shift by a negative amount raises, and must not panic the analyser.**
///
/// `n << -1` raises `ValueError` before the loop begins, so the loop executes
/// nothing and any upper bound is sound. The hazard is on this side: an
/// implementer computing `2i64.pow(-1)` or `1 << -1` in Rust gets a panic or an
/// arithmetic overflow, and this test is a test only because a panic in the
/// analyser is a test failure.
#[test]
fn a_negative_shift_amount_does_not_erase_or_panic() {
    for endpoint in ["n << -1", "n >> -1"] {
        let source = loop_over(endpoint);
        let result = cost(only_function(&source).program());
        assert!(
            !matches!(result, TripCount::Unknown),
            "`range({endpoint})` erased the whole function's bound. A shift \
             that raises at runtime is one bad expression, not a reason to stop \
             analysing around it. Got {} for:\n{source}",
            describe(&result)
        );
    }
}

// ---------------------------------------------------------------------------
// 5. division by a variable - the anti-monotone case
// ---------------------------------------------------------------------------

/// **`for i in range(n // m)` may be counted, but only over `n`.**
///
/// By hand, at the valuations below:
///
/// | `n`  | `m`  | `n // m` | steps |
/// |-----:|-----:|---------:|------:|
/// | 7    | 2    | 3        | 8     |
/// | 7    | 1    | 7        | 16    |
/// | 7    | 100  | 0        | 2     |
/// | 100  | 3    | 33       | 68    |
///
/// The available bound is the dividend: `|a // b| <= |a|` for every `b != 0`,
/// including negative `b` (`7 // -2 == -4`, `-7 // -2 == 3`), and `b == 0`
/// raises so the loop executes nothing at all. That inequality mentions the
/// divisor nowhere, which is precisely what makes it expressible here.
#[test]
fn a_variable_divisor_range_endpoint_is_never_under_reported() {
    for (n, m, iterations) in [
        (7_u64, 2_u64, 3_u64),
        (7, 1, 7),
        (7, 100, 0),
        (100, 3, 33),
        (0, 1, 0),
    ] {
        assert_endpoint_never_under_reports("n // m", &[("n", n), ("m", m)], iterations);
    }
    assert_endpoint_is_derived("n // m");
}

/// **A quotient bound must survive the divisor being `1`.**
///
/// The anti-monotonicity trap, isolated. Division is **not** monotone in its
/// second operand - a bigger divisor gives a smaller quotient - while every
/// `Bound` is weakly monotone in every variable it mentions. So a bound written
/// over `m` is at its **minimum** at `m = 1`, which is exactly where the true
/// quotient is at its **maximum**.
///
/// Concretely: `100 // 1 == 100`, so the loop executes `202` steps. A bound of
/// `m` evaluates to `4` there. A bound of `n + m` evaluates to `204` and
/// survives; a bound of `n` to `202` and survives, tightly. This test does not
/// forbid mentioning `m` - `n + m` is sound - it forbids the number coming out
/// small when `m` does.
///
/// Two divisors at the same `n`, because a bound that happens to be large at
/// `m = 100` tells you nothing about `m = 1`.
#[test]
fn a_quotient_bound_must_survive_the_divisor_being_one() {
    assert_endpoint_never_under_reports("n // m", &[("n", 100), ("m", 1)], 100);
    assert_endpoint_never_under_reports("n // m", &[("n", 100), ("m", 2)], 50);
    assert_endpoint_never_under_reports("n % m", &[("n", 100), ("m", 7)], 2);
    assert_endpoint_never_under_reports("n % m", &[("n", 6), ("m", 1000)], 6);
}

// ---------------------------------------------------------------------------
// 6. division by zero
// ---------------------------------------------------------------------------

/// **`n // 0` raises, and the analysis must neither panic nor claim a count.**
///
/// Two hazards, and the first is in the analyser rather than in the analysed
/// program. `7 // 0` is two literals, so anything that constant-folds
/// expressions will evaluate it - and `7i64 / 0` **panics** in Rust. A test
/// that merely lowers this program is the check, because a panic in the
/// analyser is a test failure.
///
/// The second is the claim. At runtime the `range` argument raises before the
/// loop begins, so the loop executes zero iterations. Any upper bound is
/// therefore sound - but `Exact` is not an upper bound, it is a `Theta`, and
/// `Exact(n)` for a loop that runs zero times is false. The honest answers are
/// a refusal or an `AtMost`.
#[test]
fn dividing_by_the_literal_zero_neither_panics_nor_claims_an_exact_count() {
    for endpoint in ["7 // 0", "7 % 0", "n // 0", "n % 0"] {
        let source = loop_over(endpoint);
        let function = only_function(&source);
        let result = cost(function.program());
        assert!(
            !matches!(result, TripCount::Unknown),
            "`range({endpoint})` erased the whole function's bound. A division \
             by zero is a runtime error in one expression, not a reason to stop \
             analysing the function around it. Got {} for:\n{source}",
            describe(&result)
        );
        assert!(
            !result.is_exact(),
            "`range({endpoint})` raises `ZeroDivisionError` before the loop \
             begins, so it executes zero iterations. `Exact` claims the program \
             achieves the reported count, which it never does. Got {} \
             for:\n{source}",
            describe(&result)
        );
    }
}

// ---------------------------------------------------------------------------
// 7. the three that must stay refused
// ---------------------------------------------------------------------------

/// **`&`, `|` and `^` stay refused.**
///
/// The honest reason, because the obvious one is wrong. It is *not* that no
/// monotone over-approximation exists: for non-negative operands
/// `a & b <= a`, and `a | b <= a + b`, and `a ^ b <= a + b`, and all three of
/// those right-hand sides are perfectly good `Bound`s. Non-monotonicity of the
/// operator does not prevent a monotone function that dominates it.
///
/// It is that every one of those bounds is **vacuous or unjustified**:
///
/// * `a | b <= a + b` and `a ^ b <= a + b` say nothing a reader could not have
///   guessed, and they are not what the code means - a bitwise or over flags is
///   not a magnitude anything loops over;
/// * `a & b <= min(a, b)` is the only tight one, and there is no `Min`
///   constructor in the bound algebra, so an implementer must pick an operand.
///   Picking by "whichever is the literal" is an ad-hoc rule with no soundness
///   argument behind it;
/// * all three inequalities require **both** operands to be non-negative, and
///   `-1 & b == b`, `-1 | b == -1`, `-2 ^ b` is negative. A parameter annotated
///   `int` says nothing about its sign, so the precondition is one the frontend
///   cannot establish.
///
/// 109 occurrences blocking 14 functions does not buy a new class of
/// approximation whose correctness rests on a fact nobody checked. This test
/// exists so that an implementer counting refusals cannot quietly accept them:
/// changing this decision means changing this test, in a commit that says so.
#[test]
fn bitwise_and_or_and_xor_stay_refused() {
    for (operator, endpoint) in [("&", "n & 7"), ("|", "n | 7"), ("^", "n ^ 7")] {
        // In a `range` endpoint the refusal is reported as `non-integer-value`,
        // because the loop counter is what fails to be provably integral. The
        // named `bitwise-operator` refusal appears at a binding site, so both
        // positions are checked.
        let source = loop_over(endpoint);
        let result = cost(only_function(&source).program());
        assert!(
            !result.is_complete(),
            "`range({endpoint})` must not yield a complete bound. `{operator}` \
             is not a magnitude: every sound bound for it either says nothing \
             (`a + b`) or assumes both operands non-negative, which an `int` \
             annotation does not establish. Got {} for:\n{source}",
            describe(&result)
        );

        let binding = format!(
            "\
def masked(n: int, m: int) -> int:
    q = {endpoint}
    return q
"
        );
        let function = only_function(&binding);
        let refusals = refusals_of(&function);
        assert!(
            refusals.contains(&Construct::BitwiseOperator),
            "`q = {endpoint}` must still be refused as `bitwise-operator` so \
             the coverage report names it. Refusals are {refusals:?} \
             for:\n{binding}"
        );
    }
}

// ---------------------------------------------------------------------------
// 8. none of the seven may erase the function
// ---------------------------------------------------------------------------

/// **A refused operator costs its own expression, never the whole function.**
///
/// `TripCount::Unknown` means the engine found a refusal node nothing charged
/// and failed closed on the entire function - the `LAN-90` shape. All seven
/// operators must land as a hole or as a bound, in every position, whichever
/// way the acceptance decision goes.
///
/// The sweep covers a binding, an augmented assignment, a `return` value and a
/// `range` endpoint, because those are four different arms of the translator
/// and `LAN-90` was one arm that a sweep missed.
#[test]
fn no_integer_operator_erases_the_enclosing_function() {
    let operators = ["//", "%", "<<", ">>", "&", "|", "^"];
    for operator in operators {
        let sources = [
            format!(
                "\
def bound_it(n: int, m: int) -> int:
    q = n {operator} 2
    return q
"
            ),
            format!(
                "\
def augment(n: int, m: int) -> int:
    n {operator}= 2
    return n
"
            ),
            format!(
                "\
def returned(n: int, m: int) -> int:
    return n {operator} 2
"
            ),
            loop_over(&format!("n {operator} 2")),
        ];
        for source in sources {
            let result = cost(only_function(&source).program());
            assert!(
                !matches!(result, TripCount::Unknown),
                "`{operator}` erased the whole function's bound. A refused \
                 operator must cost a region, not the function around it - \
                 `Unknown` means an operand was translated and then orphaned. \
                 Got {} for:\n{source}",
                describe(&result)
            );
        }
    }
}

/// **A quotient bound to a local does not cost an unrelated loop its count.**
///
/// This is `LAN-91`'s coverage half, and the shape behind "13 functions blocked
/// solely by `integer-division`". `q = n // 2` is refused, and a refusal is a
/// region; `Walk::region` clears every readable name, because a region may
/// assign to anything. So the `for i in range(n)` after it - over a parameter
/// nothing touches - loses its exact count too, and the whole function reports
/// `3 + #hole0 + #hole1 + #hole2` instead of `3 + 2n`.
///
/// The half is worth `2n`, not a rounding error. Accepting `//` as an
/// *expression* removes the region and the loop is counted again, exactly. This
/// test is the driver for that: it says nothing about what bound `q` gets, only
/// that computing a quotient must stop being an event that forgets everything
/// the function knew.
#[test]
fn a_quotient_bound_to_a_local_does_not_cost_an_unrelated_loop_its_count() {
    let source = "\
def half_then_walk(n: int, m: int) -> int:
    q = n // 2
    total = 0
    for i in range(n):
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let shown = describe(&result);

    let names: Vec<String> = result.bound().map_or_else(Vec::new, |bound| {
        bound
            .vars()
            .iter()
            .map(|var| var.symbol().as_str().to_owned())
            .collect()
    });
    assert!(
        names.iter().any(|name| name == "n"),
        "the loop after `q = n // 2` runs `n` times over a parameter nothing \
         assigns, so the bound must still be a function of `n`. Today the \
         refused quotient is a region, `Walk::region` clears every readable \
         name, and the loop is holed as well - which is how one `//` blocks a \
         whole function. Got {shown} for:\n{source}"
    );
    assert_eq!(
        result.holes().len(),
        0,
        "`q = n // 2` is decidable arithmetic and the loop after it is a plain \
         counted loop; neither should leave a hole. Got {shown} for:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// 9. the control: this file must not be able to pass vacuously
// ---------------------------------------------------------------------------

/// **The harness compares real numbers, and fails when they disagree.**
///
/// Every soundness assertion above returns early on a partial result, because a
/// hole denotes `omega` and makes no finite claim. That is correct and it is
/// also how this whole file could quietly stop testing anything: if `//` were
/// accepted in a way that still left a hole somewhere, or if
/// `cost_of_a_loop_running` stopped producing a truth, every assertion would
/// pass while checking nothing.
///
/// So this test does three things:
///
/// * runs the harness on `range(n)`, which is derived exactly today, and
///   confirms it is found complete, sound and *equal* to the truth - proving
///   both the soundness branch and the `Exact` branch are live;
/// * confirms `cost_of_a_loop_running` returns the number the statement model
///   actually produces (`2 + 2k`: one step for `total = 0`, one for
///   `return total`, two per iteration), so the truths quoted in the doc
///   comments above are the truths being compared against;
/// * feeds the harness a deliberately wrong truth and requires it to **panic**.
///   A soundness assertion that cannot fail is decoration, and this project
///   shipped `LAN-88` with exactly that kind of decoration in place.
#[test]
fn the_harness_compares_real_numbers_and_can_fail() {
    // The statement model, pinned once so the hand arithmetic above is checked.
    assert_eq!(
        cost_of_a_loop_running(0),
        Nat::Fin(2),
        "`total = 0` and `return total`, and no iterations"
    );
    assert_eq!(
        cost_of_a_loop_running(3),
        Nat::Fin(8),
        "two statements plus two steps for each of three iterations"
    );
    assert_eq!(cost_of_a_loop_running(50), Nat::Fin(102));

    // The live positive control: an endpoint the engine derives today.
    for n in [0_u64, 1, 7, 50] {
        assert_endpoint_never_under_reports("n", &[("n", n)], n);
    }
    assert_endpoint_is_derived("n");
    let derived = cost(only_function(&loop_over("n")).program());
    assert!(
        derived.is_exact(),
        "`range(n)` over an unassigned parameter is exact, so the `Exact` \
         branch of the harness is exercised by the loop above. Got {}",
        describe(&derived)
    );

    // The negative control: the harness must reject a false truth.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let rejected = std::panic::catch_unwind(|| {
        // `range(n)` at n = 3 costs 8. Claiming it runs 100 times (202 steps)
        // is a truth the reported bound does not meet, so this must panic.
        assert_endpoint_never_under_reports("n", &[("n", 3)], 100);
    });
    std::panic::set_hook(previous);
    assert!(
        rejected.is_err(),
        "the soundness harness accepted a bound of 8 steps against a truth of \
         202. It cannot detect an under-report, so every assertion in this \
         file is decoration and `LAN-88` can happen again"
    );
}
