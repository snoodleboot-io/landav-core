//! `LAN-118`: **a leaf a region cannot use is not a hole of its own.**
//!
//! `LAN-117` made a bare name or a non-integer constant at the *root* of a
//! discarded value one step. This is the rest of the shape: the value stored
//! through a target the fragment cannot bind (`self.x = y`), the value of an
//! annotated binding to a non-integer name (`x: str = y`), what a `raise`
//! raises, and a leaf inside a display, an f-string field or a ternary arm
//! whose enclosing form is already the region. In every case the enclosing
//! store, display or statement is the consumer, and the leaf is a step it
//! already pays. What refuses, and must: an operand of an operator or a
//! comparison, a truth test, and any leaf with an interior.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_engine::{TripCount, cost};
use landav_python::LoweredFunction;

fn subject(source: &str) -> LoweredFunction {
    let functions = landav_python::lower_module(Path::new("nested.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    functions
        .into_iter()
        .next_back()
        .unwrap_or_else(|| panic!("expected a function in:\n{source}"))
}

fn result(body: &str) -> TripCount {
    let source = format!("def g(self, x, y, n: int) -> int:\n{body}\n");
    cost(subject(&source).program())
}

fn describe(result: &TripCount) -> String {
    let bound = result
        .bound()
        .map_or_else(|| "-".to_owned(), ToString::to_string);
    format!("{bound} holes={:?}", constructs(result))
}

fn constructs(result: &TripCount) -> Vec<&str> {
    result.holes().iter().map(|hole| hole.construct()).collect()
}

/// **A value stored through a target the fragment cannot bind is read by
/// nothing: the store is the one region.**
#[test]
fn a_store_through_an_unbindable_target_is_one_region() {
    for body in [
        "    self.x = y\n    return 1",
        "    a, b = y\n    return 1",
        "    a = b = y\n    return 1",
        "    self.x += y\n    return 1",
    ] {
        let r = result(body);
        assert_eq!(
            constructs(&r),
            vec!["complex-assignment-target"],
            "{body:?}: the store and only the store, got {}",
            describe(&r)
        );
    }
}

/// **An annotated binding to a non-integer name is bounded work, like an
/// unannotated one since `LAN-117`.**
#[test]
fn an_annotated_binding_to_a_non_integer_name_is_a_step() {
    for body in [
        "    z: str = y\n    return 1",
        "    z: object = None\n    return 1",
    ] {
        let r = result(body);
        assert!(r.holes().is_empty(), "{body:?}: {}", describe(&r));
    }
}

/// **A display, an f-string or a ternary arm over leaves is free in a
/// discarded position; what is raised is a step.**
#[test]
fn leaves_under_a_free_container_are_not_holes() {
    for body in [
        "    return (x, y)",
        "    return [x, None]",
        "    return {\"k\": x, \"n\": n}",
        "    return f\"{x} and {y}\"",
        "    return x if n > 0 else y",
        "    raise x",
    ] {
        let r = result(body);
        assert!(
            r.holes().is_empty(),
            "{body:?}: the container or statement is the consumer and the \
             leaves cost it nothing extra, got {}",
            describe(&r)
        );
    }
}

/// **A leaf with an interior keeps its own region, and only that one.**
#[test]
fn an_element_with_an_interior_is_still_charged() {
    for (body, construct) in [
        ("    return (f(x), y)", "call"),
        ("    return f\"{x.kind}\"", "attribute"),
        ("    raise f(x)", "call"),
        ("    self.x = f(y)", "call"),
    ] {
        let r = result(body);
        assert!(
            constructs(&r).contains(&construct),
            "{body:?}: the interior's region must survive, got {}",
            describe(&r)
        );
        assert!(
            !constructs(&r).contains(&"non-integer-value"),
            "{body:?}: and the leaves beside it are not holes, got {}",
            describe(&r)
        );
    }
}

/// **An operand is not a leaf the container can absorb.**
///
/// `x + y` may run `__add__`, `x == y` may run `__eq__`, `not x` and `x or y`
/// run `__bool__`: each reads the value, each may be user code, and each keeps
/// refusing by name - inside a display or a ternary arm as much as at the root.
#[test]
fn an_operand_still_refuses_inside_a_free_container() {
    for body in [
        "    return (x + y,)",
        "    return [x == y]",
        "    return f\"{-x}\"",
        "    return (not x) if n > 0 else y",
        "    return x or y",
        "    return (x, y) if x else y",
    ] {
        let r = result(body);
        assert!(
            constructs(&r).contains(&"non-integer-value"),
            "{body:?}: an operand over a value this fragment cannot hold must \
             still refuse, got {}",
            describe(&r)
        );
    }
}
