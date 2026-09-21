//! `LAN-106` tier 2: **an abstract collection type is trusted, and the trust is
//! said out loud.**
//!
//! # The decision, and why it is not free
//!
//! `Sequence`, `Collection` and `Mapping` promise `__len__`, and 32 typed-corpus
//! loops iterate a parameter annotated with one and would otherwise never be
//! counted. They are admitted.
//!
//! They are not the same claim as `list`. For a concrete builtin, CPython
//! guarantees that iterating yields exactly `len` items. For an abstract type
//! both sides are user code - `__len__`, and `__iter__` or `__getitem__` until
//! `IndexError` - and a class whose iteration outruns its length makes a bound
//! derived from that length **exceedable**, which is the one direction this
//! project gives a zero target.
//!
//! It is admitted because being wrong requires a class to contradict the
//! protocol it declares, and because the premise no longer disappears into an
//! identical-looking number.
//!
//! # Premises are not assumptions
//!
//! `landav_bound::Assumption` names what a run could **not** derive, and hangs
//! off a hole in a `partial` result. A premise is the opposite shape: the bound
//! is complete, possibly exact, and rests on something nothing verified. So it
//! needed its own carrier -
//! [`landav_its::SourceProgram::rests_on_a_protocol`] - rather than being bent
//! into the hole machinery.
//!
//! Both tiers are published, because admitting the weaker one is also what made
//! the older, silent one worth saying: every `len(items)` has always rested on
//! the annotation being true.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use serde_json::Value;

use common::Project;

const LOOP: &str = "    total = 0\n    for row in rows:\n        total += 1\n    return total\n";

fn functions(source: &str) -> Vec<landav_python::LoweredFunction> {
    landav_python::lower_module(std::path::Path::new("protocol.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"))
}

/// Whether `rows` became a length the caller supplies, and what it rests on.
fn length_trust(source: &str) -> Option<&'static str> {
    let functions = functions(source);
    let program = functions.last().expect("a function").program();
    let length = program
        .params()
        .iter()
        .find(|name| name.symbol().as_str() == "len(rows)")?;
    Some(if program.rests_on_a_protocol(length) {
        "protocol"
    } else {
        "concrete-type"
    })
}

// ---------------------------------------------------------------------------
// 1 · what is admitted, and on which trust
// ---------------------------------------------------------------------------

/// **Every sized protocol counts, from either home, in every spelling.**
#[test]
fn the_sized_protocols_are_admitted_as_protocol_trust() {
    for (import, annotation) in [
        ("from typing import Sequence", "Sequence[str]"),
        ("from collections.abc import Sequence", "Sequence[str]"),
        ("from collections.abc import Collection", "Collection[str]"),
        ("from collections.abc import Mapping", "Mapping[str, int]"),
        (
            "from collections.abc import MutableSequence",
            "MutableSequence[str]",
        ),
        ("from collections.abc import Sequence as Seq", "Seq[str]"),
        ("import typing", "typing.Sequence[str]"),
        ("import collections.abc as abc", "abc.Sequence[str]"),
        ("import collections.abc", "collections.abc.Sequence[str]"),
    ] {
        let source = format!("{import}\n\n\ndef walk(rows: {annotation}) -> int:\n{LOOP}");
        assert_eq!(
            length_trust(&source),
            Some("protocol"),
            "`{annotation}` promises `__len__`, and its length is trusted - but as a \
             protocol, not as a concrete type:\n{source}"
        );
    }
}

/// **A concrete builtin is still concrete trust**, including `typing`'s
/// spelling of it. The tiers must not collapse into each other.
#[test]
fn the_concrete_builtins_keep_concrete_trust() {
    for (import, annotation) in [
        ("", "list[str]"),
        ("", "dict"),
        ("from typing import List", "List[str]"),
        ("import typing", "typing.Dict[str, int]"),
    ] {
        let source = format!("{import}\n\n\ndef walk(rows: {annotation}) -> int:\n{LOOP}");
        assert_eq!(
            length_trust(&source),
            Some("concrete-type"),
            "`{annotation}` is a builtin, whose iteration CPython makes agree with \
             its length:\n{source}"
        );
    }
}

/// **What promises no length still counts for nothing.** `Iterable` is the
/// common case and the reason the protocol list is a list rather than "anything
/// abstract": a generator satisfies `Iterable` and has no length at all.
#[test]
fn an_iterable_is_not_sized() {
    for annotation in ["Iterable[str]", "Iterator[str]", "Generator"] {
        let source = format!(
            "from collections.abc import Iterable, Iterator, Generator\n\n\ndef walk(rows: {annotation}) -> int:\n{LOOP}"
        );
        assert_eq!(
            length_trust(&source),
            None,
            "`{annotation}` promises no `__len__`:\n{source}"
        );
    }
}

