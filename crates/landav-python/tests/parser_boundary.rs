//! What the frontend does with Python it cannot parse.
//!
//! # Why this is a separate file from the fixture corpus
//!
//! The corpus asserts *findings*. This asserts the thing that has to be true
//! before a finding means anything: a file the frontend cannot read is
//! reported as such, never as "no findings". Those are different answers, and a
//! caller that cannot tell them apart will read a parse failure as a clean bill
//! of health for the file.
//!
//! It also pins the frontend's Python version ceiling, so that the ceiling is a
//! fact the build asserts rather than something a user discovers.

// See `common/mod.rs` for why the panic lints are relaxed in test code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_python::{PythonError, analyze_source};

/// Modern syntax the frontend does parse, one construct each.
///
/// Every one of these is ordinary in a repository written against a supported
/// Python, so a regression here silently removes files from analysis.
const SUPPORTED: [(&str, &str); 17] = [
    (
        "match statement",
        "match command:\n    case {'op': op}:\n        pass\n    case [head, *tail]:\n        pass\n",
    ),
    (
        "walrus in a comprehension",
        "kept = [y for x in xs if (y := clean(x)) is not None]\n",
    ),
    (
        "except*",
        "try:\n    run()\nexcept* ValueError:\n    pass\n",
    ),
    ("PEP 695 type alias", "type Ids = list[int]\n"),
    (
        "PEP 695 generic function",
        "def first[T](xs: list[T]) -> T:\n    return xs[0]\n",
    ),
    ("PEP 695 generic class", "class Box[T]:\n    pass\n"),
    (
        "async for",
        "async def drain(source):\n    async for row in source:\n        pass\n",
    ),
    (
        "parenthesised with items",
        "with (open('a') as first, open('b') as second):\n    pass\n",
    ),
    (
        "positional-only parameters",
        "def clamp(value, /, low, *, high):\n    return value\n",
    ),
    (
        "non-ASCII identifiers",
        "def moyenne(données):\n    return sum(données)\n",
    ),
    (
        "tab-indented blocks",
        "def f():\n\tif True:\n\t\treturn 1\n\treturn 2\n",
    ),
    (
        "nested f-string, pre-PEP-701 spelling",
        "label = f\"{f'{value}'}\"\n",
    ),
    // `LAN-132`: 3.13, 3.14 and 3.15. Each was unreadable or unclaimed
    // before it, and each is here because the *rule* path walks the tree with
    // different code from the lowering path - `python_syntax_level.rs` pins
    // the same forms on the other side.
    (
        "PEP 696 type-parameter default (3.13)",
        "def make[T = int](value: T) -> T:\n    return value\n",
    ),
    (
        "PEP 758 unparenthesised except tuple (3.14)",
        "try:\n    go()\nexcept ValueError, TypeError:\n    pass\n",
    ),
    (
        "PEP 750 template string (3.14)",
        "message = t\"hello {name}\"\n",
    ),
    (
        "PEP 810 lazy import (3.15)",
        "lazy import os\nlazy from threading import Lock\n",
    ),
    (
        "PEP 798 starred element in a comprehension (3.15)",
        "items = [*row for row in rows]\n",
    ),
];

/// Source this frontend cannot read. **There is one class of it left.**
///
/// This list has shrunk twice, both times because the frontend caught up:
///
/// * `LAN-130` moved two PEP 701 f-strings into [`SUPPORTED`] - valid Python
///   3.12 that the old parser predated;
/// * `LAN-132` moved the PEP 758 multi-except there too. It was the
///   *above-the-declared-level* case: the parser read the form and reported it
///   as unsupported for the target, which `parse_guarded` folds into the same
///   refusal, because a file above the level is a file this build has not
///   read.
///
/// Nothing replaced the second entry, and nothing can while
/// `landav_python::SUPPORTED_PYTHON` is both the newest released Python and
/// the newest its parser knows. `landav-python`'s
/// `the_declared_level_is_the_newest_python_the_parser_knows` fails when that
/// stops holding, and whoever sees it puts a 3.16-only form back here.
///
/// What remains is the class no version claim can empty: source that is not
/// Python at any version, where nothing will ever parse it.
const UNREADABLE: [(&str, &str); 1] = [("not Python at any version", "def f(:\n    return 1\n")];

#[test]
fn supported_syntax_is_analysed_rather_than_rejected() {
    let mut rejected = Vec::new();
    for (label, source) in SUPPORTED {
        if let Err(error) = analyze_source(Path::new("supported.py"), source) {
            rejected.push(format!("{label}: {error}"));
        }
    }
    assert!(
        rejected.is_empty(),
        "the frontend rejected syntax it is expected to read:\n  {}",
        rejected.join("\n  ")
    );
}

/// A file that cannot be parsed must produce an error naming a position, not an
/// empty finding list. This is the assertion that keeps "could not look" from
/// being reported as "nothing to see".
#[test]
fn unparsable_source_is_an_error_with_a_position_and_never_an_empty_result() {
    for (label, source) in UNREADABLE {
        match analyze_source(Path::new("modern.py"), source) {
            Ok(findings) => panic!(
                "{label}: parsed, yielding {} finding(s). If the frontend now reads this, move \
                 the entry into SUPPORTED rather than leaving a claim here that no longer holds",
                findings.len()
            ),
            Err(PythonError::Parse { line, column, .. }) => {
                assert!(line >= 1 && column >= 1, "{label}: position is not 1-based");
            }
            Err(other) => panic!("{label}: expected a parse error, got {other}"),
        }
    }
}
