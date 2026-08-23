//! `LAN-91` acceptance, second half: **in a position that reads no value, a
//! boolean, a comparison and a ternary cost their operands and nothing else.**
//!
//! # Why this file exists beside `conditional_expression.rs`
//!
//! The ticket's headline was "`conditional-expression` blocks 118 stdlib
//! functions", and the first implementation read that as "the ternary blocks
//! 118 functions" and taught `x = a if c else b` to become a branch. That work
//! is right and stays. It moved the number by zero.
//!
//! Decomposing the regions those functions actually carried by Python AST node
//! gives `Compare` 62, `BoolOp` 47, `IfExp` 8. The construct **bucket**
//! conflates three forms, and the ternary is seven per cent of it. What the
//! stdlib is full of is not `x = a if c else b`; it is
//!
//! ```python
//! return sys.base_prefix != sys.prefix or hasattr(sys, "real_prefix")
//! return year % 4 == 0 and (year % 100 != 0 or year % 400 == 0)
//! ```
//!
//! In each, a boolean or a comparison whose value is **returned**.
//!
//! # The argument, which is about the value slot and not about booleans
//!
//! [`landav_its::SourceStmt::Return`] has no value slot: the fragment never
//! represents what a function returns. A bare expression statement has none
//! either. So in those two positions the value of `a and b` is read by nothing:
//! it cannot become a bound variable, because there is nowhere to put it, and
//! the only question left is what evaluating it **costs**. That is its
//! operands' regions, summed, which is an upper bound because `and` may skip
//! its right-hand side and a ternary runs one arm of two.
//!
//! Everything an operand contributes here is a refusal, so the over-charge can
//! only ever be a hole too many. A function carrying a hole is
//! `TripCount::Partial`, whose `exact_elsewhere` speaks for the arithmetic
//! *outside* the holes - untouched by this - so no `Theta` is claimed for a
//! program that short-circuits. `a_skipped_operand_is_never_claimed_exact`
//! pins that.
//!
//! # Value position is deliberately not widened
//!
//! `x = a and b` keeps its `conditional-expression` region. Soundness does not
//! require it - `x` is condemned as `non-integer-value` either way - but the
//! report does: without it the user is told that `x` is not an integer and
//! never told which construct made it one. `a_boolean_in_value_position_stays_
//! a_region` here and `boolean_and_comparison_values_stay_refused` in
//! `conditional_expression.rs` pin the two halves of that fence.
//!
//! # The rule that makes all of this safe
//!
//! `expression_children` returns nothing for a **refused** form, so a refused
//! container's interior is never translated at all. That is sound only while
//! the container itself is a region denoting `omega`. The moment a form is
//! *accepted*, a call inside it would vanish with no hole anywhere and the
//! function would publish a complete bound that omits it - and `Walk::
//! reconciled` cannot catch that, because a node that was never built is not in
//! the arena to reconcile against. So every arm added to `build_expression`
//! gets one in `expression_children`, and
//! `a_discarded_boolean_names_every_call_inside_it` is the test that fails if
//! it does not.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, path::Path};

use landav_bound::{Nat, Symbol, Valuation, VarId};
use landav_engine::{Hole, TripCount, cost};
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

/// A valuation giving every named region the same finite cost.
fn every_region_costs(result: &TripCount, each: u64) -> Bindings {
    Bindings(
        result
            .holes()
            .iter()
            .map(|hole| (hole.var().symbol().clone(), each))
            .collect(),
    )
}

fn only_function(source: &str) -> LoweredFunction {
    let mut functions = landav_python::lower_module(Path::new("discarded.py"), source)
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

/// The regions this result blames on `construct`.
fn holes_blamed_on<'a>(result: &'a TripCount, construct: &str) -> Vec<&'a Hole> {
    result
        .holes()
        .iter()
        .filter(|hole| hole.construct() == construct)
        .collect()
}

// ---------------------------------------------------------------------------
// rule 1: accepting a container obliges you to traverse it
// ---------------------------------------------------------------------------