/// **The name still has to be the protocol's where the annotation reads it.**
/// The tier-1 rule applies unchanged: a module's own `Sequence` class is not
/// `collections.abc`'s.
#[test]
fn a_module_class_named_sequence_is_not_admitted() {
    let source = format!("class Sequence:\n    pass\n\n\ndef walk(rows: Sequence) -> int:\n{LOOP}");
    assert_eq!(length_trust(&source), None, "{source}");
    let shadowed = format!(
        "from collections.abc import Sequence\n\nSequence = MySequence\n\n\ndef walk(rows: Sequence) -> int:\n{LOOP}"
    );
    assert_eq!(length_trust(&shadowed), None, "{shadowed}");
}

// ---------------------------------------------------------------------------
// 2 · the premise is reported
// ---------------------------------------------------------------------------

fn json_of(project: &Project, source: &str) -> io::Result<Value> {
    let target = project.write("premise.py", source)?;
    let run = project.check(&target, &["--json"])?;
    Ok(serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not valid JSON ({why}): {}", run.describe())))
}

/// **Each bound publishes what it is believed on, and which tier.**
#[test]
fn the_json_publishes_the_premise_and_its_tier() -> io::Result<()> {
    let project = Project::new()?;
    let parsed = json_of(
        &project,
        &format!(
            "from collections.abc import Sequence\n\n\ndef walk(rows: Sequence[str]) -> int:\n{LOOP}\n\ndef concrete(rows: list) -> int:\n{LOOP}"
        ),
    )?;
    for (name, tier) in [("walk", "protocol"), ("concrete", "concrete-type")] {
        let function = parsed["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap_or_else(|| panic!("no function {name}"));
        let premises = function["premises"].as_array().unwrap();
        assert_eq!(
            premises.len(),
            1,
            "the bound mentions one trusted length: {function}"
        );
        assert_eq!(premises[0]["subject"], "len(rows)", "{function}");
        assert_eq!(
            premises[0]["trust"], tier,
            "the two tiers must stay distinguishable, or filtering on them is \
             impossible: {function}"
        );
        assert!(
            premises[0]["because"]
                .as_str()
                .is_some_and(|s| s.len() > 40),
            "a premise must say why the number is believed: {function}"
        );
    }
    Ok(())
}

/// **A length the analysis never read is not a premise.**
///
/// Nothing about the result depends on it, and listing it would make the field
/// noise instead of signal.
#[test]
fn an_unread_length_is_not_a_premise() -> io::Result<()> {
    let project = Project::new()?;
    let parsed = json_of(
        &project,
        "from collections.abc import Sequence\n\n\ndef ignores(rows: Sequence[str], n: int) -> int:\n    return n\n",
    )?;
    let function = &parsed["functions"][0];
    assert_eq!(
        function["premises"].as_array().unwrap().len(),
        0,
        "the function never reads `len(rows)`: {function}"
    );
    Ok(())
}

/// **A bound that rests on the premise while naming nothing still declares it.**
///
/// The case a rule keyed on the rendered bound gets wrong, and the one that
/// matters most. Trusting the protocol makes `len(element)` a value this
/// fragment can read, so the function is `Theta(1)` - complete, exact, and
/// mentioning no length. Decline the trust and `len()` is an unknown call and
/// the result is partial. The `1` therefore rests entirely on the premise, and
/// if a `__len__` is expensive it is a constant-cost claim the program exceeds.
///
/// Found by running the typed corpus under both settings and asking which
/// results moved: two did so without declaring a premise, and both claimed
/// constant cost.
#[test]
fn a_constant_bound_that_only_a_read_length_made_complete_declares_it() -> io::Result<()> {
    let project = Project::new()?;
    let parsed = json_of(
        &project,
        "from collections.abc import Collection\n\n\ndef max_len(size: int, element: Collection[object]) -> bool:\n    return len(element) <= size\n",
    )?;
    let function = &parsed["functions"][0];
    assert_eq!(
        function["bound_kind"], "exact",
        "the length is readable, so nothing here is a region: {function}"
    );
    let premises = function["premises"].as_array().unwrap();
    assert_eq!(
        premises.len(),
        1,
        "the bound names no length and rests entirely on one: {function}"
    );
    assert_eq!(premises[0]["trust"], "protocol", "{function}");
    Ok(())
}

/// **The text says the weaker premise, and not the baseline one.**
///
/// A reader is told what is unusual about this function. That every annotation
/// is trusted is the tool's documented baseline, and repeating it on every line
/// would bury the line that matters.
#[test]
fn the_text_names_the_protocol_premise_only() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write(
        "premise.py",
        &format!(
            "from collections.abc import Sequence\n\n\ndef walk(rows: Sequence[str]) -> int:\n{LOOP}"
        ),
    )?;
    let run = project.check(&target, &["--bounds"])?;
    assert!(
        run.mentions("believed on len(rows)") && run.mentions("__len__"),
        "the weaker trust must be visible to a reader: {}",
        run.describe()
    );

    let concrete = project.write(
        "concrete.py",
        &format!("def walk(rows: list) -> int:\n{LOOP}"),
    )?;
    let run = project.check(&concrete, &["--bounds"])?;
    assert!(
        !run.mentions("believed on"),
        "a concrete annotation is the baseline and is not repeated per function: {}",
        run.describe()
    );
    Ok(())
}
