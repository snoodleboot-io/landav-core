//! `LAN-101`: **a collection parameter that a loop or a `with` rebinds has no
//! length variable; one an assignment rebinds still does.**
//!
//! Two binding forms, two disciplines, and the line between them is what the
//! translated program records. `items = []` is a refused binding carrying
//! `LAN-100`'s write set, so `len(items)` is forgotten exactly where the
//! program changes it and a loop above the assignment is still counted. `for
//! items in rows:` binds `items` to nothing the program can see - the walk
//! counts on a synthetic counter - so the length stayed readable across it and
//! a loop over `items` below counted by the caller's length. That was unsound
//! from `LAN-89` on, and it is closed here by never declaring the length.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_engine::{TripCount, cost};
use landav_python::LoweredFunction;

fn subject(source: &str) -> LoweredFunction {
    let functions = landav_python::lower_module(Path::new("rebound.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    functions
        .into_iter()
        .next_back()
        .unwrap_or_else(|| panic!("expected at least one function in:\n{source}"))
}

fn params(function: &LoweredFunction) -> Vec<String> {
    function
        .program()
        .params()
        .iter()
        .map(|param| param.symbol().as_str().to_owned())
        .collect()
}

fn mentions(result: &TripCount, name: &str) -> bool {
    result
        .bound()
        .is_some_and(|bound| bound.vars().iter().any(|var| var.symbol().as_str() == name))
}

/// **A `for` target or a `with ... as` target, and `len(items)` is not a
/// parameter.** The loop over `items` below is then over something with no
/// known length and is refused - not counted, and not counted wrongly.
#[test]
fn a_parameter_rebound_by_a_target_has_no_length_variable() {
    let sources = [
        "\
def rebound_by_a_loop(items: list, rows: list) -> int:
    for items in rows:
        pass
    count = 0
    for item in items:
        count += 1
    return count
",
        "\
def rebound_by_a_tuple_target(items: list, rows: list) -> int:
    for key, items in rows:
        pass
    count = 0
    for item in items:
        count += 1
    return count
",
        "\
def rebound_by_a_with(items: list, opener) -> int:
    with opener() as items:
        pass
    count = 0
    for item in items:
        count += 1
    return count
",
        "\
def rebound_inside_a_branch(items: list, rows: list, flag) -> int:
    if flag:
        for items in rows:
            pass
    count = 0
    for item in items:
        count += 1
    return count
",
    ];
    for source in sources {
        let function = subject(source);
        let declared = params(&function);
        assert!(
            !declared.iter().any(|name| name == "len(items)"),
            "`items` is rebound by a target the program does not record, so its \
             length on entry is not a value the cost can be a function of. \
             Parameters are {declared:?} for:\n{source}"
        );
        let result = cost(function.program());
        assert!(
            !mentions(&result, "len(items)"),
            "no bound may mention a length that was never declared. Got {:?} \
             for:\n{source}",
            result.bound().map(ToString::to_string)
        );
    }
}

/// **An assignment keeps the length variable, and the loop above it is still
/// counted.** The rebinding is a statement the program records, with
/// `items` and `len(items)` in its write set; the length is forgotten there
/// and not before. Measured: excluding this shape too cost three of the typed
/// corpus's fifty counted loops.
#[test]
fn a_parameter_rebound_by_an_assignment_keeps_its_length_until_then() {
    let source = "\
def rebound_after(items: list) -> int:
    count = 0
    for item in items:
        count += 1
    items = []
    return count
";
    let function = subject(source);
    let declared = params(&function);
    assert!(
        declared.iter().any(|name| name == "len(items)"),
        "the assignment is after the loop and is recorded where it stands. \
         Parameters are {declared:?}"
    );
    let result = cost(function.program());
    assert!(
        mentions(&result, "len(items)"),
        "the loop runs over the caller's list and is counted by its length. Got {:?}",
        result.bound().map(ToString::to_string)
    );
    // And the converse is `refusal_granularity.rs::
    // rebinding_a_collection_parameter_forgets_its_length`: the same
    // assignment *above* the loop forgets the length before it is read.
}

/// **A binding inside a nested `def` is that scope's, not this one's.**
#[test]
fn a_target_in_a_nested_definition_does_not_rebind_the_parameter() {
    let source = "\
def outer(items: list, rows: list) -> int:
    def inner():
        for items in rows:
            pass
    count = 0
    for item in items:
        count += 1
    return count
";
    let declared = params(&subject(source));
    assert!(
        declared.iter().any(|name| name == "len(items)"),
        "the nested `def` binds its own `items`; the parameter is untouched. \
         Parameters are {declared:?}"
    );
}

/// **A body the walk cannot enumerate disqualifies every parameter.** A
/// `match` statement binds names this pass does not read.
#[test]
fn a_body_with_a_match_statement_has_no_collection_parameters() {
    let source = "\
def matched(items: list, command) -> int:
    match command:
        case [items]:
            pass
    count = 0
    for item in items:
        count += 1
    return count
";
    let declared = params(&subject(source));
    assert!(
        !declared.iter().any(|name| name == "len(items)"),
        "a `match` may bind `items` in a way the walk does not read. Parameters \
         are {declared:?}"
    );
}
