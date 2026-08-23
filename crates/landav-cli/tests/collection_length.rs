//! `LAN-89` acceptance: **a bound that grows with the input.**
//!
//! # The measurement this exists to move
//!
//! Across the Python 3.12 stdlib, not one of the 2323 bounds `landav-engine`
//! derives mentions a parameter. All 14 exact bounds are the constant `0` or
//! `1`, and the non-hole part of every partial is a constant - so filling every
//! hole in the corpus would still leave every bound constant. A tool whose
//! purpose is to say how cost scales with input had never once said it.
//!
//! The cause is narrow. The engine counts a loop whose endpoint is an integer
//! parameter exactly, and Python's scale-bearing loops do not have one: they
//! walk a collection, and `len(items)` was refused as a non-integer value while
//! `for x in items` was refused as unbounded iteration. Both refusals were
//! correct given the vocabulary. The vocabulary was what was missing.
//!
//! # What a collection parameter now carries
//!
//! One norm: its **outer length**, as an ordinary parameter of the derived
//! bound, spelled `len(items)`. Python has no identifier containing parentheses,
//! so that name cannot collide with anything the source could have written.
//!
//! Deliberately one norm. Inner lengths, total element counts and the relations
//! between them are `LAN-11`, and a bound over the wrong norm is worse than no
//! bound.
//!
//! # Why these tests read the libraries rather than driving the binary
//!
//! The assertions are about the *shape* of a bound - which variables it
//! mentions, whether the loop left a hole - and the process boundary offers
//! only the rendered string, which these tests are forbidden to pin.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, path::Path};

use landav_bound::{Nat, Symbol, Valuation, VarId};
use landav_engine::{TripCount, cost};
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

