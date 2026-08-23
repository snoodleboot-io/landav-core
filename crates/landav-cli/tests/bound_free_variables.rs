//! `LAN-87` acceptance, over a corpus: **every variable in a reported bound is
//! one the caller can supply.**
//!
//! # The invariant
//!
//! For every function and every `VarId` occurring in the bound the engine
//! reports:
//!
//! ```text
//! program.params().contains(var) || Hole::is_hole(var)
//! ```
//!
//! A parameter is a number the caller knows. A hole is a named region whose
//! cost the caller can look up, fill with `Bound::subst`, or read as `omega`.
//! Anything else — a local, a loop counter, a name from an enclosing scope — is
//! a symbol with no referent outside the function body, and a bound containing
//! one cannot be evaluated, compared against a budget, or substituted into.
//!
//! It is also how the number comes out *small*. `Bound::eval` needs a total
//! valuation; a consumer that supplies the parameters and nothing else gets
//! zero for the local, and `m = n * n; for i in range(m)` reports `1 + 2m`,
//! which reads as a constant.
//!
//! # Why over a corpus rather than as three cases
//!
//! `value_soundness.rs` pins the three shapes that were measured. This is the
//! same invariant asked of every function the frontend can translate, which is
//! the only form in which "the fix was general" is a claim rather than a hope.
//! It matters more after `LAN-87` than before: today the engine is consulted
//! for the 0.5% of functions that lower, and afterwards it is consulted for all
//! of them.
//!
//! # Running it against the real corpora
//!
//! ```text
//! LANDAV_BOUND_CORPUS=/usr/lib/python3.12:$(python3 -c 'import numpy, os; print(os.path.dirname(numpy.__file__))') \
//!   cargo test -p landav-cli --test bound_free_variables
//! ```
//!
//! With the variable unset the harness runs the in-file corpus below, which is
//! small, known-clean of exotica, and deliberately contains the shapes the
//! invariant is about — so an unset variable weakens the evidence and never
//! makes the test vacuous. Every run reports how many functions it actually
//! examined and fails if that number is zero.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    fs,
    path::{Path, PathBuf},
};

use landav_engine::{Hole, cost};

/// Points the harness at real Python trees, colon-separated.
const CORPUS_ENV: &str = "LANDAV_BOUND_CORPUS";

/// The fallback corpus: ordinary-looking functions, each of which a reader
/// would expect a bound for.
///
/// Chosen to span the ways a name reaches a range endpoint — a parameter, a
/// local computed from one, a local constant, a reassigned parameter, a loop
/// counter — because that is where a variable escapes into the answer.
const IN_FILE_CORPUS: &[(&str, &str)] = &[
    (
        "counted.py",
        "def straight(n: int) -> int:\n    total = 0\n    for i in range(n):\n        total = total + i\n    return total\n",
    ),
    (
        "local_endpoint.py",
        "def widened(n: int) -> int:\n    m = n * n\n    x = 0\n    for i in range(m):\n        x = i\n    return x\n",
    ),
    (
        "constant_endpoint.py",
        "def fixed(n: int) -> int:\n    m = 5\n    x = 0\n    for i in range(m):\n        x = i\n    return x\n",
    ),
    (
        "reassigned.py",
        "def squared(n: int) -> int:\n    n = n * n\n    x = 0\n    for i in range(n):\n        x = i\n    return x\n",
    ),
    (
        "nested.py",
        "def triangular(n: int) -> int:\n    total = 0\n    for i in range(n):\n        for j in range(i):\n            total = total + 1\n    return total\n",
    ),
    (
        "calls.py",
        "def measured(n: int) -> int:\n    x = 0\n    for i in range(n):\n        helper(i)\n    return x\n",
    ),
];

/// Every `.py` file under `root`, depth first, symlinks not followed.
fn python_files(root: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            python_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "py") {
            found.push(path);
        }
    }
}

