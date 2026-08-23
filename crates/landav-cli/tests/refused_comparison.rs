//! `LAN-90`: **a refused comparison costs the comparison, not the function.**
//!
//! # The defect
//!
//! `Translator::comparison` translated both operands and *then* discovered the
//! operator was one it refuses - `is`, `is not`, `in`, `not in`. It returned a
//! fresh unsupported condition that referenced neither operand, so an operand
//! that had itself refused (`None` is not an integer; `(1, 2)` is a collection)
//! stayed in the arena with nothing pointing at it.
//!
//! `Walk::reconciled` finds an `Unsupported` node the walk never charged and
//! answers `Unknown` for the whole function. That is the engine behaving
//! **correctly** - it fails closed rather than publishing a partial that is
//! silently missing a region - so the bug is entirely on the frontend side, and
//! the symptom is a function that reports no bound at all because one `if`
//! used the wrong operator.
//!
//! This is the same leak `LAN-87` closed for `Stmt::Return`, `assign`,
//! `aug_assign` and `ann_assign`, in the one path that sweep did not reach.
//!
//! # Why this is tested from the frontend and not in `hole_totality.rs`
//!
//! The engine suite works at the `SourceProgram` contract, where an orphan can
//! be constructed deliberately - and it does, in
//! `an_unsupported_node_with_no_parent_never_yields_a_complete_bound`. What it
//! cannot show is that the **Python translator** produces one, because that
//! takes real Python going through real `comparison()`. A hand-built program
//! would never have the shape.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_engine::{TripCount, cost};
use landav_python::LoweredFunction;

fn only_function(source: &str) -> LoweredFunction {
    let mut functions = landav_python::lower_module(Path::new("comparisons.py"), source)
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
    format!("{kind}({bound})")
}

/// A refused comparison must leave the rest of the function derivable.
fn assert_still_derivable(source: &str) {
    let function = only_function(source);
    let result = cost(function.program());
    assert!(
        !matches!(result, TripCount::Unknown),
        "a comparison this fragment refuses must cost a region, not the whole \
         function's bound - `Unknown` here means an operand was translated and \
         then orphaned, and `Walk::reconciled` failed closed on it. Got {} \
         for:\n{source}",
        describe(&result),
    );
}

/// **`is not None` does not erase the function.**
///
/// The shape that motivated the ticket: it appears in real code as a null
/// check, usually beside the loop whose cost is the interesting part.
#[test]
fn an_identity_comparison_costs_a_region_not_the_function() {
    assert_still_derivable(
        "\
def is_not_none(n: int) -> int:
    if n is not None:
        return 1
    return 0
",
    );
}

/// **`is None` likewise.**
#[test]
fn the_other_identity_comparison_behaves_the_same() {
    assert_still_derivable(
        "\
def is_none(n: int) -> int:
    if n is None:
        return 1
    return 0
",
    );
}

/// **`in` over a literal container likewise.**
///
/// Here the orphan is the tuple rather than `None`, so it exercises a different
/// refusing arm of `build_expression` reaching the same leak.
#[test]
fn a_membership_comparison_costs_a_region_not_the_function() {
    assert_still_derivable(
        "\
def in_op(n: int) -> int:
    if n in (1, 2):
        return 1
    return 0
",
    );
}

/// **`not in` likewise.**
#[test]
fn a_negated_membership_comparison_behaves_the_same() {
    assert_still_derivable(
        "\
def not_in_op(n: int) -> int:
    if n not in (1, 2):
        return 1
    return 0
",
    );
}

/// **The comparison is still refused, and still named.**
///
/// The fix must not turn a refusal into an acceptance. `is not` has no model
/// here and the region has to stay - a bound that quietly assumed the branch
/// was free would be the opposite failure.
#[test]
fn the_refused_comparison_is_still_blamed() {
    let function = only_function(
        "\
def is_not_none(n: int) -> int:
    if n is not None:
        return 1
    return 0
",
    );
    let result = cost(function.program());
    assert!(
        !result.is_complete(),
        "`is not` is not something this fragment can evaluate, so the branch it \
         guards must stay a named region: {}",
        describe(&result)
    );
    assert!(
        !result.holes().is_empty(),
        "the refusal must be carried as a hole that names it, got {}",
        describe(&result)
    );
}

/// **A supported comparison whose operand refuses is unaffected.**
///
/// The operand of a comparison the fragment *does* understand is referenced by
/// the `compare` node, so it is reachable and gets charged. This is the case
/// the fix must leave exactly as it was, and it is the reason the fix checks
/// the operator rather than declining to translate operands generally.
#[test]
fn a_supported_comparison_still_carries_its_refused_operand() {
    let function = only_function(
        "\
def compares(n: int) -> int:
    if n < len(other):
        return 1
    return 0
",
    );
    let result = cost(function.program());
    assert!(
        !matches!(result, TripCount::Unknown),
        "`<` is supported and its operands are referenced by the comparison, so \
         a refusal inside one is charged where it stands: {}",
        describe(&result)
    );
    assert!(
        !result.holes().is_empty(),
        "the call in the operand must still be blamed, got {}",
        describe(&result)
    );
}

/// **A chained comparison is still translated.**
///
/// `1 < n < 5` expands to two links and neither operator is refused, so the fix
/// must not short-circuit it.
#[test]
fn a_chained_comparison_is_unaffected() {
    let function = only_function(
        "\
def chained(n: int) -> int:
    if 1 < n < 5:
        return 1
    return 0
",
    );
    let result = cost(function.program());
    assert!(
        result.is_complete(),
        "a chained comparison of integers is fully understood and must stay so: {}",
        describe(&result)
    );
}
