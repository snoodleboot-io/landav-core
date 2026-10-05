//! `LAN-114`: **a declared call over an attribute or subscript argument stops
//! erasing the frame, and the argument's cost stays named.**
//!
//! `isinstance(x, self.klass)` has a row — `isinstance` is a constant type test
//! — but until this change the `self.klass` argument was neither free nor a
//! call, so the whole call stayed a hole. A hole erases the frame (`LAN-100`),
//! so a loop *below* the type test lost its trip count. That is 148 stdlib
//! functions for the attribute shape and 31 for the subscript shape, the
//! widening `lowering.rs`'s `expression_children` named and deferred.
//!
//! The fix charges `self.klass` as its own `attribute` region — a hole denoting
//! the `property` the enclosing call's `omega` denoted before, so never an
//! under-approximation — and declares the call. The loop below then keeps its
//! count (`LAN-103`), and the bound is partial by that one region rather than
//! refused whole.
//!
//! Scope is attribute and subscript, the two the deferred note measured. A
//! display or a format string descends into its elements, so declaring a call
//! over `(int, self.kind)` or `f"{t}"` would value-read the bare names among
//! them; those shapes are a separate change and stay holed here.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_engine::{TripCount, cost};
use landav_python::LoweredFunction;

/// The last function `source` lowers to.
fn subject(source: &str) -> LoweredFunction {
    let functions = landav_python::lower_module(Path::new("structured.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    functions
        .into_iter()
        .next_back()
        .unwrap_or_else(|| panic!("expected a function in:\n{source}"))
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

fn holes_on(result: &TripCount, construct: &str) -> bool {
    result
        .holes()
        .iter()
        .any(|hole| hole.construct() == construct)
}

/// Whether the `for` below the call kept its trip count — the whole point.
fn loop_is_counted(result: &TripCount) -> bool {
    !holes_on(result, "unbounded-iteration") && !holes_on(result, "for")
}

/// A type test over `argument`, then a counted loop of `n`.
fn test_then_loop(argument: &str) -> String {
    format!(
        "def g(x, node, table, n: int) -> int:\n    \
         isinstance(x, {argument})\n    \
         total = 0\n    \
         for i in range(n):\n        \
         total = total + 1\n    \
         return total\n"
    )
}

/// **An attribute argument: the loop counts, the attribute is still charged.**
#[test]
fn an_attribute_argument_declares_the_call_and_keeps_the_loop() {
    let source = test_then_loop("node.kind");
    let function = subject(&source);
    let result = cost(function.program());

    assert!(
        loop_is_counted(&result),
        "the loop below the type test lost its count, so the call still erased \
         the frame: {}",
        describe(&result)
    );
    assert!(
        holes_on(&result, "attribute"),
        "declaring the call must not drop the `property` cost — it has to \
         reappear as an `attribute` region: {}",
        describe(&result)
    );
    assert!(
        !holes_on(&result, "call"),
        "the call is declared now, so nothing should blame a `call`: {}",
        describe(&result)
    );
}

/// **A subscript argument, the same.**
#[test]
fn a_subscript_argument_declares_the_call_and_keeps_the_loop() {
    let source = test_then_loop("table[0]");
    let function = subject(&source);
    let result = cost(function.program());

    assert!(loop_is_counted(&result), "{}", describe(&result));
    assert!(
        holes_on(&result, "subscript"),
        "the `__getitem__` cost must survive as a `subscript` region: {}",
        describe(&result)
    );
    assert!(!holes_on(&result, "call"), "{}", describe(&result));
}

/// **A nested attribute is one region, not one per dot.**
#[test]
fn a_nested_attribute_argument_is_a_single_region() {
    let source = test_then_loop("node.inner.kind");
    let function = subject(&source);
    let result = cost(function.program());

    assert!(loop_is_counted(&result), "{}", describe(&result));
    let attributes = result
        .holes()
        .iter()
        .filter(|hole| hole.construct() == "attribute")
        .count();
    assert_eq!(
        attributes,
        1,
        "`node.inner.kind` is one attribute region, not one per dot: {}",
        describe(&result)
    );
}

/// **The guard: a call argument is not made placeable.**
///
/// `f()` is a cost the fragment cannot bound. It is named as its own `call`
/// region where it stands — that part is `LAN-94` — but the enclosing
/// `isinstance` is *not* turned constant over it: an argument whose own cost is
/// unknown must keep the frame-erasing hole, or the loop below would be counted
/// under a bound that omits `f`.
#[test]
fn a_call_argument_still_erases_the_frame() {
    let source = test_then_loop("f()");
    let function = subject(&source);
    let result = cost(function.program());

    assert!(
        holes_on(&result, "call"),
        "the unknown call `f` must still be a hole: {}",
        describe(&result)
    );
    assert!(
        !loop_is_counted(&result),
        "with an unknown call in the argument the frame is not safe to keep, so \
         the loop below must not be counted: {}",
        describe(&result)
    );
}

/// **The guard: a comprehension argument is not made placeable either.**
#[test]
fn a_comprehension_argument_keeps_the_call_a_hole() {
    let source = test_then_loop("[k for k in table]");
    let function = subject(&source);
    let result = cost(function.program());

    assert!(
        holes_on(&result, "call") || !loop_is_counted(&result),
        "a comprehension runs a loop the type test's `omega` was covering; the \
         call must stay a hole: {}",
        describe(&result)
    );
}

/// **Soundness: the value never drops.**
///
/// The attribute region denotes `omega`, exactly as the whole call did before,
/// so the bound at any point is no smaller than it was. What changed is that
/// the loop's `n` is now *added* to that `omega` rather than swallowed by it.
#[test]
fn the_declared_call_never_under_reports() {
    let source = test_then_loop("node.kind");
    let function = subject(&source);
    let result = cost(function.program());
    // A partial bound carrying an `omega` hole is the sound answer; what must
    // not happen is a finite complete bound that dropped the attribute cost.
    assert!(
        matches!(result, TripCount::Partial { .. }),
        "the attribute cost is unknown, so the bound must stay partial, never \
         complete with the cost missing: {}",
        describe(&result)
    );
    assert!(
        result
            .bound()
            .is_some_and(|bound| { bound.vars().iter().any(|var| var.symbol().as_str() == "n") }),
        "the loop variable must appear in the bound — that is the gain: {}",
        describe(&result)
    );
}