/// Translates `source` and returns its single function.
fn only_function(source: &str) -> LoweredFunction {
    let mut functions = landav_python::lower_module(Path::new("collections.py"), source)
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

/// The names a bound mentions.
fn mentioned(result: &TripCount) -> Vec<String> {
    result.bound().map_or_else(Vec::new, |bound| {
        bound
            .vars()
            .iter()
            .map(|var| var.symbol().as_str().to_owned())
            .collect()
    })
}

/// **The** assertion this ticket exists for: the bound is a function of the
/// collection's length, and the loop over it left no hole.
///
/// One helper rather than three assertions per test, because a bound that
/// forgot the length, a bound that holed the loop, and a bound that mentioned
/// something else entirely are three different failures with one symptom.
fn assert_scales_with(result: &TripCount, name: &str, source: &str) {
    let shown = describe(result);
    let names = mentioned(result);
    assert!(
        names.iter().any(|var| var == name),
        "the bound must be a function of `{name}` - that is the whole point of \
         the ticket - but it mentions {names:?}. Got {shown} for:\n{source}"
    );
    assert!(
        result.holes().is_empty(),
        "the loop over the collection must be counted, not holed: a partial \
         bound makes no finite claim and is not a scale answer. Got {shown} \
         for:\n{source}"
    );
}

/// Every name a bound mentions must be something the caller can supply.
///
/// A hole variable is exempt only while the result is **partial**: an unfilled
/// hole denotes `omega`, so a partial makes no finite claim. A *complete* result
/// carrying one would be a finite-looking claim with an infinite term in it,
/// which every caller supplying the parameters and nothing else reads as zero.
fn assert_only_supplied(function: &LoweredFunction, result: &TripCount, source: &str) {
    let params: Vec<String> = function
        .program()
        .params()
        .iter()
        .map(|param| param.symbol().as_str().to_owned())
        .collect();
    let partial = !result.is_complete();
    for name in mentioned(result) {
        assert!(
            params.contains(&name) || (partial && name.starts_with("#hole")),
            "the bound mentions `{name}`, which is not a parameter of the \
             function - the caller has nothing to supply for it, so \
             `Bound::eval` reads it as zero. Parameters are {params:?}, got {} \
             for:\n{source}",
            describe(result),
        );
    }
}

// ---------------------------------------------------------------------------
// the two idioms
// ---------------------------------------------------------------------------

/// **`for x in items` is counted by the collection's length.**
///
/// The dominant scale-bearing idiom in Python, and the one that was refused as
/// `unbounded-iteration`.
#[test]
fn iterating_a_collection_parameter_scales_with_its_length() {
    let source = "\
def walk(items: list) -> int:
    total = 0
    for x in items:
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_scales_with(&result, "len(items)", source);
    assert_only_supplied(&function, &result, source);
}

/// **`for i in range(len(items))` is counted by the same length.**
///
/// The other idiom. It reaches the loop through `range`, so it exercises
/// `len` as an expression rather than as an iterable.
#[test]
fn ranging_over_a_length_scales_with_it() {
    let source = "\
def indexed(items: list) -> int:
    total = 0
    for i in range(len(items)):
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_scales_with(&result, "len(items)", source);
    assert_only_supplied(&function, &result, source);
}

/// **The two idioms agree on the number.**
///
/// They are the same loop written two ways, so a difference between them is a
/// bug in one of them, and comparing the numbers catches it where comparing
/// shapes would not.
#[test]
fn the_two_idioms_derive_the_same_cost() {
    let walking = only_function(
        "\
def walk(items: list) -> int:
    total = 0
    for x in items:
        total = total + 1
    return total
",
    );
    let indexing = only_function(
        "\
def indexed(items: list) -> int:
    total = 0
    for i in range(len(items)):
        total = total + 1
    return total
",
    );

    let table = bindings(&[("len(items)", 7)]);
    let walked = cost(walking.program()).bound().map(|b| b.eval(&table));
    let ranged = cost(indexing.program()).bound().map(|b| b.eval(&table));

    assert_eq!(
        walked, ranged,
        "`for x in items` and `for i in range(len(items))` run the same number \
         of times and must cost the same"
    );
}

/// **A nested walk is quadratic in the length.**
///
/// The shape the tool exists to find. A bound that stayed linear here would be
/// exceeded by the program, which is the direction that must never happen.
#[test]
fn a_nested_walk_is_quadratic_in_the_length() {
    let source = "\
def pairs(items: list) -> int:
    total = 0
    for x in items:
        for y in items:
            total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_scales_with(&result, "len(items)", source);
    assert_only_supplied(&function, &result, source);

    let bound = result.bound().expect("a counted nest carries a bound");
    // Doubling the length must roughly quadruple the cost. Stated as a ratio
    // rather than a literal so it survives a change to what a statement costs.
    let at_10 = bound.eval(&bindings(&[("len(items)", 10)]));
    let at_20 = bound.eval(&bindings(&[("len(items)", 20)]));
    let (Nat::Fin(small), Nat::Fin(large)) = (at_10, at_20) else {
        panic!("a counted nest must evaluate finitely, got {at_10:?} {at_20:?}");
    };
    assert!(
        large >= small * 3,
        "doubling the collection must more than double the cost of a nested \
         walk - {small} at 10 and {large} at 20 is not quadratic growth"
    );
}

/// **An integer parameter and a collection parameter compose.**
#[test]
fn a_length_and_an_integer_parameter_multiply() {
    let source = "\
def grid(items: list, n: int) -> int:
    total = 0
    for x in items:
        for j in range(n):
            total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_scales_with(&result, "len(items)", source);
    let names = mentioned(&result);
    assert!(
        names.iter().any(|var| var == "n"),
        "the bound must mention the integer parameter too, got {names:?} for:\n{source}"
    );
    assert_only_supplied(&function, &result, source);
}

// ---------------------------------------------------------------------------
// the soundness fence - LAN-88's discipline, applied to the new variable
// ---------------------------------------------------------------------------

/// **A rebound collection does not report a stale length.**
///
/// The exact shape of `LAN-88`, in the new vocabulary: `len(items)` denotes the
/// length of what the *caller* passed, and after `items = other` it is no
/// longer the length of what the loop walks. A bound still written over
/// `len(items)` there would be exceeded by the program.
#[test]
fn a_rebound_collection_does_not_reuse_the_callers_length() {
    let source = "\
def rebound(items: list, other: list) -> int:
    total = 0
    items = other
    for x in items:
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let shown = describe(&result);

    // Either it holes the loop or it counts it over `len(other)`. What it may
    // not do is keep claiming `len(items)`, which is now the length of
    // something the loop does not walk.
    if result.is_complete() {
        let names = mentioned(&result);
        assert!(
            !names.iter().any(|var| var == "len(items)"),
            "after `items = other` the loop walks `other`, so a complete bound \
             over `len(items)` is a claim about a collection the loop never \
             touches: {shown} for:\n{source}"
        );
    }
    assert_only_supplied(&function, &result, source);
}

/// **A collection the analysis cannot see into does not acquire a length.**
///
/// `len` of a local, a call result or an attribute is not a value the caller
/// supplies, so it may not become a bound variable. The loop becomes a region
/// instead - which is the honest answer, not a regression.
#[test]
fn a_length_of_something_that_is_not_a_parameter_is_not_a_bound_variable() {
    let source = "\
def local(n: int) -> int:
    made = build(n)
    total = 0
    for x in made:
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        !result.is_complete(),
        "`made` is a local built by a call this analysis cannot read, so its \
         length is not something the caller supplies and the loop over it \
         cannot be counted: {shown} for:\n{source}"
    );
    assert_only_supplied(&function, &result, source);
}

