//! `LAN-106`, first half: **`typing.List[str]` is a list.**
//!
//! `annotation_is_collection` matched only the lowercase builtins, because it
//! was written against PEP 585's spelling. `typing.List`, `Dict`, `Set`,
//! `FrozenSet`, `Tuple` and `Text` are those same builtins under the older
//! name, so a parameter annotated with one had no length variable, and a loop
//! over it was not counted. Measured on the typed corpus: 28 loops, 8 of which
//! survive to a bound.
//!
//! # Admitting them adds no trust, and that is the whole scope
//!
//! The frontend already trusts `list[str]` to be a list. Trusting `List[str]`
//! the same way is not a new claim. The abstract types - `Sequence`,
//! `Collection`, `Mapping` - *are* a new claim, because their length comes from
//! a user `__len__`, and they stay out; that is the ticket's second half and a
//! decision rather than a gap.
//!
//! # The one soundness condition
//!
//! The name has to be `typing`'s where the annotation reads it. `class List:`
//! in a domain model is an ordinary thing to write, and it is not a list. So a
//! name is admitted only if every binding of it in the module is the `typing`
//! import - and, for a method, only if the class body does not bind it too,
//! because a parameter annotation is evaluated in the class scope.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_engine::cost;
use landav_python::LoweredFunction;

fn functions(source: &str) -> Vec<LoweredFunction> {
    landav_python::lower_module(Path::new("typing_aliases.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"))
}

fn named<'a>(functions: &'a [LoweredFunction], name: &str) -> &'a LoweredFunction {
    functions
        .iter()
        .find(|function| function.name() == name)
        .unwrap_or_else(|| panic!("no function named `{name}`"))
}

fn has_length(function: &LoweredFunction, parameter: &str) -> bool {
    let length = format!("len({parameter})");
    function
        .program()
        .params()
        .iter()
        .any(|param| param.symbol().as_str() == length)
}

/// The loop over `items` is counted by its length.
fn assert_counted(source: &str, name: &str) {
    let functions = functions(source);
    let function = named(&functions, name);
    assert!(
        has_length(function, "items"),
        "`items` is a list under `typing`'s spelling, so it has a length the \
         caller supplies. Parameters are {:?} for:\n{source}",
        function.program().params()
    );
    let result = cost(function.program());
    assert!(
        result.bound().is_some_and(|bound| bound
            .vars()
            .iter()
            .any(|var| var.symbol().as_str() == "len(items)")),
        "the loop over `items` must be counted by `len(items)`. Got {:?} for:\n{source}",
        result.bound().map(ToString::to_string)
    );
}

/// `items` has no length variable: the annotation is not a sized builtin here.
fn assert_not_a_collection(source: &str, name: &str, because: &str) {
    let functions = functions(source);
    let function = named(&functions, name);
    assert!(
        !has_length(function, "items"),
        "`items` must not be a sized collection: {because}. Parameters are {:?} \
         for:\n{source}",
        function.program().params()
    );
}

/// A function body that loops over `items`. Written with explicit `\n` rather
/// than a trailing-backslash continuation, because a `\` at the end of a line in
/// a Rust string literal also discards the next line's leading whitespace - and
/// the indentation is the body.
const LOOP: &str = "    total = 0\n    for item in items:\n        total += 1\n    return total\n";

// ---------------------------------------------------------------------------
// 1 · every spelling of the same builtin
// ---------------------------------------------------------------------------

/// **`from typing import List`, the ordinary spelling.**
#[test]
fn a_typing_list_is_a_list() {
    assert_counted(
        &format!("from typing import List\n\n\ndef f(items: List[str]) -> int:\n{LOOP}"),
        "f",
    );
}

/// **Every alias of a sized builtin, subscripted and bare.**
#[test]
fn every_sized_alias_is_admitted() {
    for (alias, annotation) in [
        ("List", "List[int]"),
        ("Dict", "Dict[str, int]"),
        ("Set", "Set[int]"),
        ("FrozenSet", "FrozenSet[int]"),
        ("Tuple", "Tuple[int, ...]"),
        ("Text", "Text"),
        ("List", "List"),
    ] {
        assert_counted(
            &format!("from typing import {alias}\n\n\ndef f(items: {annotation}) -> int:\n{LOOP}"),
            "f",
        );
    }
}

