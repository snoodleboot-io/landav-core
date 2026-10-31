//! The Python frontend — the reference FDK implementation.
//!
//! # Scope
//!
//! Components `C-04` (Parser Frontend), `C-05` (Type Resolution), `C-31`
//! (Pattern Rule Engine) and `C-34` (Python Frontend). Feature [`F-005`],
//! release R0, milestone M0.
//!
//! # Reference, not specification
//!
//! Python is the **first frontend, not the product**. Of thirty-six
//! components, twenty-one are Core and never mention a source language; seven
//! are per-language. Core carries 644 story points and is written once; the
//! per-language surface carries roughly 90 and recurs. That roughly
//! seven-to-one ratio is the entire leverage argument.
//!
//! It only holds if the boundary is real, so the plan enforces it with a lint
//! and measures it with the language-independent conformance suite (`F-044`).
//! This crate must never become the specification — that is exactly what the
//! conformance suite exists to prevent.
//!
//! # Day-one value before any inference works
//!
//! `F-005` ports the PERF/RUF-class rules: `try`/`except` inside a loop, list
//! concatenation in a loop, membership test against a list, `.index()` inside a
//! loop over the same list, loop-invariant statements. These ship value before
//! bound inference works and validate the frontend plumbing.
//!
//! They are also the one part of Landav a linter already does. They earn their
//! place as plumbing validation, not as differentiation: PERF-class rules match
//! patterns someone already named, while Landav derives bounds.
//!
//! # Edition
//!
//! OSS — Apache-2.0, ships in `landav-core`.
//!
//! [`F-005`]: https://linear.app/snoodleboot/issue/LAN-4

//! # The `LAN-65` contract
//!
//! The types below and [`analysis::analyze_source`] are the surface the
//! `LAN-65` fixture corpus is written against. They were declared by the test
//! author before any rule existed, which is the point: per `CONTRIBUTING.md`,
//! the acceptance criteria are encoded by someone other than the implementer
//! and are not edited to make an implementation pass.
//!
//! The implementation lane fills them: [`registry::registry`] declares the
//! `LAV0xx` rules and [`analysis::analyze_source`] parses the source and runs
//! them. The contract itself is unchanged — no fixture and no assertion was
//! edited to make a rule pass, which is the only reading of the corpus that
//! means anything.
//!
//! # Ten rules, not eleven
//!
//! `LAV010` (`try`/`except` in a loop) was specified, implemented, and then
//! **withdrawn**. Its premise is that a never-slower total alternative to the
//! guarded lookup always exists, and that premise is false: `sqlite3.Row` has
//! no `.get`, `Element.get` reads an XML attribute rather than a child, and on
//! a hot dispatch table `.get` costs an attribute lookup and a Python-level
//! call where `[]` is one opcode. Which of those applies depends on the
//! receiver's type, which this crate cannot know. The corpus contains a case
//! that is *structurally identical* to a true positive and is nonetheless
//! correct code, so no narrowing separates them.
//!
//! `MINIMUM_RULE_COUNT` is eight and the specification carried eleven
//! precisely so that a rule which cannot be made precise can be withdrawn
//! rather than shipped noisy. Its fixture directory is left in place: deleting
//! it needs the test author's sign-off, not the implementer's.
//!
//! # Suppression (`LAN-66`)
//!
//! A rule set nobody can silence is a rule set that gets switched off whole,
//! so [`analyze_module_with`] honours `# noqa: LAV003` comments and the
//! per-path [`PathWaiver`]s a driver hands it. What makes this more than a
//! filter is [`ModuleAnalysis::suppressions`]: every waiver produces a
//! [`Suppression`] record — including the ones that suppressed nothing and the
//! ones naming a code that does not exist — so a waiver can never go quiet.
//! [`suppression`] explains the record's shape and the `E-001` seam it was
//! designed against.

#![doc(html_root_url = "https://docs.rs/landav-python")]
#![forbid(unsafe_code)]

// TODO(LAN-25): Implement the six FDK traits once F-043 publishes them at R3.
// Until then this crate is written against the shape in `landav_fdk`, so that
// the eventual extraction is a move rather than a redesign.

pub mod analysis;
pub mod finding;
pub mod location;
pub mod lowered_function;
pub mod lowering;
pub mod python_error;
pub mod registry;
pub mod rule;
pub mod rule_code;
pub mod suppression;

// Internal, and deliberately not `pub`. Everything below is a detail of *how*
// the rules are decided; publishing it would make the parser choice part of the
// crate's contract, and F-043 has to be able to swap it at R3 without a
// breaking change.
//
// `noqa` is here for the same reason: the *record* a waiver produces is public
// because a driver has to report it, but the comment syntax it was written in
// is a Python fact and stays behind the frontend boundary.
mod context;
mod noqa;
mod patterns;
mod syntax;

pub use crate::{
    analysis::{ModuleAnalysis, analyze_module, analyze_module_with, analyze_source},
    finding::Finding,
    location::Location,
    lowered_function::LoweredFunction,
    lowering::{AnnotationTrust, SiblingCost, Siblings, lower_module, lower_module_with},
    python_error::PythonError,
    registry::{RETIRED_CODES, is_retired_code, registry, rule, rule_for_code},
    rule::Rule,
    rule_code::RuleCode,
    suppression::{PathWaiver, Suppression, SuppressionOrigin, SuppressionStatus, path_matches},
};

/// The lowest rule count `F-005` may ship with.
///
/// Acceptance criterion 1 of `LAN-65`. It lives here rather than only inside a
/// test so that the number is a published fact about the crate: a build that
/// silently dropped to seven rules is a build whose release notes are wrong,
/// not merely a build with a failing test.
pub const MINIMUM_RULE_COUNT: usize = 8;

