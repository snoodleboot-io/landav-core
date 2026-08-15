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
//!
//! # The grid must stay inside the algebra's *exact* regime (LAN-59)
//!
//! [`crate::Nat`] saturates to `omega` on overflow and lets `omega` absorb
//! unconditionally. Both rules are frozen and both are sound - they
//! over-approximate upwards - but together they mean the shipped algebras
//! satisfy L2 and L3 as **equations** only where no intermediate saturates
//! next to a zero. Two exact statements, both pinned by tests in
//! [`crate::b`]:
//!
//! * `times` (`B`): `(a (*) b) (*) c` and `a (*) (b (*) c)` disagree exactly
//!   when the literal product `a * b` leaves `u64` while `c` denotes `0`. The
//!   saturated `Const(omega)` then absorbs the zero, where the other grouping
//!   folds the zero first and is exact. `prod([2^40, 2^40])` against
//!   `Const(0)` is the smallest witness.
//! * `plus` (`B`): `a (*) (b (+) c)` and `(a (*) b) (+) (a (*) c)` disagree
//!   exactly when `a` denotes `0` and `b (+) c` leaves `u64` while `b` and `c`
//!   are both finite.
//!
//! Neither is a soundness defect - every disagreeing pair over-approximates -
//! but both are genuine failures of the *equation*, so a grid that reaches
//! them reports a law violation for an algebra this crate ships on purpose.
//! The grids therefore keep every literal's **square** inside `u64` and every
//! pair of finite denotations' **sum** inside `u64`, and
//! [`crate::b`] pins both boundaries with characterisation tests so the choice
//! is a recorded decision rather than a lucky constant. `MaxPlus` has neither
//! problem: its `times` is `+`, which only grows, so saturation is monotone on
//! both sides of every regrouping.

use crate::{
    dioid::Dioid, law::Law, law_failure::LawFailure, lifted::Lifted, nat::Nat,
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

/// How much of one law's work actually discriminated between two values.
///
/// The headline soundness property in this crate discharged 71.6% of its cases
/// vacuously until somebody measured it. A law suite has the same failure mode
/// in three distinct shapes, and this type counts all three:
///
/// * an **assertion** whose two sides are the *same carrier value* is
///   guaranteed to hold whatever [`DioidLaws::denote`] does, because the two
///   denotations are computed from one value. Canonical sorting and flattening
///   make that the common case for associativity and commutativity - which is
///   [`crate::Terms`] working as designed, not a defect - so the interesting
///   number is how many assertions are left over. That is
///   [`LawCoverage::discriminating`];
/// * an assertion that only ever compares an **absorbing** denotation -
///   `Bottom` at every valuation, or `omega` at every valuation - cannot tell
///   a correct implementation from one that returns the absorbing element
///   unconditionally. This is the shape that made the *inequational* star law
///   vacuous, and it is [`LawCoverage::informative`]'s complement;
/// * a **guarded** law (L6, L7, and L11's non-idempotence witness) whose
///   premise never fires is discharged vacuously in the ordinary sense. That
///   is [`LawCoverage::fired`].
///
/// # The arithmetic ceiling on `discriminating`, which is not obvious
///
/// It is bounded well below 1000 per mille and no grid can move it. L1 and L2
/// are `plus`- and `times`-associativity, and both `Bound::sum` and
/// `Bound::prod` flatten and canonically sort, so *both sides of every
/// associativity assertion are literally the same term* - by construction, for
/// every grid. Those two laws are roughly half of the suite's assertions and
/// contribute exactly zero. Read `discriminating` as "how much of the suite
/// needed [`crate::Bound::eval`] to decide it", not as a coverage percentage.
///
/// Counters saturate rather than wrapping; a wrapped counter would report a
/// better number than the truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LawCoverage {
    assertions: u64,
    discriminating: u64,
    informative: u64,
    guards: u64,
    fired: u64,
}

impl LawCoverage {
    /// Every assertion this law made.
    #[must_use]
    pub const fn assertions(self) -> u64 {
        self.assertions
    }

    /// The assertions whose two sides were **distinct carrier values**, so
    /// that evaluation - not structural equality - decided the outcome.
    #[must_use]
    pub const fn discriminating(self) -> u64 {
        self.discriminating
    }

    /// The assertions that reached a denotation which is **not** an absorbing
    /// element at some valuation.
    ///
    /// An assertion comparing `omega` with `omega` at every point of the grid
    /// is satisfied by an implementation that returns `omega` unconditionally,
    /// and so is one comparing `Bottom` with `Bottom`. Only the rest can
    /// falsify such a mutant.
    #[must_use]
    pub const fn informative(self) -> u64 {
        self.informative
    }

    /// The guarded cases this law considered.
    #[must_use]
    pub const fn guards(self) -> u64 {
        self.guards
    }

    /// The guarded cases whose premise actually held.
    #[must_use]
    pub const fn fired(self) -> u64 {
        self.fired
    }

    /// [`LawCoverage::discriminating`] over [`LawCoverage::assertions`], in
    /// parts per thousand.
    ///
    /// An integer ratio, because this crate contains **no floating point
    /// anywhere** and a coverage number that differs between two targets is
    /// not a gate.
    #[must_use]
    pub const fn discriminating_permille(self) -> u64 {
        permille(self.discriminating, self.assertions)
    }

    /// [`LawCoverage::informative`] over [`LawCoverage::assertions`], in parts
    /// per thousand.
    #[must_use]
    pub const fn informative_permille(self) -> u64 {
        permille(self.informative, self.assertions)
    }

    /// [`LawCoverage::fired`] over [`LawCoverage::guards`], in parts per
    /// thousand.
    #[must_use]
    pub const fn fired_permille(self) -> u64 {
        permille(self.fired, self.guards)
    }

    /// Component-wise addition, saturating.
    #[must_use]
    const fn merge(self, other: Self) -> Self {
        Self {
            assertions: self.assertions.saturating_add(other.assertions),
            discriminating: self.discriminating.saturating_add(other.discriminating),
            informative: self.informative.saturating_add(other.informative),
            guards: self.guards.saturating_add(other.guards),
            fired: self.fired.saturating_add(other.fired),
        }
    }
}

/// `true` for the two denotations an assertion cannot learn anything from:
/// the bottom and the top of the lattice.
const fn is_absorbing(value: Lifted<Nat>) -> bool {
    matches!(value, Lifted::Bottom | Lifted::Elem(Nat::Omega))
}