/// **`typing.List`, `t.Dict`, and `from typing import List as L`.**
#[test]
fn the_module_and_renamed_spellings_are_admitted() {
    assert_counted(
        &format!("import typing\n\n\ndef f(items: typing.List[str]) -> int:\n{LOOP}"),
        "f",
    );
    assert_counted(
        &format!("import typing as t\n\n\ndef f(items: t.Dict[str, int]) -> int:\n{LOOP}"),
        "f",
    );
    assert_counted(
        &format!("from typing import List as L\n\n\ndef f(items: L[str]) -> int:\n{LOOP}"),
        "f",
    );
}

/// **A method is admitted the same way.** `LAN-104` put methods in the
/// denominator, and a parameter annotation is the same annotation in either.
#[test]
fn a_method_parameter_is_admitted() {
    assert_counted(
        &format!(
            "from typing import List\n\n\nclass C:\n    def m(self, items: List[str]) -> int:\n{}",
            LOOP.lines()
                .map(|line| format!("    {line}\n"))
                .collect::<String>()
        ),
        "C.m",
    );
}

// ---------------------------------------------------------------------------
// 2 · the name is not `typing`'s where the annotation reads it
// ---------------------------------------------------------------------------

/// **A module's own `List` class is not a list.** The case that makes the
/// soundness condition necessary: a domain model with a class named `List`.
#[test]
fn a_module_class_named_list_is_not_admitted() {
    assert_not_a_collection(
        &format!("class List:\n    pass\n\n\ndef f(items: List) -> int:\n{LOOP}"),
        "f",
        "`List` is a class this module defines",
    );
}

/// **A `typing` import later rebound is not admitted.** By the time the
/// annotation reads `List` it is `MyList`.
#[test]
fn a_rebound_typing_import_is_not_admitted() {
    assert_not_a_collection(
        &format!(
            "from typing import List\n\nList = MyList\n\n\ndef f(items: List[str]) -> int:\n{LOOP}"
        ),
        "f",
        "`List` is rebound after the import",
    );
    assert_not_a_collection(
        &format!(
            "from typing import List\nfrom mymodels import List\n\n\ndef f(items: List[str]) -> int:\n{LOOP}"
        ),
        "f",
        "a second import binds `List` to something that is not `typing`'s",
    );
}

/// **`from .typing import List` is this package's module, not the stdlib's.**
#[test]
fn a_relative_typing_import_is_not_admitted() {
    assert_not_a_collection(
        &format!("from .typing import List\n\n\ndef f(items: List[str]) -> int:\n{LOOP}"),
        "f",
        "a relative import names a module of this package that is called `typing`",
    );
}

/// **A bare `List` that was never imported is not admitted.** Python would
/// fail to evaluate it at all; there is nothing to trust.
#[test]
fn an_unimported_list_is_not_admitted() {
    assert_not_a_collection(
        &format!("def f(items: List[str]) -> int:\n{LOOP}"),
        "f",
        "`List` is not bound by anything",
    );
}

/// **A class body that binds `List` changes a method's annotation.**
///
/// A parameter annotation is evaluated in the class scope when the `def` runs,
/// so `List` there is the class attribute. This is the opposite of a bare name
/// in the method *body*, which skips the class scope - `LAN-104` pins that.
#[test]
fn a_class_attribute_named_list_changes_a_method_annotation() {
    let body = LOOP
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    assert_not_a_collection(
        &format!(
            "from typing import List\n\n\nclass C:\n    List = MyList\n\n    def m(self, items: List[str]) -> int:\n{body}"
        ),
        "C.m",
        "the class body binds `List`, and the annotation is evaluated there",
    );
}

// ---------------------------------------------------------------------------
// 3 · what stays out, and what stays in
// ---------------------------------------------------------------------------

/// **The abstract types are not admitted.** `Sequence` is `LAN-106`'s second
/// half: a user `__len__` can disagree with the iteration count, which is a
/// weaker trust than the builtins carry, and it is a decision not taken here.
#[test]
fn an_abstract_sequence_is_not_admitted() {
    for (import, annotation) in [
        ("from typing import Sequence", "Sequence[str]"),
        ("from collections.abc import Collection", "Collection[str]"),
        ("from typing import Mapping", "Mapping[str, int]"),
        ("from typing import Iterable", "Iterable[str]"),
    ] {
        assert_not_a_collection(
            &format!("{import}\n\n\ndef f(items: {annotation}) -> int:\n{LOOP}"),
            "f",
            "an abstract collection type is not a builtin under another name",
        );
    }
}

/// **The lowercase builtins are unchanged.**
#[test]
fn the_lowercase_builtins_are_unchanged() {
    assert_counted(&format!("def f(items: list[str]) -> int:\n{LOOP}"), "f");
    assert_counted(&format!("def f(items: dict) -> int:\n{LOOP}"), "f");
}