/// **`return f(n) or g(n)` names two calls.**
///
/// The single most important test in this file, and the one that fails loudly
/// if `build_expression` gains an arm that `expression_children` does not.
///
/// Before this change the whole expression was one `conditional-expression`
/// region: sound, because a region denotes `omega` and `omega` dominates two
/// calls. Accepting the `or` without descending into it would produce a
/// function with **no regions at all** and a complete `Theta(1)` bound for a
/// body that runs two unknown callees - an unsound bound that no later check
/// can recover, because neither call was ever built into an arena for
/// `Walk::reconciled` to notice missing.
///
/// Two, not one: `or` reaching its right-hand side is exactly the case that
/// must be charged, and a translation that stopped at the first operand would
/// still pass a "there is a call region" assertion.
#[test]
fn a_discarded_boolean_names_every_call_inside_it() {
    let source = "def both(n: int) -> int:\n    return f(n) or g(n)\n";
    let function = only_function(source);
    let result = cost(function.program());

    let calls = holes_blamed_on(&result, "call");
    assert_eq!(
        calls.len(),
        2,
        "`or` is accepted in a returned position, so the two calls inside it \
         are the only things left that can cost anything - and each must be \
         named where it stands. A count of 0 means the operands were never \
         translated and this function now publishes a bound that omits two \
         calls; a count of 1 means only the left operand was. Got {} for:\n{source}",
        describe(&result)
    );
    for hole in &calls {
        assert!(
            hole.origin().as_str().contains(':'),
            "a region must be placed as well as named, got {} for:\n{source}",
            hole.origin()
        );
    }
    assert!(
        holes_blamed_on(&result, "conditional-expression").is_empty(),
        "the `or` itself is no longer a region: {} for:\n{source}",
        describe(&result)
    );
}

/// **A call nested two containers deep is still named.**
///
/// `f(n) > g(n) or h(n)` is a `BoolOp` over a `Compare` and a call. Both new
/// containers have to descend for all three to appear, so this fails if either
/// arm of `expression_children` is missing while the other is present.
#[test]
fn a_call_inside_a_comparison_inside_a_boolean_is_still_named() {
    let source = "def nested(n: int) -> int:\n    return f(n) > g(n) or h(n)\n";
    let function = only_function(source);
    let result = cost(function.program());

    assert_eq!(
        holes_blamed_on(&result, "call").len(),
        3,
        "three calls are written and three regions must be reported: {} \
         for:\n{source}",
        describe(&result)
    );
}

/// **Both arms of a returned ternary are charged.**
///
/// Exactly one arm runs, so summing them is an upper bound rather than the
/// truth - deliberately accepted here, because the alternative is losing one of
/// the two calls entirely. `Translator::bind_expression` splits the assignment
/// form into a real branch and gets the maximum; a `return` has no value slot to
/// bind into, so there is nothing to put in the arms of a branch and the sum is
/// what is left. Sound, loose, and stated.
#[test]
fn both_arms_of_a_returned_ternary_are_charged() {
    let source = "def pick(n: int) -> int:\n    return f(n) if n else g(n)\n";
    let function = only_function(source);
    let result = cost(function.program());

    assert_eq!(
        holes_blamed_on(&result, "call").len(),
        2,
        "an arm that is not traversed is a call that vanishes, so both are \
         charged even though one runs: {} for:\n{source}",
        describe(&result)
    );
}

/// **A bare boolean statement names its calls too.**
///
/// The other position with no value slot. `f(n) or g(n)` written as a statement
/// is how a Python programmer spells "call `g` unless `f` answered", and the
/// calls are the whole cost of the line.
#[test]
fn a_bare_boolean_statement_names_its_calls() {
    let source = "def side_effects(n: int) -> int:\n    f(n) or g(n)\n    return 0\n";
    let function = only_function(source);
    let result = cost(function.program());

    assert_eq!(
        holes_blamed_on(&result, "call").len(),
        2,
        "a bare expression statement reads no value either, so the same rule \
         applies and the same two calls must be named: {} for:\n{source}",
        describe(&result)
    );
}

// ---------------------------------------------------------------------------
// what the change buys
// ---------------------------------------------------------------------------

/// **A returned boolean, comparison or ternary over integers derives a bound.**
///
/// These are the shapes the corpus is full of. Each is region-free once the
/// container stops being one, and the whole function is a single `return`
/// costing a single step.
#[test]
fn a_returned_boolean_over_integers_is_fully_derived() {
    for source in [
        "def conjunction(a: int, b: int) -> int:\n    return a and b\n",
        "def disjunction(a: int, b: int) -> int:\n    return a or b\n",
        "def comparison(a: int, b: int) -> int:\n    return a < b\n",
        "def chained(a: int, b: int, c: int) -> int:\n    return a < b < c\n",
        "def guarded(n: int, m: int) -> int:\n    return n > 0 and m > 0\n",
        "def ternary(n: int) -> int:\n    return 1 if n else 2\n",
        "def sign(x: int, y: int) -> int:\n    return 0 if x == y else 1 if x > y else -1\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert!(
            result.is_complete(),
            "nothing in this function is unknown - it returns a value the \
             fragment never has to represent, and evaluating it touches only \
             proven integers: {} for:\n{source}",
            describe(&result)
        );
        assert!(
            landav_its::lower(function.program()).is_ok(),
            "and it must reach a transition system, which is the headline \
             coverage number:\n{source}"
        );
    }
}

