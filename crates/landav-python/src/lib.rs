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
}
