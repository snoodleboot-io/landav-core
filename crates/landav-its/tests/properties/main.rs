//! `LAN-67` acceptance criteria, as executable properties.
//!
//! # Why this file exists
//!
//! Cargo compiles `tests/*.rs` and `tests/*/main.rs`, and **nothing else**. A
//! file dropped into `tests/properties/` that is not reached from here is not
//! compiled, is not run, and reports no failure — which on a suite whose whole
//! job is a zero-target soundness metric is worse than having no suite at all.
//! Every sibling module is declared below.
//!
//! # What is encoded here
//!
//! | `LAN-67` AC | file |
//! |---|---|
//! | 1. locations, transitions, guards and polynomial updates emitted | `fragment`, `koat_format` |
//! | 2. `while`, `for`-range, `if`/`else`, nested loops, integer arithmetic | `fragment`, `soundness` |
//! | 3. 20 hand-written functions lower and are accepted | `corpus`, `koat_format` |
//! | 4. unsupported constructs diagnose, never silently truncate | `refusal` |
//!
//! `LAN-68` continues from there — the report, the accumulation across units
//! and the coverage number:
//!
//! | `LAN-68` AC | file |
//! |---|---|
//! | 1. every unsupported construct maps to a named diagnostic | `refusal`, `coverage` |
//! | 2. a coverage report lists which constructs were skipped and why | `coverage` |
//!
//! and the non-negotiable that outranks all four:
//!
//! | Non-negotiable | file |
//! |---|---|
//! | 1. soundness has a zero target | `soundness` |
//! | 2. never panic; no recursion on untrusted input | `refusal::deep_input` |
//! | 3. failure carries blame | `refusal` |
//!
//! # The reference semantics is not the lowering
//!
//! `reference` contains two interpreters written from the *specification* in
//! the crate's doc comments: one for [`landav_its::SourceProgram`] and one for
//! [`landav_its::Its`]. Neither calls `lower`, `Polynomial::evaluate`,
//! `Guard::holds` or any other decision procedure from the crate under test —
//! polynomial evaluation is written out again over `monomials()` and
//! `coefficient()`, so a disagreement is a real disagreement and not two
//! copies of one mistake. This follows `landav-bound`'s `naive_eval` idiom
//! deliberately.
//!
//! # These tests were written against the frozen signatures, before the bodies
//!
//! `lower` and `koat::render` were `todo!()` when this suite was written, so it
//! started red on purpose. **It is green now, and green is the expectation.** A
//! failure below is a soundness defect in the lowering, not a milestone
//! artefact.

// The panic lints are relaxed in test code only. A test that cannot assert is
// not a test, and `assert!`/`unwrap` failing *is* the reporting mechanism —
// unlike library code, where a panic destroys the blame path that makes a
// partial result useful.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod reference;
mod support;

mod algebra;
mod corpus;
mod coverage;
mod fragment;
mod koat_format;
mod refusal;
mod soundness;