// ---------------------------------------------------------------------------
// the fences
// ---------------------------------------------------------------------------

/// **A boolean or comparison bound to a name is still a region.**
///
/// The near half of the fence. `Translator::refusals_of` hoists the right-hand
/// side of `x = a and b` through the same scratch builder a `return` uses, so a
/// widening keyed on "am I in a scratch builder" would take this with it.
///
/// Soundness would survive that: `x` is condemned as `non-integer-value`
/// whatever happens to the `and`. The **report** would not. "`x` is not a
/// proven integer" without "because of the boolean at 2:9" names a symptom and
/// withholds the cause, and a user cannot act on it.
#[test]
fn a_boolean_in_value_position_stays_a_region() {
    for source in [
        "def conjunction(a: int, b: int) -> int:\n    x = a and b\n    return x\n",
        "def disjunction(a: int, b: int) -> int:\n    x = a or b\n    return x\n",
        "def comparison(a: int, b: int) -> int:\n    x = a < b\n    return x\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert!(
            !holes_blamed_on(&result, "conditional-expression").is_empty(),
            "a binding names a value, and the construct that made that value \
             unreadable has to be named beside it. If this is now empty, the \
             discarded-position widening was keyed on the scratch builder \
             rather than on there being no value slot: {} for:\n{source}",
            describe(&result)
        );
    }
}

/// **`in`, `not in`, `is` and `is not` are refused in every position.**
///
/// `x in items` runs `__contains__` and `x is y` compares identities: the first
/// is arbitrary user code with an arbitrary cost, exactly as `x.y` runs a
/// `property`, and the second has no model here at all. `Translator::comparison`
/// already refuses both in condition position under these names, and one
/// operator answers to one construct wherever it is written - so a returned
/// `c in "0123456789"` is a `collection` region and not a
/// `conditional-expression` one.
///
/// The construct name is the assertion. Reporting these as
/// "conditional expression used as a value" told the user to go and look at a
/// comparison when what the tool could not read was the container.
#[test]
fn membership_and_identity_are_refused_wherever_they_are_written() {
    for (source, expected) in [
        (
            "def member(c: int) -> int:\n    return c in \"0123456789\"\n",
            "collection",
        ),
        (
            "def absent(c: int, items: list) -> int:\n    return c not in items\n",
            "collection",
        ),
        (
            "def identity(a: int) -> int:\n    return a is None\n",
            "non-integer-value",
        ),
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert!(
            !holes_blamed_on(&result, expected).is_empty(),
            "membership and identity keep the names condition position already \
             gives them, and `{expected}` is what this one should be: {} \
             for:\n{source}",
            describe(&result)
        );
        assert!(
            holes_blamed_on(&result, "conditional-expression").is_empty(),
            "and they are not comparisons-as-values: {} for:\n{source}",
            describe(&result)
        );
    }
}

/// **A short-circuited operand is charged, and no `Theta` is claimed for it.**
///
/// `f(n) or g(n)` may never call `g`, and both are charged. That is an upper
/// bound and it is the reason this is allowed: everything an operand
/// contributes is a *region*, so over-charging can only ever add a hole - never
/// a number the tool then presents as exact.
///
/// The two assertions are the two halves of that. The result is `Partial`, so
/// nothing here is offered as a `Theta`; and with each region given a finite
/// cost the bound exceeds the cost of taking the short circuit, which is what
/// "upper bound" means and what would break if the sum were ever turned into a
/// maximum for the wrong reason.
#[test]
fn a_skipped_operand_is_charged_but_never_claimed_exact() {
    let source = "def maybe(n: int) -> int:\n    return f(n) or g(n)\n";
    let function = only_function(source);
    let result = cost(function.program());

    assert!(
        matches!(result, TripCount::Partial { .. }),
        "two regions are named, so this is a partial result and not an exact \
         one - a `Theta` here would be claiming a program that short-circuits \
         runs both operands: {} for:\n{source}",
        describe(&result)
    );

    let at = every_region_costs(&result, 7);
    let bound = result
        .bound()
        .unwrap_or_else(|| panic!("no bound at all was derived for:\n{source}"));
    let Nat::Fin(total) = bound.eval(&at) else {
        panic!(
            "the bound is omega with every region finite, so a term escaped \
             the arithmetic: {} for:\n{source}",
            describe(&result)
        )
    };
    assert!(
        total > 7,
        "charging both operands must exceed charging one, or the second call \
         is not in the bound at all: {total} for:\n{source}"
    );
}
