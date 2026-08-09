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

/// Syntax that Python 3.12 accepts and this frontend does not.
///
/// PEP 701 lifted the restriction on reusing the enclosing quote inside an
/// f-string and on splitting the expression across lines. Both are now emitted
/// by formatters, so a 3.12 repository will contain them.
const PEP_701: [(&str, &str); 2] = [
    ("same quote nested", "label = f\"{row[\"name\"]}\"\n"),
    ("multi-line expression", "label = f\"{\n    row.name\n}\"\n"),
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
    for (label, source) in PEP_701 {
        match analyze_source(Path::new("modern.py"), source) {
            Ok(findings) => assert!(
                !findings.is_empty(),
                "{label}: parsed but yielded nothing; if the parser now reads PEP 701 this test \
                 should be moved into SUPPORTED"
            ),
            Err(PythonError::Parse { line, column, .. }) => {
                assert!(line >= 1 && column >= 1, "{label}: position is not 1-based");
            }
            Err(other) => panic!("{label}: expected a parse error, got {other}"),
        }
    }
}

/// The frontend's Python ceiling, asserted so that it is a published fact.
///
/// This currently records a **gap**: PEP 701 f-strings are valid Python 3.12
/// and are rejected. When the parser is upgraded this test starts failing, and
/// the fix is to move the two cases into [`SUPPORTED`] and delete this test —
/// which is exactly the notification an upgrade should produce.
#[test]
fn pep_701_f_strings_are_a_known_gap() {
    let unread: Vec<&str> = PEP_701
        .iter()
        .filter(|(_, source)| analyze_source(Path::new("modern.py"), source).is_err())
        .map(|(label, _)| *label)
        .collect();
    assert_eq!(
        unread.len(),
        PEP_701.len(),
        "the parser now reads some PEP 701 f-strings ({unread:?} still fail); move the readable \
         cases into SUPPORTED and retire this test"
    );
}
