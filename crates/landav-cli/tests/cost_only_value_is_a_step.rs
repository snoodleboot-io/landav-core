//! `LAN-117`: **a value nothing reads costs one step, not a hole.**
//!
//! A third of every hole on the standard library was `non-integer-value`, and
//! most of those stood on `return self.x`, `name = value` and `return None`:
//! a read of a name this fragment cannot hold, or a binding to one. The hole
//! never bounded a *cost* - a read is one step, a binding is one step - it stood
//! for an unknown *value*. The engine already has a narrower answer for that:
//! the binding forgets the one name it binds (`LAN-100`), and a read that needs
//! the value - a guard, a `range`, an operator - refuses on its own where it
//! stands. So the cost-only positions are now steps, and this pins both the
//! gain and the fence around it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_engine::{TripCount, cost};
use landav_python::LoweredFunction;

/// The last function `source` lowers to.
fn subject(source: &str) -> LoweredFunction {
    let functions = landav_python::lower_module(Path::new("cost_only.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    functions
        .into_iter()
        .next_back()
        .unwrap_or_else(|| panic!("expected a function in:\n{source}"))
}

fn result(body: &str) -> TripCount {
    let source = format!("def g(x, y, items: list, n: int) -> int:\n{body}\n");
    cost(subject(&source).program())
}

fn describe(result: &TripCount) -> String {
    let bound = result
        .bound()
        .map_or_else(|| "-".to_owned(), ToString::to_string);
    let holes: Vec<String> = result
        .holes()
        .iter()
        .map(|hole| hole.construct().to_owned())
        .collect();
    format!("{bound} holes={holes:?}")
}

fn constructs(result: &TripCount) -> Vec<&str> {
    result.holes().iter().map(|hole| hole.construct()).collect()
}

fn mentions(result: &TripCount, symbol: &str) -> bool {
    result.bound().is_some_and(|bound| {
        bound
            .vars()
            .iter()
            .any(|var| var.symbol().as_str() == symbol)
    })
}

/// **A bare name or a non-integer constant, returned or bound, is a step.**
#[test]
fn a_cost_only_read_or_binding_derives_exactly() {
    for body in [
        "    return x",
        "    return None",
        "    z = x\n    return 1",
        "    z = None\n    return 1",
        "    z = 2.5\n    return 1",
        "    z = x\n    w = z\n    return 1",
    ] {
        let r = result(body);
        assert!(
            r.holes().is_empty(),
            "nothing here reads a value as a number, so there is nothing to \
             hole: {body:?} gave {}",
            describe(&r)
        );
        assert!(
            matches!(r, TripCount::Exact(_)),
            "{body:?} must be exact, got {}",
            describe(&r)
        );
    }
}

/// **The value's own cost survives; only the binding's hole is gone.**
///
/// `z = x.attr` runs a `property` and `z = f(x)` runs `f`. Each keeps its
/// region, and the function has exactly that one region rather than that one
/// plus a `non-integer-value` for `z`.
#[test]
fn a_binding_keeps_the_values_own_cost_and_nothing_else() {
    for (body, construct) in [
        ("    z = x.attr\n    return 1", "attribute"),
        ("    z = f(x)\n    return 1", "call"),
        ("    z = x[0]\n    return 1", "subscript"),
    ] {
        let r = result(body);
        assert_eq!(
            constructs(&r),
            vec![construct],
            "{body:?}: the value's region and only that, got {}",
            describe(&r)
        );
    }
}

/// **A read that needs the value still refuses, and names the variable.**
///
/// A guard, a `range` bound, and an operator over operands this fragment
/// cannot type: the first two would be mis-modelled, the third may run user
/// code (`__add__`, `__neg__`). The fence is exactly where it was.
#[test]
fn a_read_that_needs_the_value_still_refuses() {
    for body in [
        "    if x == y:\n        return 1\n    return 0",
        "    for i in range(x):\n        pass\n    return 1",
        "    z = x + y\n    return 1",
        "    return -x",
        "    z = x\n    for i in range(z):\n        pass\n    return 1",
    ] {
        let r = result(body);
        assert!(
            constructs(&r).contains(&"non-integer-value"),
            "{body:?} reads a value this fragment cannot hold where the value \
             matters, and must still hole: {}",
            describe(&r)
        );
    }
}

/// **An augmented assignment on a non-integer stays a hole.**
///
/// `s += v` runs `__iadd__`; a string or a list concatenation is linear in the
/// operand. Not a binding, and not one step.
#[test]
fn an_augmented_assignment_on_a_non_integer_stays_a_hole() {
    let r = result("    x += y\n    return 1");
    assert!(
        constructs(&r).contains(&"non-integer-value"),
        "`x += y` runs user code and must keep its hole: {}",
        describe(&r)
    );
}

/// **Soundness: rebinding a collection forgets its length.**
///
/// `items = []` is bounded work now, not a hole, so the engine must still be
/// told that `len(items)` after it is not the `len(items)` the caller passed.
/// The loop below must not be counted on the entry length - and the control
/// shows that without the rebinding it is.
#[test]
fn rebinding_a_collection_forgets_its_length() {
    let control =
        result("    total = 0\n    for a in items:\n        total = total + 1\n    return total");
    assert!(
        mentions(&control, "len(items)"),
        "control: the loop over the parameter is counted on its length: {}",
        describe(&control)
    );
    let rebound = result(
        "    items = []\n    total = 0\n    for a in items:\n        total = total + 1\n    return total",
    );
    assert!(
        !mentions(&rebound, "len(items)"),
        "after `items = []` the entry length is not the loop's length: {}",
        describe(&rebound)
    );
    assert!(
        !rebound.holes().is_empty(),
        "the loop over a list this fragment cannot measure must be a hole: {}",
        describe(&rebound)
    );
}

/// **The ITS lowering refuses a declared rebinding rather than keeping the
/// old length.**
///
/// The transition system has no update meaning "anything"; an identity update
/// would carry `len(items)` across `items = []` unchanged. So the native engine
/// composes and the ITS refuses, exactly as it does for a composed sibling.
#[test]
fn the_its_lowering_refuses_a_declared_rebinding() {
    let source = "def g(items: list) -> int:\n    items = []\n    total = 0\n    for a in items:\n        total = total + 1\n    return total\n";
    let function = subject(source);
    assert!(
        landav_its::lower(function.program()).is_err(),
        "an identity update across `items = []` would count the loop on the entry length"
    );
    // And the fence is not the whole function: without the rebinding it lowers.
    let source = "def g(items: list) -> int:\n    total = 0\n    for a in items:\n        total = total + 1\n    return total\n";
    let function = subject(source);
    assert!(
        landav_its::lower(function.program()).is_ok(),
        "control: the counted loop over a parameter lowers"
    );
}
