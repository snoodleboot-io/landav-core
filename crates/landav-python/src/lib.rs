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

#![doc(html_root_url = "https://docs.rs/landav-python")]

// TODO(LAN-4): Quadratic anti-pattern rules over the parsed frontend.
//
// TODO(LAN-25): Implement the six FDK traits once F-043 publishes them at R3.
// Until then this crate is written against the shape in `landav_fdk`, so that
// the eventual extraction is a move rather than a redesign.

/// Placeholder so the workspace builds before `LAN-4` lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Unimplemented;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_builds() {
        assert_eq!(Unimplemented, Unimplemented);
    }
}
