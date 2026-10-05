//! `LAN-115`: **a format string over names and constants is as free as a string
//! literal, so the constructor over it completes.**
//!
//! `raise ValueError("bad")` already derives a complete bound — a string literal
//! costs nothing, so the constructor declares. `raise ValueError(f"expected
//! {t}")` did not: the f-string was a `Collection` hole, so the call stayed a
//! hole and the function was partial. But building that string is bounded by the
//! format's own length, a constant of the program — the same cost as the
//! literal. This pins that such a format is now free, while a format that
//! interpolates real work keeps its cover.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_engine::{TripCount, cost};

fn result(body: &str) -> TripCount {
    let source = format!("def g({}) -> int:\n    {body}\n", "a, b, t, obj, n: int");
    let functions = landav_python::lower_module(Path::new("fmt.py"), &source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    let function = functions
        .into_iter()
        .find(|it| it.name() == "g")
        .expect("function g");
    cost(function.program())
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

fn is_complete(result: &TripCount) -> bool {
    result.holes().is_empty()
}

fn holes_on(result: &TripCount, construct: &str) -> bool {
    result
        .holes()
        .iter()
        .any(|hole| hole.construct() == construct)
}

/// **An f-string over a name completes the constructor.**
#[test]
fn an_f_string_over_a_name_is_free() {
    let r = result("raise ValueError(f\"expected {t}\")");
    assert!(
        is_complete(&r),
        "the f-string is constant work, so the constructor must declare and the \
         function complete: {}",
        describe(&r)
    );
}

/// **A `%` format over a name, and over a tuple of names, completes.**
#[test]
fn a_percent_format_over_names_is_free() {
    for body in [
        "raise TypeError(\"bad %s\" % t)",
        "raise TypeError(\"%s %s\" % (a, b))",
    ] {
        let r = result(body);
        assert!(is_complete(&r), "{body}: {}", describe(&r));
    }
}

/// **The string literal it is modelled on still completes** — no regression.
#[test]
fn a_string_literal_argument_still_completes() {
    let r = result("raise ValueError(\"bad\")");
    assert!(is_complete(&r), "{}", describe(&r));
}

/// **A call inside the format keeps its cost named.**
///
/// `f"{expensive()}"` is not constant, so the format does not free the
/// constructor outright; the interpolated call is charged as its own region and
/// the bound is partial by exactly that — never a complete bound that dropped
/// it.
#[test]
fn a_call_interpolation_is_charged_not_swallowed() {
    let r = result("raise ValueError(f\"got {expensive()}\")");
    assert!(
        holes_on(&r, "call"),
        "the interpolated call's cost must survive as a region: {}",
        describe(&r)
    );
}

/// **Arithmetic and attribute interpolations keep the hole.**
///
/// `f"{a + b}"` reads a value, `f"{obj.kind}"` runs a `property`; neither is
/// free, so the conservative answer is to keep the constructor's hole over it
/// rather than silently treat it as constant.
#[test]
fn a_value_reading_interpolation_keeps_the_hole() {
    for body in [
        "raise ValueError(f\"got {a + b}\")",
        "raise ValueError(f\"got {obj.kind}\")",
    ] {
        let r = result(body);
        assert!(
            !is_complete(&r),
            "{body}: a value-reading interpolation must not be freed: {}",
            describe(&r)
        );
    }
}

/// **Integer modulo is untouched.**
///
/// `a % b` is `integer-division`, distinguished from `%` formatting by a
/// non-string left operand. The format change must not reach it.
#[test]
fn integer_modulo_is_unchanged() {
    let r = result("return a % b");
    assert!(
        holes_on(&r, "integer-division"),
        "`a % b` must stay an `integer-division` hole, not be read as a format: {}",
        describe(&r)
    );
}