/// **A mutated collection does not report the length it had on entry.**
///
/// `items.append(...)` changes the length the loop will walk. The call is
/// already a region, and a region may change anything - so the length must stop
/// being readable across it, exactly as an integer does.
#[test]
fn appending_to_a_collection_forgets_its_entry_length() {
    let source = "\
def grows(items: list) -> int:
    total = 0
    items.append(1)
    for x in items:
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        !result.is_complete(),
        "`items.append(1)` changes what the loop walks, so the length the \
         caller supplied no longer counts it: {shown} for:\n{source}"
    );
    assert_only_supplied(&function, &result, source);
}

/// **An unannotated parameter acquires no length.**
///
/// The frontend trusts annotations and cannot check them, so it may only trust
/// the ones that are there. A bare `def f(items)` says nothing about what
/// `items` is.
#[test]
fn an_unannotated_parameter_is_not_a_collection() {
    let source = "\
def bare(items) -> int:
    total = 0
    for x in items:
        total = total + 1
    return total
";
    let function = only_function(source);

    let params: Vec<String> = function
        .program()
        .params()
        .iter()
        .map(|param| param.symbol().as_str().to_owned())
        .collect();
    assert!(
        !params.iter().any(|param| param == "len(items)"),
        "an unannotated parameter must not acquire a length, got {params:?} \
         for:\n{source}"
    );
    assert!(
        !cost(function.program()).is_complete(),
        "a loop over an unannotated parameter cannot be counted for:\n{source}"
    );
}

/// **The element of a walk is not an integer.**
///
/// `for x in items` binds `x` to an element, and an element of a `list` is not
/// something this fragment may do arithmetic on. Reading it has to refuse, or a
/// bound could be derived over a value that is not a number.
#[test]
fn the_element_of_a_walk_is_not_readable_as_an_integer() {
    let source = "\
def sums(items: list) -> int:
    total = 0
    for x in items:
        total = total + x
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let shown = describe(&result);
    let names = mentioned(&result);

    assert!(
        !names.iter().any(|var| var == "x"),
        "`x` is an element of the collection, not an integer the caller \
         supplied - a bound mentioning it would be arithmetic on a value that \
         may be a string: {shown} for:\n{source}"
    );
    assert_only_supplied(&function, &result, source);
}