/// `part / whole`, in parts per thousand. `0` when `whole` is `0`, so an
/// unrun law reports no coverage rather than full coverage.
const fn permille(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    match part.checked_mul(1000) {
        Some(scaled) => scaled / whole,
        // Unreachable at any grid this crate can build, and reporting the
        // ceiling is the pessimistic direction for a *denominator* overflow.
        None => 1000,
    }
}

/// Per-law coverage for one instance, in [`Law::ALL`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LawReport {
    entries: Vec<(Law, LawCoverage)>,
}

impl LawReport {
    /// Every law's coverage, in [`Law::ALL`] order.
    #[must_use]
    pub fn entries(&self) -> &[(Law, LawCoverage)] {
        &self.entries
    }

    /// One law's coverage. A law that ran nothing reports zeroes.
    #[must_use]
    pub fn for_law(&self, law: Law) -> LawCoverage {
        self.entries
            .iter()
            .find(|(entry, _)| *entry == law)
            .map_or_else(LawCoverage::default, |(_, coverage)| *coverage)
    }

    /// The whole suite's coverage.
    #[must_use]
    pub fn total(&self) -> LawCoverage {
        self.entries
            .iter()
            .fold(LawCoverage::default(), |running, (_, coverage)| {
                running.merge(*coverage)
            })
    }
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
    measure_dioid_laws::<D>().map(|_| ())
}

/// [`check_dioid_laws`], and the per-law coverage it accumulated on the way.
///
/// Exposed because "the suite passed" and "the suite looked at anything" are
/// different claims, and only the second one is falsifiable by a number.
///
/// # Errors
///
/// The first [`LawFailure`] found, exactly as [`check_dioid_laws`] reports it.
pub fn measure_dioid_laws<D: DioidLaws>() -> Result<LawReport, LawFailure> {
    let mut suite = Suite::<D>::new();
    suite.run()?;
    Ok(suite.into_report())
}

/// The generic law runner: the grids, and the counters, in one place.
struct Suite<D: DioidLaws> {
    grid: Vec<D::Carrier>,
    valuations: Vec<TotalValuation>,
    coverage: Vec<(Law, LawCoverage)>,
}

impl<D: DioidLaws> Suite<D> {
    fn new() -> Self {
        Self {
            grid: D::grid(),
            valuations: D::valuations(),
            coverage: Law::ALL
                .iter()
                .map(|law| (*law, LawCoverage::default()))
                .collect(),
        }
    }

    fn into_report(self) -> LawReport {
        LawReport {
            entries: self.coverage,
        }
    }

    /// L1 through L11, in the frozen numbering order.
    ///
    /// The order is load bearing for the suite's own tests: an instance built
    /// to violate exactly one law must be reported against *that* law, which
    /// is only true if every earlier law is checked first.
    fn run(&mut self) -> Result<(), LawFailure> {
        self.plus_monoid()?;
        self.times_monoid()?;
        self.distributivity()?;
        self.annihilation()?;
        self.star_unfolding()?;
        self.zero_sum_freedom()?;
        self.antisymmetry()?;
        self.non_degeneracy()?;
        self.star_at_zero()?;
        self.star_monotonicity()?;
        self.idempotence()
    }

    // ---- the counting primitives ----

    /// Updates one law's counters. A law absent from [`Law::ALL`] is counted
    /// nowhere rather than panicking; `Law::ALL` is the authoritative list, so
    /// that cannot happen without a compile error elsewhere first.
    fn count(&mut self, law: Law, update: impl FnOnce(&mut LawCoverage)) {
        if let Some((_, coverage)) = self.coverage.iter_mut().find(|(entry, _)| *entry == law) {
            update(coverage);
        }
    }

    /// The index of the first valuation at which two elements' denotations
    /// differ, if any. **Not counted**: this is the premise machinery.
    fn divergence(&self, left: &D::Carrier, right: &D::Carrier) -> Option<usize> {
        self.valuations
            .iter()
            .position(|at| D::denote(left, at) != D::denote(right, at))
    }

    /// Records one assertion's coverage: whether its two sides were distinct
    /// carrier values, and whether it reached anything but the absorbing
    /// elements. One pass over the valuations, shared by all three
    /// `require_*` primitives.
    fn observe(&mut self, law: Law, left: &D::Carrier, right: &D::Carrier) {
        let distinct = left != right;
        let informative = self
            .valuations
            .iter()
            .any(|at| !is_absorbing(D::denote(left, at)) || !is_absorbing(D::denote(right, at)));
        self.count(law, |coverage| {
            coverage.assertions = coverage.assertions.saturating_add(1);
            if distinct {
                coverage.discriminating = coverage.discriminating.saturating_add(1);
            }
            if informative {
                coverage.informative = coverage.informative.saturating_add(1);
            }
        });
    }

    /// Extensional equality: agreement of the denotations at every valuation.
    fn agree(&self, left: &D::Carrier, right: &D::Carrier) -> bool {
        self.divergence(left, right).is_none()
    }

    fn failure(&self, law: Law, context: &str, detail: String) -> LawFailure {
        LawFailure {
            semiring: D::SEMIRING,
            law,
            detail: format!("{context}: {detail}"),
        }
    }

    /// Renders the disagreement at `index` for a diagnostic.
    fn rendered(&self, index: usize, left: &D::Carrier, right: &D::Carrier) -> String {
        match self.valuations.get(index) {
            Some(at) => format!(
                "left = {left:?}, right = {right:?}; at valuation #{index} {at:?} \
                 they denote {:?} and {:?}",
                D::denote(left, at),
                D::denote(right, at)
            ),
            // Unreachable: the index came from `divergence`, which indexes
            // this same slice.
            None => format!("left = {left:?}, right = {right:?}"),
        }
    }

    /// Asserts that two elements are extensionally equal, and counts whether
    /// they were distinct carrier values.
    fn require_equal(
        &mut self,
        law: Law,
        left: &D::Carrier,
        right: &D::Carrier,
        context: &str,
    ) -> Result<(), LawFailure> {
        self.observe(law, left, right);
        match self.divergence(left, right) {
            None => Ok(()),
            Some(index) => Err(self.failure(law, context, self.rendered(index, left, right))),
        }
    }

    /// Asserts that two elements are **not** extensionally equal.
    fn require_distinct(
        &mut self,
        law: Law,
        left: &D::Carrier,
        right: &D::Carrier,
        context: &str,
    ) -> Result<(), LawFailure> {
        self.observe(law, left, right);
        if self.agree(left, right) {
            return Err(self.failure(
                law,
                context,
                format!("{left:?} and {right:?} agree at every valuation"),
            ));
        }
        Ok(())
    }