/// The Python version this frontend reads, bar the forms in
/// [`UNREAD_AT_SUPPORTED_LEVEL`].
///
/// Acceptance criterion of `LAN-82`, and published here for
/// [`MINIMUM_RULE_COUNT`]'s reason: a build that reads less Python than its
/// release notes claim is a wrong build, not merely a red suite.
///
/// A version string on its own would be worth nothing — it cannot be exceeded
/// by a parser, and a claim that cannot be wrong is not a contract. What makes
/// this one falsifiable is `python_syntax_level.rs`, which holds a sample per
/// version and asserts that the highest one the frontend actually reads is
/// this number. Raising it without teaching the frontend the syntax fails.
///
/// # What 3.15 added over 3.12, and what each one does here
///
/// `LAN-132` raised this from 3.12 once `LAN-130` had put a parser underneath
/// that reads the whole range. Nothing in the three releases changes a bound;
/// each new form either lowers like the thing it resembles or becomes a named
/// refusal, and `python_syntax_level.rs` pins which:
///
/// | Release | Form | Here |
/// |---|---|---|
/// | 3.13 | PEP 696 type-parameter defaults | read; a `type` statement and a generic `def` are declarations either way |
/// | 3.14 | PEP 758 `except A, B:` without parentheses | read; the handler is `exceptional-control-flow` as always |
/// | 3.14 | PEP 750 t-strings | read, then refused as [`landav_its::Construct::TemplateString`] |
/// | 3.15 | PEP 810 `lazy import` | read; and it **removes** a cost row, see `lowering::imports_of` |
/// | 3.15 | PEP 798 `[*a, *b]` in a comprehension | read; still `comprehension` |
/// | 3.15 | `case +1:` | read; still `pattern-match` |
///
/// The one that is a soundness matter rather than a coverage one is `lazy
/// import`: a name bound lazily pays its module's import on first read, so
/// `LAN-128`'s module-attribute rows must not resolve through it.
pub const SUPPORTED_PYTHON: &str = "3.15";

/// [`SUPPORTED_PYTHON`] as the parser's own version, which is what
/// `analysis::parse_guarded` hands it.
///
/// The two must be the same fact, so this derives one from the other rather
/// than restating it: a version the parser does not know is a compile error
/// here, not a frontend that quietly reads a different language from the one
/// it advertises. `LAN-130`.
#[must_use]
pub fn target_version() -> ruff_python_ast::PythonVersion {
    match SUPPORTED_PYTHON {
        "3.12" => ruff_python_ast::PythonVersion::PY312,
        "3.13" => ruff_python_ast::PythonVersion::PY313,
        "3.14" => ruff_python_ast::PythonVersion::PY314,
        "3.15" => ruff_python_ast::PythonVersion::PY315,
        // Unreachable while `SUPPORTED_PYTHON` is one of the above, and a
        // deliberate floor rather than a panic: a library may not abort over
        // its own constant. Reading *less* Python than claimed makes
        // `python_syntax_level.rs` fail, which is where this would surface.
        _ => ruff_python_ast::PythonVersion::PY312,
    }
}

/// The forms at or below [`SUPPORTED_PYTHON`] the frontend nonetheless refuses.
///
/// **Empty since `LAN-130`**, and that is the point of the constant rather
/// than an invitation to delete it. It held five PEP 701 f-string forms, which
/// `rustpython-parser 0.4.0` predated and which cost three files of the 3.12
/// standard library and the 51 top-level functions in them. With
/// `ruff_python_parser` every one reads, so the list emptied - and
/// `python_syntax_level.rs` asserts the list against its own samples, so it
/// could not have stayed full by accident.
///
/// An entry goes back here the moment a form at the declared level turns out
/// unreadable, with its measured cost beside it. A gap nobody published is a
/// coverage denominator that silently shrank, which is the failure `LAN-82`
/// opens with.
pub const UNREAD_AT_SUPPORTED_LEVEL: &[&str] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_sorted_by_code() {
        let codes: Vec<&str> = registry().iter().map(|rule| rule.code().as_str()).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        assert_eq!(codes, sorted, "registry() must be in ascending code order");
    }

    #[test]
    fn rule_for_code_rejects_an_unknown_code() {
        assert!(rule_for_code("LAV999").is_none());
    }

    /// **The declared level is the newest Python the parser knows.**
    ///
    /// Not a vanity assertion. While this holds, *there is no valid Python
    /// above [`SUPPORTED_PYTHON`]* - so the whole class of "a well-formed file
    /// this build is too old to read" is empty, and the test fixtures that
    /// used to stand for it are gone rather than re-pointed at something else.
    /// `python_syntax_level.rs` records that, and this is the tripwire: when
    /// the parser learns 3.16 this fails, and whoever sees it knows to pick a
    /// 3.16-only form and put that class back.
    ///
    /// It is deliberately *not* an assertion that landav reads everything
    /// forever. It fails on a dependency bump, which is the point - the next
    /// release is a decision somebody records, exactly as `LAN-132` recorded
    /// this one.
    #[test]
    fn the_declared_level_is_the_newest_python_the_parser_knows() {
        assert_eq!(
            target_version(),
            ruff_python_ast::PythonVersion::latest(),
            "`SUPPORTED_PYTHON` is {SUPPORTED_PYTHON}, but the parser now \
             knows a newer release. Nothing is broken - but the \
             above-the-declared-level fixtures are empty on the grounds that \
             no such Python exists, and that is no longer true. See \
             `python_syntax_level.rs`"
        );
    }
}