/// The corpus: the real trees when they are named, the in-file set otherwise.
fn corpus() -> (String, Vec<(PathBuf, String)>) {
    let Ok(roots) = std::env::var(CORPUS_ENV) else {
        let files = IN_FILE_CORPUS
            .iter()
            .map(|(name, source)| (PathBuf::from(name), (*source).to_owned()))
            .collect();
        return ("the in-file corpus".to_owned(), files);
    };

    let mut paths = Vec::new();
    for root in roots.split(':').filter(|part| !part.is_empty()) {
        let root = Path::new(root);
        assert!(
            root.is_dir(),
            "{CORPUS_ENV} names `{}`, which is not a directory",
            root.display()
        );
        python_files(root, &mut paths);
    }
    let files: Vec<(PathBuf, String)> = paths
        .into_iter()
        .filter_map(|path| fs::read_to_string(&path).ok().map(|text| (path, text)))
        .collect();
    (format!("{roots} ({} file(s))", files.len()), files)
}

/// **No bound mentions anything the caller cannot supply.**
#[test]
fn every_variable_in_a_reported_bound_is_a_parameter_or_a_hole() {
    let (label, files) = corpus();
    assert!(!files.is_empty(), "{label}: no Python to analyse");

    let mut examined = 0_usize;
    let mut unparsed = 0_usize;
    let mut violations: Vec<String> = Vec::new();

    for (path, text) in &files {
        // A file the frontend cannot parse is a coverage gap, not a violation.
        let Ok(functions) = landav_python::lower_module(path, text) else {
            unparsed += 1;
            continue;
        };
        for function in &functions {
            let program = function.program();
            let result = cost(program);
            examined += 1;
            let Some(bound) = result.bound() else {
                continue;
            };
            // A hole variable is exempt only while the result is *partial*. A
            // complete bound is one a consumer may compare against a budget, and
            // an unfilled hole denotes omega - so a hole in one is not a
            // footnote, it is a finite-looking claim with an infinite term that
            // a caller supplying only the parameters reads as zero. Exempting
            // holes unconditionally is what let `close_over_counter` publish
            // `O(2 + n * (1 + #hole0 + 2n))` as complete, with an empty hole
            // ledger, for a program containing an unanalysed `while`.
            let complete = result.is_complete();
            for var in bound.vars() {
                let name = var.symbol().as_str().to_owned();
                let supplied = program
                    .params()
                    .iter()
                    .any(|param| param.symbol().as_str() == name);
                if supplied || (!complete && Hole::is_hole(&var)) {
                    continue;
                }
                let at = function.location();
                violations.push(format!(
                    "  {}:{}: {}: `{name}` in {bound}",
                    at.file().display(),
                    at.line(),
                    function.name(),
                ));
            }
        }
    }

    assert!(
        examined > 0,
        "{label}: no function was analysed, so this test asserted nothing \
         ({unparsed} file(s) did not parse)"
    );

    // Deduplicated and capped: a systematic violation produces one row per
    // function, and a thousand identical rows hides the shape.
    violations.sort();
    violations.dedup();
    let shown: Vec<&String> = violations.iter().take(25).collect();
    assert!(
        violations.is_empty(),
        "{label}: {} of {examined} analysed function(s) reported a bound \
         mentioning a variable that is neither a parameter nor a hole. Such a \
         bound cannot be evaluated by the caller, and evaluates to zero for a \
         caller who supplies the parameters and nothing else - which turns a \
         quadratic cost into a constant.\n\n{}\n{}",
        violations.len(),
        shown
            .iter()
            .map(|row| row.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        if violations.len() > shown.len() {
            format!("  ... and {} more", violations.len() - shown.len())
        } else {
            String::new()
        }
    );
}

/// **The corpus contains something the invariant could catch.**
///
/// The positive control. The test above passes trivially over a corpus of
/// functions that report no bound at all, which is close to what the corpus
/// looked like before this ticket: at 0.5% coverage the engine was consulted
/// for almost nothing. This asserts that the run actually produced bounds, so
/// that a green result means the invariant held rather than that nothing was
/// examined.
#[test]
fn the_corpus_produces_bounds_for_the_invariant_to_hold_over() {
    let (label, files) = corpus();
    let mut with_a_bound = 0_usize;
    let mut functions = 0_usize;

    for (path, text) in &files {
        let Ok(lowered) = landav_python::lower_module(path, text) else {
            continue;
        };
        for function in &lowered {
            functions += 1;
            if cost(function.program()).bound().is_some() {
                with_a_bound += 1;
            }
        }
    }

    assert!(
        with_a_bound > 0,
        "{label}: none of the {functions} function(s) produced a bound, so the \
         free-variable invariant held vacuously"
    );
}