    /// Asserts `lower <= upper` in the denotation's magnitude order, at every
    /// valuation. `Lifted<Nat>` is `Ord`; `Lifted<Bound>` deliberately is not.
    fn require_le(
        &mut self,
        law: Law,
        lower: &D::Carrier,
        upper: &D::Carrier,
        context: &str,
    ) -> Result<(), LawFailure> {
        self.observe(law, lower, upper);
        for (index, at) in self.valuations.iter().enumerate() {
            let (small, large) = (D::denote(lower, at), D::denote(upper, at));
            if small > large {
                return Err(self.failure(
                    law,
                    context,
                    format!(
                        "lower = {lower:?}, upper = {upper:?}; at valuation #{index} {at:?} \
                         they denote {small:?} and {large:?}"
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Records that a guarded case was considered, and whether it fired.
    fn guard(&mut self, law: Law, fired: bool) {
        self.count(law, |coverage| {
            coverage.guards = coverage.guards.saturating_add(1);
            if fired {
                coverage.fired = coverage.fired.saturating_add(1);
            }
        });
    }

    // ---- L1 .. L11 ----

    /// L1: `plus` is associative and commutative, with identity `zero`.
    fn plus_monoid(&mut self) -> Result<(), LawFailure> {
        let grid = self.grid.clone();
        let zero = D::zero();
        for a in &grid {
            self.require_equal(Law::PlusMonoid, &D::plus(a, &zero), a, "plus(a, zero) == a")?;
            self.require_equal(Law::PlusMonoid, &D::plus(&zero, a), a, "plus(zero, a) == a")?;
            for b in &grid {
                self.require_equal(
                    Law::PlusMonoid,
                    &D::plus(a, b),
                    &D::plus(b, a),
                    "plus(a, b) == plus(b, a)",
                )?;
                for c in &grid {
                    let left = D::plus(&D::plus(a, b), c);
                    let right = D::plus(a, &D::plus(b, c));
                    self.require_equal(
                        Law::PlusMonoid,
                        &left,
                        &right,
                        "plus(plus(a, b), c) == plus(a, plus(b, c))",
                    )?;
                }
            }
        }
        Ok(())
    }

    /// L2: `times` is associative, with identity `one`. **Not** commutative:
    /// nothing in the engine may assume it, so nothing here asserts it.
    fn times_monoid(&mut self) -> Result<(), LawFailure> {
        let grid = self.grid.clone();
        let one = D::one();
        for a in &grid {
            self.require_equal(
                Law::TimesMonoid,
                &D::times(a, &one),
                a,
                "times(a, one) == a",
            )?;
            self.require_equal(
                Law::TimesMonoid,
                &D::times(&one, a),
                a,
                "times(one, a) == a",
            )?;
            for b in &grid {
                for c in &grid {
                    let left = D::times(&D::times(a, b), c);
                    let right = D::times(a, &D::times(b, c));
                    self.require_equal(
                        Law::TimesMonoid,
                        &left,
                        &right,
                        "times(times(a, b), c) == times(a, times(b, c))",
                    )?;
                }
            }
        }
        Ok(())
    }

    /// L3: `times` distributes over `plus` on **both** sides. `times` is not
    /// assumed commutative, so one side does not imply the other.
    fn distributivity(&mut self) -> Result<(), LawFailure> {
        let grid = self.grid.clone();
        for a in &grid {
            for b in &grid {
                for c in &grid {
                    let left = D::times(a, &D::plus(b, c));
                    let right = D::plus(&D::times(a, b), &D::times(a, c));
                    self.require_equal(
                        Law::Distributivity,
                        &left,
                        &right,
                        "times(a, plus(b, c)) == plus(times(a, b), times(a, c))",
                    )?;

                    let left = D::times(&D::plus(a, b), c);
                    let right = D::plus(&D::times(a, c), &D::times(b, c));
                    self.require_equal(
                        Law::Distributivity,
                        &left,
                        &right,
                        "times(plus(a, b), c) == plus(times(a, c), times(b, c))",
                    )?;
                }
            }
        }
        Ok(())
    }

    /// L4: `times(zero, a) == zero == times(a, zero)`.
    fn annihilation(&mut self) -> Result<(), LawFailure> {
        let grid = self.grid.clone();
        let zero = D::zero();
        for a in &grid {
            self.require_equal(
                Law::Annihilation,
                &D::times(&zero, a),
                &zero,
                "times(zero, a) == zero",
            )?;
            self.require_equal(
                Law::Annihilation,
                &D::times(a, &zero),
                &zero,
                "times(a, zero) == zero",
            )?;
        }
        Ok(())
    }

    /// L5: star unfolds as an **equation**, on both sides.
    ///
    /// The inequation `star(a) >= plus(one, times(a, star(a)))` is vacuous -
    /// `omega` is the canonical top, so `star(a) = omega` satisfies it for
    /// every input and the mutant that drops the zero case survives.
    fn star_unfolding(&mut self) -> Result<(), LawFailure> {
        let grid = self.grid.clone();
        let one = D::one();
        for a in &grid {
            let starred = D::star(a);
            let left = D::plus(&one, &D::times(a, &starred));
            self.require_equal(
                Law::StarUnfolding,
                &starred,
                &left,
                "star(a) == plus(one, times(a, star(a)))",
            )?;
            let right = D::plus(&one, &D::times(&starred, a));
            self.require_equal(
                Law::StarUnfolding,
                &starred,
                &right,
                "star(a) == plus(one, times(star(a), a))",
            )?;
        }
        Ok(())
    }

    /// L6: `plus(a, b) == zero` implies `a == zero` and `b == zero`.
    fn zero_sum_freedom(&mut self) -> Result<(), LawFailure> {
        let grid = self.grid.clone();
        let zero = D::zero();
        for a in &grid {
            for b in &grid {
                let summed = D::plus(a, b);
                let fired = self.agree(&summed, &zero);
                self.guard(Law::ZeroSumFreedom, fired);
                if !fired {
                    continue;
                }
                self.require_equal(
                    Law::ZeroSumFreedom,
                    a,
                    &zero,
                    "plus(a, b) == zero implies a == zero",
                )?;
                self.require_equal(
                    Law::ZeroSumFreedom,
                    b,
                    &zero,
                    "plus(a, b) == zero implies b == zero",
                )?;
            }
        }
        Ok(())
    }

    /// L7: antisymmetry of the canonical preorder, in the constructive
    /// universal form: for all `a`, `c`, `d`, let `b = plus(a, c)`; if
    /// `plus(b, d) == a` then `a == b`.
    ///
    /// **This is the law that defines the trait.** It does not follow from
    /// zero-sum-freeness: `N / (n ~ n+2 for n >= 1)` is zero-sum-free and has
    /// two distinct mutually-preceding elements.
    fn antisymmetry(&mut self) -> Result<(), LawFailure> {
        let grid = self.grid.clone();
        for a in &grid {
            for c in &grid {
                let b = D::plus(a, c);
                for d in &grid {
                    let closed = D::plus(&b, d);
                    let fired = self.agree(&closed, a);
                    self.guard(Law::Antisymmetry, fired);
                    if !fired {
                        continue;
                    }
                    self.require_equal(
                        Law::Antisymmetry,
                        a,
                        &b,
                        "a <= b and b <= a imply a == b, with b = plus(a, c)",
                    )?;
                }
            }
        }
        Ok(())
    }

    /// L8: `zero() != one()`. The one-element semiring satisfies every other
    /// law and reports every program as costing nothing.
    fn non_degeneracy(&mut self) -> Result<(), LawFailure> {
        let (zero, one) = (D::zero(), D::one());
        self.require_distinct(Law::NonDegeneracy, &zero, &one, "zero() != one()")
    }

    /// L9: `star(zero) == one`. Kills the returns-`omega`-unconditionally
    /// mutant that L5 alone lets through.
    fn star_at_zero(&mut self) -> Result<(), LawFailure> {
        let (zero, one) = (D::zero(), D::one());
        self.require_equal(Law::StarAtZero, &D::star(&zero), &one, "star(zero) == one")
    }

    /// L10: `a <= b` implies `star(a) <= star(b)`.
    ///
    /// `b` is constructed as `plus(a, c)`, which is precisely the canonical
    /// preorder's witness, so the premise holds by construction and the check
    /// is universal over the grid rather than guarded. The conclusion's order
    /// is the denotation's magnitude order, which coincides with the canonical
    /// preorder on both shipped carriers.
    fn star_monotonicity(&mut self) -> Result<(), LawFailure> {
        let grid = self.grid.clone();
        for a in &grid {
            for c in &grid {
                let b = D::plus(a, c);
                let (lower, upper) = (D::star(a), D::star(&b));
                self.require_le(
                    Law::StarMonotonicity,
                    &lower,
                    &upper,
                    "a <= b implies star(a) <= star(b), with b = plus(a, c)",
                )?;
            }
        }
        Ok(())
    }

    /// L11: `plus(a, a) == a` for every `a` **iff** [`Dioid::PLUS_IDEMPOTENT`].
    ///
    /// Both directions. When the flag is `false` the suite requires a witness
    /// `a` with `plus(a, a) != a`, so an instance cannot quietly claim the
    /// wrong thing in either direction.
    fn idempotence(&mut self) -> Result<(), LawFailure> {
        let grid = self.grid.clone();
        if D::PLUS_IDEMPOTENT {
            for a in &grid {
                self.require_equal(
                    Law::Idempotence,
                    &D::plus(a, a),
                    a,
                    "PLUS_IDEMPOTENT is true, so plus(a, a) == a",
                )?;
            }
            return Ok(());
        }

        let mut witness: Option<D::Carrier> = None;
        for a in &grid {
            let doubled = D::plus(a, a);
            let differs = !self.agree(&doubled, a);
            self.guard(Law::Idempotence, differs);
            if differs && witness.is_none() {
                witness = Some(a.clone());
            }
        }
        match witness {
            Some(found) => {
                let doubled = D::plus(&found, &found);
                self.require_distinct(
                    Law::Idempotence,
                    &doubled,
                    &found,
                    "PLUS_IDEMPOTENT is false, so some a must have plus(a, a) != a",
                )
            }
            None => Err(self.failure(
                Law::Idempotence,
                "PLUS_IDEMPOTENT is false, so some a must have plus(a, a) != a",
                format!(
                    "no element of the {}-element grid witnesses non-idempotence",
                    self.grid.len()
                ),
            )),
        }
    }
}

#[cfg(test)]
mod apparatus {
    //! The suite checking itself.
    //!
    //! Every instance below is **deliberately wrong in exactly one way**, and
    //! each test names the law that must catch it. Three of the four are
    //! adversaries the design panel raised against the law *set*, not against
    //! any implementation: without them "the law set includes antisymmetry"
    //! and "claiming idempotence must fail" are claims about source text
    //! rather than about behaviour.

    use super::{
        DioidLaws, LawCoverage, LawReport, check_dioid_laws, measure_dioid_laws, permille,
    };
    use crate::{
        bound::Bound, canonical::Canonical, dioid::Dioid, law::Law, lifted::Lifted, nat::Nat,
        semiring_id::SemiringId, total_valuation::TotalValuation,
    };
    use std::collections::BTreeMap;

    /// A valuation grid good enough for a synthetic carrier whose denotation
    /// ignores the valuation entirely.
    fn trivial_valuations() -> Vec<TotalValuation> {
        vec![
            TotalValuation::with_default(BTreeMap::new(), Nat::ZERO),
            TotalValuation::with_default(BTreeMap::new(), Nat::OMEGA),
        ]
    }

    // -----------------------------------------------------------------
    // L8: the one-element semiring
    // -----------------------------------------------------------------

    /// `zero == one`, every operation returns `Bottom`, one-element grid.
    ///
    /// This is the semiring that "reports every program as costing nothing".
    /// It satisfies L1-L7 and L9-L11 outright, which is exactly why L8 has to
    /// exist as a separate law.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Degenerate {}

    impl Dioid for Degenerate {
        type Carrier = Lifted<Bound>;

        const SEMIRING: SemiringId = SemiringId::new("degenerate");
        const PLUS_IDEMPOTENT: bool = true;

        fn zero() -> Self::Carrier {
            Lifted::Bottom
        }
        fn one() -> Self::Carrier {
            Lifted::Bottom
        }
        fn plus(_a: &Self::Carrier, _b: &Self::Carrier) -> Self::Carrier {
            Lifted::Bottom
        }
        fn times(_a: &Self::Carrier, _b: &Self::Carrier) -> Self::Carrier {
            Lifted::Bottom
        }
        fn star(_a: &Self::Carrier) -> Self::Carrier {
            Lifted::Bottom
        }
    }

    impl DioidLaws for Degenerate {
        fn grid() -> Vec<Self::Carrier> {
            vec![Lifted::Bottom]
        }
        fn valuations() -> Vec<TotalValuation> {
            trivial_valuations()
        }
        fn denote(_value: &Self::Carrier, _at: &TotalValuation) -> Lifted<Nat> {
            Lifted::Bottom
        }
    }

    /// **AC5, half one.** `zero != one` does not follow from the other ten:
    /// here is a structure that satisfies all ten and is still useless.
    #[test]
    fn the_one_element_semiring_is_caught_by_l8_and_by_nothing_else() {
        let failure = check_dioid_laws::<Degenerate>();

        let Err(reported) = failure else {
            unreachable!("the one-element semiring must violate L8");
        };
        assert_eq!(reported.law, Law::NonDegeneracy);
        assert_eq!(reported.semiring, SemiringId::new("degenerate"));
        assert!(
            reported.to_string().contains("L8"),
            "the rendered failure must name the law: {reported}"
        );
    }

    // -----------------------------------------------------------------
    // L7: a zero-sum-free semiring whose canonical preorder is not
    // antisymmetric
    // -----------------------------------------------------------------

    /// `N / (n ~ n+2 for n >= 1)`, with a top adjoined so that `star` has a
    /// solution.
    ///
    /// `Odd + Odd = Even` and `Even + Odd = Odd`, so `Odd <= Even` and
    /// `Even <= Odd` while `Odd != Even`. Every other law holds: the quotient
    /// is a genuine commutative semiring, it is zero-sum-free, `zero != one`,
    /// and `Top` makes `star` an exact solution of the unfolding equation.
    ///
    /// This is the "parity of allocations" or "mod-k counting" carrier the
    /// design panel warned about, in the smallest form that still has a star.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum Parity {
        /// The class of `0`. The additive identity.
        Nil,
        /// The class of the odd naturals. The multiplicative identity.
        Odd,
        /// The class of the even naturals `>= 2`.
        Even,
        /// An adjoined top, absorbing under both operations except against
        /// `Nil` under `times`.
        Top,
    }

    impl Canonical for Parity {
        fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
            self.tag().cmp(&other.tag())
        }
        fn write_canonical(&self, out: &mut Vec<u8>) {
            out.push(self.tag());
        }
    }

    impl Parity {
        const fn tag(self) -> u8 {
            match self {
                Self::Nil => 0,
                Self::Odd => 1,
                Self::Even => 2,
                Self::Top => 3,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum NotAntisymmetric {}

    impl Dioid for NotAntisymmetric {
        type Carrier = Parity;

        const SEMIRING: SemiringId = SemiringId::new("parity");
        const PLUS_IDEMPOTENT: bool = false;

        fn zero() -> Self::Carrier {
            Parity::Nil
        }

        fn one() -> Self::Carrier {
            Parity::Odd
        }

        fn plus(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
            match (a, b) {
                (Parity::Top, _) | (_, Parity::Top) => Parity::Top,
                (Parity::Nil, other) | (other, Parity::Nil) => *other,
                (Parity::Odd, Parity::Odd) | (Parity::Even, Parity::Even) => Parity::Even,
                (Parity::Odd, Parity::Even) | (Parity::Even, Parity::Odd) => Parity::Odd,
            }
        }

        fn times(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
            match (a, b) {
                (Parity::Nil, _) | (_, Parity::Nil) => Parity::Nil,
                (Parity::Top, _) | (_, Parity::Top) => Parity::Top,
                (Parity::Odd, other) | (other, Parity::Odd) => *other,
                (Parity::Even, Parity::Even) => Parity::Even,
            }
        }

        fn star(a: &Self::Carrier) -> Self::Carrier {
            match a {
                Parity::Nil => Parity::Odd,
                Parity::Odd | Parity::Even | Parity::Top => Parity::Top,
            }
        }
    }

    impl DioidLaws for NotAntisymmetric {
        fn grid() -> Vec<Self::Carrier> {
            vec![Parity::Nil, Parity::Odd, Parity::Even, Parity::Top]
        }
        fn valuations() -> Vec<TotalValuation> {
            trivial_valuations()
        }
        fn denote(value: &Self::Carrier, _at: &TotalValuation) -> Lifted<Nat> {
            // Injective, so extensional equality on this carrier is equality.
            match value {
                Parity::Nil => Lifted::Bottom,
                Parity::Odd => Lifted::Elem(Nat::Fin(1)),
                Parity::Even => Lifted::Elem(Nat::Fin(2)),
                Parity::Top => Lifted::Elem(Nat::OMEGA),
            }
        }
    }

    /// **AC5, half two.** Antisymmetry does not follow from zero-sum-freeness.
    ///
    /// The failure must be reported against L7 specifically: if it were
    /// reported against L1, L2, L3 or L6 the counterexample would be wrong and
    /// the independence argument would not hold.
    #[test]
    fn a_zero_sum_free_non_antisymmetric_semiring_is_caught_by_l7() {
        let Err(reported) = check_dioid_laws::<NotAntisymmetric>() else {
            unreachable!("the parity quotient must violate L7");
        };
        assert_eq!(
            reported.law,
            Law::Antisymmetry,
            "expected L7, got {reported}"
        );
        assert_eq!(reported.semiring, SemiringId::new("parity"));
    }

    /// The parity quotient is only a counterexample to L7 if it really does
    /// satisfy the laws L7 is supposed to be independent of. Checked directly,
    /// so that a bug in the counterexample cannot masquerade as independence.
    #[test]
    fn the_parity_quotient_satisfies_every_law_up_to_l7() {
        let mut suite = super::Suite::<NotAntisymmetric>::new();
        assert!(suite.plus_monoid().is_ok(), "L1 must hold");
        assert!(suite.times_monoid().is_ok(), "L2 must hold");
        assert!(suite.distributivity().is_ok(), "L3 must hold");
        assert!(suite.annihilation().is_ok(), "L4 must hold");
        assert!(suite.star_unfolding().is_ok(), "L5 must hold");
        assert!(suite.zero_sum_freedom().is_ok(), "L6 must hold");
        assert!(suite.antisymmetry().is_err(), "L7 must fail");
        // And the laws after L7 are satisfied too, so L7 is the only one it
        // breaks.
        assert!(suite.non_degeneracy().is_ok(), "L8 must hold");
        assert!(suite.star_at_zero().is_ok(), "L9 must hold");
        assert!(suite.star_monotonicity().is_ok(), "L10 must hold");
        assert!(suite.idempotence().is_ok(), "L11 must hold");
    }

    // -----------------------------------------------------------------
    // L11: the flag, in both directions
    // -----------------------------------------------------------------

    /// `B`'s algebra with the idempotence flag flipped to `true`.
    ///
    /// The marker-trait design this replaces could not catch it:
    /// `impl IdempotentDioid for B {}` compiles while `1 + 1 = 2`, and nothing
    /// runs.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ClaimsIdempotent {}

    impl Dioid for ClaimsIdempotent {
        type Carrier = Lifted<Bound>;

        const SEMIRING: SemiringId = SemiringId::new("additive-mislabelled");
        const PLUS_IDEMPOTENT: bool = true;

        fn zero() -> Self::Carrier {
            crate::b::B::zero()
        }
        fn one() -> Self::Carrier {
            crate::b::B::one()
        }
        fn plus(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
            crate::b::B::plus(a, b)
        }
        fn times(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
            crate::b::B::times(a, b)
        }
        fn star(a: &Self::Carrier) -> Self::Carrier {
            crate::b::B::star(a)
        }
    }

    impl DioidLaws for ClaimsIdempotent {
        fn grid() -> Vec<Self::Carrier> {
            <crate::b::B as DioidLaws>::grid()
        }
        fn valuations() -> Vec<TotalValuation> {
            <crate::b::B as DioidLaws>::valuations()
        }
        fn denote(value: &Self::Carrier, at: &TotalValuation) -> Lifted<Nat> {
            <crate::b::B as DioidLaws>::denote(value, at)
        }
    }

    /// `MaxPlus`'s algebra with the idempotence flag flipped to `false`.
    ///
    /// The other direction: an instance that *is* idempotent but does not say
    /// so. Without the witness requirement this would pass silently, and the
    /// flag would be decorative in one direction.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum DeniesIdempotent {}

    impl Dioid for DeniesIdempotent {
        type Carrier = Lifted<Bound>;

        const SEMIRING: SemiringId = SemiringId::new("peak-mislabelled");
        const PLUS_IDEMPOTENT: bool = false;

        fn zero() -> Self::Carrier {
            crate::max_plus::MaxPlus::zero()
        }
        fn one() -> Self::Carrier {
            crate::max_plus::MaxPlus::one()
        }
        fn plus(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
            crate::max_plus::MaxPlus::plus(a, b)
        }
        fn times(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
            crate::max_plus::MaxPlus::times(a, b)
        }
        fn star(a: &Self::Carrier) -> Self::Carrier {
            crate::max_plus::MaxPlus::star(a)
        }
    }

    impl DioidLaws for DeniesIdempotent {
        fn grid() -> Vec<Self::Carrier> {
            <crate::max_plus::MaxPlus as DioidLaws>::grid()
        }
        fn valuations() -> Vec<TotalValuation> {
            <crate::max_plus::MaxPlus as DioidLaws>::valuations()
        }
        fn denote(value: &Self::Carrier, at: &TotalValuation) -> Lifted<Nat> {
            <crate::max_plus::MaxPlus as DioidLaws>::denote(value, at)
        }
    }

    /// **AC6.** Claiming idempotence without satisfying it fails.
    ///
    /// The failure's rendering is pinned here too, because this is the only
    /// path in the suite that reports a *divergence* rather than a missing
    /// witness. A law suite whose diagnostic says only "L11 failed" costs a
    /// debugging session per failure, and nothing else in the crate exercises
    /// the renderer.
    #[test]
    fn claiming_idempotence_without_satisfying_it_fails_l11() {
        let Err(reported) = check_dioid_laws::<ClaimsIdempotent>() else {
            unreachable!("the additive algebra is not idempotent: 1 + 1 = 2");
        };
        assert_eq!(
            reported.law,
            Law::Idempotence,
            "expected L11, got {reported}"
        );
        assert!(
            reported.detail.contains("PLUS_IDEMPOTENT is true"),
            "the failure must quote the law it checked: {reported}"
        );
        assert!(
            reported.detail.contains("at valuation #"),
            "the failure must name the valuation that separated the two sides: {reported}"
        );
        assert!(
            reported.detail.contains("they denote"),
            "the failure must show the two denotations: {reported}"
        );
        // `plus(Elem(1), Elem(1))` is `Elem(2)`: the first grid element that
        // is not idempotent, and the numbers a reader needs.
        assert!(
            reported.detail.contains("Fin(2)") && reported.detail.contains("Fin(1)"),
            "the failure must carry the offending magnitudes: {reported}"
        );
    }

    /// **AC6, the other direction.** Denying idempotence while satisfying it
    /// fails too, so the flag cannot be wrong in either direction.
    #[test]
    fn denying_idempotence_while_satisfying_it_fails_l11() {
        let Err(reported) = check_dioid_laws::<DeniesIdempotent>() else {
            unreachable!("max is idempotent, so no non-idempotence witness exists");
        };
        assert_eq!(
            reported.law,
            Law::Idempotence,
            "expected L11, got {reported}"
        );
        assert!(
            reported.detail.contains("witnesses non-idempotence"),
            "the failure must say a witness is missing: {reported}"
        );
    }

    // -----------------------------------------------------------------
    // the counters
    // -----------------------------------------------------------------

    /// `observe` counts an assertion as informative when **either** side
    /// leaves the absorbing elements.
    ///
    /// `&&` in place of `||` would under-count exactly the pairs that compare
    /// a real magnitude against the top - which is the most interesting
    /// comparison the suite makes, and the one that catches an implementation
    /// widening to `omega` where it should not.
    #[test]
    fn observe_counts_a_one_sided_escape_from_the_absorbing_elements() {
        let finite = Lifted::Elem(Bound::one());
        let top = Lifted::Elem(Bound::omega());
        let bottom: Lifted<Bound> = Lifted::Bottom;

        let mut suite = super::Suite::<crate::b::B>::new();
        suite.observe(Law::PlusMonoid, &finite, &top);
        suite.observe(Law::TimesMonoid, &top, &finite);
        suite.observe(Law::Distributivity, &top, &bottom);
        suite.observe(Law::Annihilation, &finite, &finite);
        let report = suite.into_report();

        assert_eq!(
            report.for_law(Law::PlusMonoid).informative(),
            1,
            "the left side denotes 1, which is neither bottom nor omega"
        );
        assert_eq!(
            report.for_law(Law::TimesMonoid).informative(),
            1,
            "the right side denotes 1"
        );
        assert_eq!(
            report.for_law(Law::Distributivity).informative(),
            0,
            "omega against bottom cannot falsify anything"
        );
        assert_eq!(report.for_law(Law::Annihilation).informative(), 1);

        // `discriminating` is about the carrier values, not the denotations,
        // so the two counters must not move together.
        assert_eq!(report.for_law(Law::PlusMonoid).discriminating(), 1);
        assert_eq!(
            report.for_law(Law::Annihilation).discriminating(),
            0,
            "one value compared with itself decides nothing"
        );
        assert_eq!(report.for_law(Law::Annihilation).assertions(), 1);
    }

    #[test]
    fn permille_is_total_and_reports_nothing_for_an_empty_denominator() {
        assert_eq!(permille(0, 0), 0);
        assert_eq!(permille(7, 0), 0);
        assert_eq!(permille(1, 2), 500);
        assert_eq!(permille(1, 3), 333);
        assert_eq!(permille(3, 3), 1000);
        assert_eq!(permille(0, 9), 0);
        // The overflow arm: reachable only for a denominator no grid can
        // build, and it must saturate up rather than wrap to a small number.
        assert_eq!(permille(u64::MAX, u64::MAX), 1000);
    }

    #[test]
    fn coverage_accessors_report_what_was_counted() {
        let coverage = LawCoverage {
            assertions: 8,
            discriminating: 2,
            informative: 6,
            guards: 5,
            fired: 1,
        };
        assert_eq!(coverage.assertions(), 8);
        assert_eq!(coverage.discriminating(), 2);
        assert_eq!(coverage.informative(), 6);
        assert_eq!(coverage.guards(), 5);
        assert_eq!(coverage.fired(), 1);
        assert_eq!(coverage.discriminating_permille(), 250);
        assert_eq!(coverage.informative_permille(), 750);
        assert_eq!(coverage.fired_permille(), 200);
        assert_eq!(LawCoverage::default().assertions(), 0);
        assert_eq!(LawCoverage::default().discriminating_permille(), 0);
        assert_eq!(LawCoverage::default().informative_permille(), 0);
        assert_eq!(LawCoverage::default().fired_permille(), 0);
    }

    #[test]
    fn coverage_merges_component_wise_and_saturates() {
        let left = LawCoverage {
            assertions: 1,
            discriminating: 2,
            informative: 5,
            guards: 3,
            fired: 4,
        };
        let right = LawCoverage {
            assertions: 10,
            discriminating: 20,
            informative: 50,
            guards: 30,
            fired: 40,
        };
        let merged = left.merge(right);
        assert_eq!(merged.assertions(), 11);
        assert_eq!(merged.discriminating(), 22);
        assert_eq!(merged.informative(), 55);
        assert_eq!(merged.guards(), 33);
        assert_eq!(merged.fired(), 44);

        let huge = LawCoverage {
            assertions: u64::MAX,
            discriminating: u64::MAX,
            informative: u64::MAX,
            guards: u64::MAX,
            fired: u64::MAX,
        };
        assert_eq!(huge.merge(huge), huge, "counters saturate, never wrap");
    }

    #[test]
    fn a_report_covers_every_law_and_totals_them() {
        let Ok(report) = measure_dioid_laws::<crate::b::B>() else {
            unreachable!("B must satisfy every law");
        };
        assert_eq!(
            report.entries().len(),
            Law::ALL.len(),
            "every law gets an entry, even one that asserts nothing"
        );
        for (index, (law, _)) in report.entries().iter().enumerate() {
            assert_eq!(
                Law::ALL.get(index),
                Some(law),
                "entries are in Law::ALL order"
            );
        }
        let total = report.total();
        let summed = report
            .entries()
            .iter()
            .fold(0u64, |running, (_, coverage)| {
                running + coverage.assertions()
            });
        assert_eq!(total.assertions(), summed);
        assert_eq!(
            report.for_law(Law::NonDegeneracy).assertions(),
            1,
            "L8 makes exactly one assertion"
        );
        assert_eq!(
            report.for_law(Law::StarAtZero).assertions(),
            1,
            "L9 makes exactly one assertion"
        );
    }

    #[test]
    fn for_law_reports_zeroes_for_a_law_that_is_not_in_the_report() {
        let report = LawReport {
            entries: vec![(Law::PlusMonoid, LawCoverage::default())],
        };
        assert_eq!(report.for_law(Law::Idempotence), LawCoverage::default());
        assert_eq!(report.entries().len(), 1);
        assert_eq!(report.total(), LawCoverage::default());
    }

    // -----------------------------------------------------------------
    // non-vacuity: what fraction of the suite's work actually discriminated
    // -----------------------------------------------------------------

    /// Measures one instance and prints the per-law numbers, so the coverage
    /// claim lives in the test log rather than in a reviewer's memory.
    fn measured<D: DioidLaws>(name: &str) -> LawReport {
        let Ok(report) = measure_dioid_laws::<D>() else {
            unreachable!("{name} must satisfy every law");
        };
        for (law, coverage) in report.entries() {
            println!(
                "{name} {law}: {}/{} discriminating ({} permille); \
                 {}/{} informative ({} permille); \
                 {}/{} guards fired ({} permille)",
                coverage.discriminating(),
                coverage.assertions(),
                coverage.discriminating_permille(),
                coverage.informative(),
                coverage.assertions(),
                coverage.informative_permille(),
                coverage.fired(),
                coverage.guards(),
                coverage.fired_permille(),
            );
        }
        let total = report.total();
        println!(
            "{name} TOTAL: {}/{} discriminating ({} permille); \
             {}/{} informative ({} permille); \
             {}/{} guards fired ({} permille)",
            total.discriminating(),
            total.assertions(),
            total.discriminating_permille(),
            total.informative(),
            total.assertions(),
            total.informative_permille(),
            total.fired(),
            total.guards(),
            total.fired_permille(),
        );
        report
    }

    /// Every law must actually *do* something for every instance: either make
    /// an assertion or fire a guard. A law whose quantifier ranges over an
    /// empty grid passes, silently, forever.
    fn assert_no_law_is_idle(report: &LawReport, name: &str) {
        for (law, coverage) in report.entries() {
            assert!(
                coverage.assertions() > 0,
                "{name} {law} asserted nothing at all"
            );
        }
    }

    /// The thresholds asserted below, and the arithmetic that fixes them.
    ///
    /// **These numbers were corrected once, downwards, and the correction is
    /// deliberate rather than an accommodation.** They were first written at
    /// 400 per mille total and 800 per mille for L3, before anything had been
    /// measured. Both are *arithmetically unreachable*, for a reason that has
    /// nothing to do with the grid:
    ///
    /// * L1 and L2 assert associativity of `plus` and `times`. `Bound::sum`
    ///   and `Bound::prod` flatten and canonically sort, so both sides of
    ///   every associativity assertion are the **same term** - for every grid,
    ///   forever. Those two laws are 5740 of the suite's 11770 assertions and
    ///   contribute exactly zero discriminating cases. The total is therefore
    ///   capped near 414 per mille even if every other assertion were perfect.
    /// * L3 quantifies over triples. Any triple containing `zero` makes both
    ///   sides the same term - `zero` annihilates on one side and is the `plus`
    ///   unit on the other - so L3's own ceiling is `(n-1)^3 / n^3`, which is
    ///   801 per mille at `n = 14`. The 800 written first was the ceiling, not
    ///   a target.
    ///
    /// The replacements are set just under the measured values, which is what
    /// makes them regression detectors. The *informative* thresholds, which
    /// count assertions that reached something other than the two absorbing
    /// elements, are new and strictly additional.
    const TOTAL_DISCRIMINATING_FLOOR: u64 = 240;
    const DISTRIBUTIVITY_DISCRIMINATING_FLOOR: u64 = 500;
    const TOTAL_INFORMATIVE_FLOOR: u64 = 690;
    const DISTRIBUTIVITY_INFORMATIVE_FLOOR: u64 = 700;

    /// The thresholds shared by both instances, applied to one report.
    fn assert_not_vacuous(report: &LawReport, name: &str) {
        assert_no_law_is_idle(report, name);

        let total = report.total();
        assert!(
            total.discriminating_permille() >= TOTAL_DISCRIMINATING_FLOOR,
            "{name} discriminated on only {} permille of {} assertions",
            total.discriminating_permille(),
            total.assertions()
        );
        assert!(
            total.informative_permille() >= TOTAL_INFORMATIVE_FLOOR,
            "{name} reached something other than an absorbing element on only \
             {} permille of {} assertions",
            total.informative_permille(),
            total.assertions()
        );

        // L3 is the law a constant-only strategy leaves entirely unchecked,
        // because both sides fold to the same constant. It must be the most
        // discriminating law in the suite, not the least.
        let distributivity = report.for_law(Law::Distributivity);
        assert!(
            distributivity.discriminating_permille() >= DISTRIBUTIVITY_DISCRIMINATING_FLOOR,
            "{name} L3 discriminated on only {} permille",
            distributivity.discriminating_permille()
        );
        assert!(
            distributivity.informative_permille() >= DISTRIBUTIVITY_INFORMATIVE_FLOOR,
            "{name} L3 was informative on only {} permille",
            distributivity.informative_permille()
        );

        // L5 is the star *equation*. Almost all of it compares `omega` with
        // `omega`, which is exactly why the *inequation* was vacuous: an
        // unconditional `omega` satisfies it everywhere. What saves the
        // equation is the handful of cases at the two zeros, and there must be
        // some.
        assert!(
            report.for_law(Law::StarUnfolding).informative() >= 4,
            "{name} L5 never left the top of the lattice, so it cannot falsify \
             a star that returns omega unconditionally"
        );

        // The guarded laws must have fired, or their conclusions were never
        // reached.
        assert!(
            report.for_law(Law::ZeroSumFreedom).fired() > 0,
            "{name} L6 never fired"
        );
        assert!(
            report.for_law(Law::Antisymmetry).fired() > 0,
            "{name} L7 never fired"
        );
    }

    /// **The measured non-vacuity of `B`.**
    ///
    /// "Discriminating" means the assertion's two sides were *distinct carrier
    /// values*, so [`DioidLaws::denote`] decided the outcome. "Informative"
    /// means it reached a denotation other than `Bottom` or `omega` somewhere,
    /// so it could falsify an implementation that returns an absorbing element
    /// unconditionally.
    #[test]
    fn the_additive_suite_is_not_vacuous() {
        let report = measured::<crate::b::B>("B");
        assert_not_vacuous(&report, "B");

        // `B` denies idempotence, so L11 must find a witness.
        assert!(
            report.for_law(Law::Idempotence).fired() > 0,
            "L11 found no non-idempotence witness"
        );
    }

    /// **The measured non-vacuity of `MaxPlus`.**
    #[test]
    fn the_peak_suite_is_not_vacuous() {
        let report = measured::<crate::max_plus::MaxPlus>("MaxPlus");
        assert_not_vacuous(&report, "MaxPlus");

        // `MaxPlus` claims idempotence, so L11 asserts rather than guards.
        assert_eq!(
            report.for_law(Law::Idempotence).guards(),
            0,
            "an idempotent instance has nothing to guard"
        );
        assert!(report.for_law(Law::Idempotence).assertions() > 0);
    }

    /// **Two laws are structurally absorbing-only, and that is not a defect
    /// the grid can fix.**
    ///
    /// L4 asserts `times(zero, a) == zero`, and `zero` denotes `Bottom` at
    /// every valuation by definition, so both sides are always `Bottom`. L6's
    /// premise only fires at `plus(zero, zero)`, for the same reason. Neither
    /// can falsify a `times` that returns `Bottom` unconditionally - L2's
    /// identity law is what does that. Recorded here so the zero in the report
    /// is a known fact rather than an unexamined one.
    #[test]
    fn l4_and_l6_can_only_ever_compare_the_bottom() {
        for report in [
            measured::<crate::b::B>("B"),
            measured::<crate::max_plus::MaxPlus>("MaxPlus"),
        ] {
            assert_eq!(report.for_law(Law::Annihilation).informative(), 0);
            assert!(report.for_law(Law::Annihilation).assertions() > 0);
            assert_eq!(report.for_law(Law::ZeroSumFreedom).informative(), 0);
            assert_eq!(
                report.for_law(Law::ZeroSumFreedom).fired(),
                1,
                "only plus(zero, zero) is zero, and zero-sum-freeness says so"
            );
        }
    }

    /// A grid whose every element is `zero` passes every law vacuously. The
    /// suite cannot reject it - the one-element semiring is a real structure -
    /// but the *measurement* must show it, which is what makes the thresholds
    /// above worth having.
    #[test]
    fn a_degenerate_grid_measures_as_degenerate() {
        // `Degenerate` fails L8, so measure the laws before it directly.
        let mut suite = super::Suite::<Degenerate>::new();
        assert!(suite.plus_monoid().is_ok());
        assert!(suite.distributivity().is_ok());
        let report = suite.into_report();
        assert_eq!(
            report.for_law(Law::Distributivity).discriminating(),
            0,
            "a one-element grid can never compare two distinct values"
        );
        assert_eq!(
            report
                .for_law(Law::Distributivity)
                .discriminating_permille(),
            0
        );
    }

    // -----------------------------------------------------------------
    // AC4: every registered instance
    // -----------------------------------------------------------------

    /// **AC4.** The registry emits `check_dioid_laws::<_>()` for every entry,
    /// so a semiring cannot be registered for `--resource` without inheriting
    /// the whole suite.
    #[test]
    fn every_registered_resource_satisfies_every_law() {
        let outcome = crate::registry::check_all_registered_laws();
        assert!(
            outcome.is_ok(),
            "a registered semiring violates a law: {outcome:?}"
        );
    }
}
