//! [`DioidLaws`] and the generic law suite.
//!
//! # Why this is evaluation-based rather than proptest-based
//!
//! An earlier plan was "constant-only proptest strategies now, upgrade once
//! the normaliser lands". That does not work, in two distinct ways.
//!
//! **It under-covers, undetectably.** On constant-only inputs both sides of
//! distributivity fold to the same constant, so **L3 is not checked at all**;
//! L1 and L2 degenerate into tests of scalar arithmetic, never exercising the
//! canonical sorting and flattening that make `Sum[a,b]` equal `Sum[b,a]`; L4
//! never reaches `zero (*) symbolic`, which is where the interesting case
//! lives; and L11 never reaches `MaxTerms` deduplication, which is where the
//! `.max()` mistake hides. Nothing can detect a strategy that under-covers.
//!
//! **The upgrade does not arrive.** Making the laws checkable *structurally*
//! requires the normaliser, which puts a hard cross-lane dependency on a
//! ticket in the same wave - the coupling the plan was trying to avoid - and
//! two of the laws are quantified over `exists c`, which no normaliser
//! decides.
//!
//! **The fix used here** is to compare **denotations over a fixed valuation
//! grid**, using [`crate::Bound::eval`] and [`crate::TotalValuation`], both of
//! which ship in wave 1. That removes the cross-lane dependency entirely, and
//! it catches the failures the constant-only plan could not: evaluating
//! `max(omega, x)` at `x = 3` gives `omega` against `3`.
//!
//! # What remains limited, and why
//!
//! * The grid is **finite**, so extensional agreement on it is a *necessary*
//!   condition, not a proof. The suite is a falsifier. This is the honest
//!   statement of what LAN-59 AC4 delivers, and it belongs in the AC.
//! * **L7 (antisymmetry)** and **L10 (star monotonicity)** are quantified over
//!   the canonical preorder's `exists c`. L7 is checked in the constructive
//!   universal form given on [`crate::dioid::Dioid`]; L10 is checked by
//!   denotation on the grid. Both are therefore checked on the grid's
//!   witnesses only.
//! * **L6** is decidable on both shipped carriers (only `Bottom (+) Bottom` is
//!   `Bottom`), so it is fully checked.
//! * A surviving `cargo-mutants` mutant remains the backstop, per CONTRIBUTING:
//!   a survivor means the grid is too small.

use crate::{
    dioid::Dioid, law_failure::LawFailure, lifted::Lifted, nat::Nat,
    total_valuation::TotalValuation,
};

/// Everything the law suite needs beyond [`Dioid`].
///
/// Separate from [`Dioid`] so the suite's machinery stays out of the release
/// dependency graph, but **required by the registry macro**, so a semiring
/// cannot be registered for `--resource` without being law-testable.
pub trait DioidLaws: Dioid {
    /// The fixed element grid. Deterministic, ordered, and identical on every
    /// run - never randomly sampled.
    ///
    /// It **must** include `zero`, `one`, the top of the lattice, at least one
    /// symbolic (variable-containing) element, and at least one element that
    /// witnesses non-idempotence when [`Dioid::PLUS_IDEMPOTENT`] is `false`.
    /// A generator that reaches only `zero` passes every law vacuously, and
    /// nothing but mutation testing can tell you that happened.
    fn grid() -> Vec<Self::Carrier>;

    /// The fixed valuation grid. Must include an all-zero point, an all-one
    /// point, at least one point with a large finite magnitude, and an
    /// all-`omega` point.
    fn valuations() -> Vec<TotalValuation>;

    /// The denotation of a carrier element at a valuation.
    ///
    /// This is what "equal" means in every law. [`Lifted<Nat>`] rather than
    /// `Nat` because the carriers have a bottom element, and because
    /// `Lifted<Nat>` **is** `Ord` (unlike `Lifted<Bound>`), which is what lets
    /// the suite check the ordering laws directly.
    fn denote(value: &Self::Carrier, at: &TotalValuation) -> Lifted<Nat>;
}

/// Runs L1-L11 for `D` over its fixed grids.
///
/// Emitted once per registered resource by the registry macro, which is how
/// "the law tests run for every instance" becomes mechanical rather than a
/// hand-maintained list.
///
/// # Errors
///
/// The first [`LawFailure`] found, naming the law and the offending elements.
pub fn check_dioid_laws<D: DioidLaws>() -> Result<(), LawFailure> {
    todo!()
}
