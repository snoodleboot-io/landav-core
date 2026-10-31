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
const SUPPORTED: [(&str, &str); 12] = [
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
];

/// Source this frontend cannot read, in both of the classes there are.
///
/// Until `LAN-130` this list was two PEP 701 f-strings - valid Python 3.12 the
/// parser predated - and they have moved into [`SUPPORTED`]. What is left is
/// the distinction that replaced them:
///
/// * **not Python at any version.** Nothing will ever parse it.
/// * **Python above the declared level.** `except A, B:` is PEP 758, valid
///   3.14, and `landav_python::SUPPORTED_PYTHON` says 3.12. The parser reads
///   the form and reports it as unsupported *for the target*, which
///   `parse_guarded` folds into the same refusal - because a file above the
///   level is a file this build has not read.
///
/// The second entry tracks the declared level and is expected to move into
/// [`SUPPORTED`] when the level passes 3.14, exactly as the f-strings did.
const UNREADABLE: [(&str, &str); 2] = [
    ("not Python at any version", "def f(:\n    return 1\n"),
    (
        "PEP 758 multi-except, above the declared level",
        "def f(xs):\n    try:\n        go(xs)\n    except ValueError, TypeError:\n        pass\n",
    ),
];

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
