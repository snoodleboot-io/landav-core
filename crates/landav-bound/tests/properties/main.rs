//! LAN-56 acceptance criteria, as executable properties.
//!
//! # Why this file exists
//!
//! Cargo compiles `tests/*.rs` and `tests/*/main.rs`, and **nothing else**. A
//! file dropped into `tests/properties/` that is not reached from here is not
//! compiled, is not run, and reports no failure - which on a suite whose whole
//! job is a zero-target soundness metric is worse than having no suite at all.
//! Every sibling module is declared below.
//!
//! # What is encoded here
//!
//! | LAN-56 AC | file |
//! |---|---|
//! | 1. six constructors, `omega` inside `Const` | `denotation`, `support::spec_of_shape` |
//! | 2. `log` is `ceil(log_k(max(1, b)))`, integer only | `log_edges` |
//! | 3. monotonicity under pointwise argument increase | `monotonicity` |
//! | 4. no panics on `omega` in any operator | `omega_totality` |
//!
//! and LAN-57's, which build on the same harness:
//!
//! | LAN-57 AC | file |
//! |---|---|
//! | 1. `subst(bound, var, bound)` is total | `substitution` |
//! | 2. composition of two bounds is a bound (closed) | `substitution` |
//! | 3. KoAT's worked example, up to canonical form | `substitution` |
//!
//! plus the decisions the design panel and the three adversaries settled,
//! which are acceptance criteria in everything but numbering: unconditional
//! `omega` absorption including `0 * omega`, saturation rather than
//! truncation, a canonical order that is total, deterministic and content
//! derived, denotation-preserving smart constructors, and a `Verdict` that
//! refuses to publish an unblamed `omega`.
//!
//! # These tests were written against the frozen signatures, before the bodies
//!
//! Every body in `landav-bound` was `todo!()` when this suite was written, so
//! it started red on purpose: a property that only exists after the code it
//! judges has a way of agreeing with it.
//!
//! **It is green now, and green is the expectation.** The note that used to
//! stand here said the suite "is expected to fail", which outlived the
//! condition that made it true — and a red-is-expected note on a suite that
//! passes is exactly how a genuinely red suite gets waved through. A failure
//! below is a soundness defect in the algebra, not a milestone artefact.

mod support;

mod adversary;
mod canonical_order;
mod denotation;
mod limits;
mod log_edges;
mod monotonicity;
mod normal_form;
mod omega_totality;
mod substitution;
mod verdict_blame;
