//! Gate 2 algebra adversary: the regressions that pin what the wave-2 claims
//! actually mean.
//!
//! # The instrument
//!
//! Every claim in this file rests on one thing the crate cannot express about
//! itself. [`Nat`] saturates at `u64`, so `Nat::Omega` means *either* "no
//! finite bound was established" *or* "a genuinely finite magnitude left the
//! carrier". [`Bound::eval`] therefore cannot tell a sound answer from a loose
//! one, and neither can a test written against it.
//!
//! [`Ideal`] below is the same operator table - the *frozen* one, `omega`
//! absorbing unconditionally and all - computed over `u128` with an explicit
//! `Big` for "finite, but past `u128`". It is the reference the shipped
//! algebra over-approximates. With it, "is this divergence upward" becomes a
//! decidable question instead of a claim.
//!
//! # What is pinned here
//!
//! | claim | test |
//! |---|---|
//! | `times`/`plus` regroupings outside the exact regime diverge **upward** | `every_regrouping_over_approximates_the_ideal_value` |
//! | the two pinned witnesses are upward, with their ideal values named | `the_two_pinned_divergences_are_both_upward` |
//! | `Bound::eval` never dips below the ideal | `eval_never_dips_below_the_ideal_value` |
//! | LAN-58 normalisation moves the denotation in **both** directions | `normalisation_moves_the_denotation_in_both_directions` |
//! | ... and never below the ideal | `normalisation_never_dips_below_the_ideal_value` |
//! | substituting and normalising do **not** commute | `substituting_before_normalising_is_tighter_than_after` |
//! | LAN-57 composition never dips below the ideal | `composition_never_dips_below_the_ideal_value` |
//! | `Const(0)` is the only bound denoting `0` everywhere | `const_zero_is_the_only_bound_denoting_zero_everywhere` |
//! | the six un-mutatable `subst` bodies | `subst_*` |

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use landav_bound::{
    b::B,
    base::Base,
    bound::Bound,
    bound_kind::BoundKind,
    dioid::Dioid,
    lifted::Lifted,
    max_plus::MaxPlus,
    nat::Nat,
    normalise::{NormaliserStop, normalise, normalise_with},
    normaliser_budget::NormaliserBudget,
    subst::Substitution,
    total_valuation::TotalValuation,
    trans_kind::TransKind,
    valuation::Valuation,
    var_id::VarId,
};

// ---------------------------------------------------------------------------
// the ideal semantics
// ---------------------------------------------------------------------------

/// The crate's frozen operator table, computed without the `u64` ceiling.
///
/// `Big` is "finite, but past `u128`". It is **not** `Omega`: `0 * Big` is
/// `0`, exactly as `0 * 5` is, whereas `0 * Omega` is `Omega` by the frozen
/// [`Nat::times`] rule. Collapsing the two would make this reference agree
/// with the implementation by construction and prove nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ideal {
    /// A finite magnitude that fits in `u128`.
    Fin(u128),
    /// Finite, but larger than `u128::MAX` - so certainly larger than
    /// `u64::MAX`.
    Big,
    /// The top of the lattice.
    Omega,
}

impl Ideal {
    fn plus(self, other: Self) -> Self {
        match (self, other) {
            (Self::Omega, _) | (_, Self::Omega) => Self::Omega,
            (Self::Big, _) | (_, Self::Big) => Self::Big,
            (Self::Fin(a), Self::Fin(b)) => a.checked_add(b).map_or(Self::Big, Self::Fin),
        }
    }

    fn times(self, other: Self) -> Self {
        match (self, other) {
            // `omega` absorbs unconditionally, including against zero. That is
            // the frozen rule, and the reference must obey it or it would be
            // measuring a different algebra.
            (Self::Omega, _) | (_, Self::Omega) => Self::Omega,
            // A finite zero times a finite huge value is zero, however huge.
            (Self::Fin(0), _) | (_, Self::Fin(0)) => Self::Fin(0),
            (Self::Big, _) | (_, Self::Big) => Self::Big,
            (Self::Fin(a), Self::Fin(b)) => a.checked_mul(b).map_or(Self::Big, Self::Fin),
        }
    }

    fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Omega, _) | (_, Self::Omega) => Self::Omega,
            (Self::Big, _) | (_, Self::Big) => Self::Big,
            (Self::Fin(a), Self::Fin(b)) => Self::Fin(a.max(b)),
        }
    }

    fn exp_of(self, base: u32) -> Self {
        match self {
            Self::Omega => Self::Omega,
            Self::Big => Self::Big,
            Self::Fin(exponent) => {
                let mut reached: u128 = 1;
                let step = u128::from(base);
                let mut taken: u128 = 0;
                while taken < exponent {
                    match reached.checked_mul(step) {
                        Some(next) => reached = next,
                        None => return Self::Big,
                    }
                    taken += 1;
                }
                Self::Fin(reached)
            }
        }
    }

    fn ceil_log(self, base: u32) -> Self {
        match self {
            Self::Omega => Self::Omega,
            // `ceil_log` of a finite value is finite and small; `Big` inputs do
            // not arise from these generators, and reporting `Big` is the
            // conservative direction for a *reference*.
            Self::Big => Self::Big,
            Self::Fin(value) => {
                let target = value.max(1);
                let step = u128::from(base);
                let mut reached: u128 = 1;
                let mut exponent: u128 = 0;
                while reached < target {
                    exponent += 1;
                    match reached.checked_mul(step) {
                        Some(next) => reached = next,
                        None => return Self::Fin(exponent),
                    }
                }
                Self::Fin(exponent)
            }
        }
    }

    fn of_nat(value: Nat) -> Self {
        match value {
            Nat::Fin(magnitude) => Self::Fin(u128::from(magnitude)),
            Nat::Omega => Self::Omega,
        }
    }

    /// `true` iff `reported` - what the shipped algebra computed in `u64`
    /// land - is a sound over-approximation of this ideal value.
    fn is_over_approximated_by(self, reported: Nat) -> bool {
        match (self, reported) {
            (_, Nat::Omega) => true,
            (Self::Omega | Self::Big, Nat::Fin(_)) => false,
            (Self::Fin(ideal), Nat::Fin(got)) => u128::from(got) >= ideal,
        }
    }
}

/// The ideal denotation of a bound, with variables read through `lookup`.
fn ideal_with(bound: &Bound, lookup: &dyn Fn(&VarId) -> Ideal) -> Ideal {
    match bound.kind() {
        BoundKind::Const(magnitude) => Ideal::of_nat(*magnitude),
        BoundKind::Var(var) => lookup(var),
        BoundKind::Sum(terms) => terms
            .as_slice()
            .iter()
            .fold(Ideal::Fin(0), |running, term| {
                running.plus(ideal_with(term, lookup))
            }),
        BoundKind::Max(terms) => terms
            .as_slice()
            .iter()
            .fold(Ideal::Fin(0), |running, term| {
                running.join(ideal_with(term, lookup))
            }),
        BoundKind::Prod(terms) => terms
            .as_slice()
            .iter()
            .fold(Ideal::Fin(1), |running, term| {
                running.times(ideal_with(term, lookup))
            }),
        BoundKind::Trans { kind, base, arg } => {
            let inner = ideal_with(arg, lookup);
            match kind {
                TransKind::Pow => inner.exp_of(base.get()),
                TransKind::Log => inner.ceil_log(base.get()),
            }
        }
    }
}

fn ideal_at<V: Valuation>(bound: &Bound, at: &V) -> Ideal {
    ideal_with(bound, &|var| Ideal::of_nat(at.value_of(var)))
}

/// The ideal denotation of a carrier element.
fn ideal_lifted<V: Valuation>(value: &Lifted<Bound>, at: &V) -> Lifted<Ideal> {
    match value {
        Lifted::Bottom => Lifted::Bottom,
        Lifted::Elem(bound) => Lifted::Elem(ideal_at(bound, at)),
    }
}

fn lifted_over_approximates(
    ideal: Lifted<Ideal>,
    reported: &Lifted<Bound>,
    at: &TotalValuation,
) -> bool {
    match (ideal, reported) {
        (Lifted::Bottom, Lifted::Bottom) => true,
        // `Bottom` is unreachability, not a magnitude: nothing over- or
        // under-approximates it, so the two must agree exactly.
        (Lifted::Bottom, Lifted::Elem(_)) | (Lifted::Elem(_), Lifted::Bottom) => false,
        (Lifted::Elem(exact), Lifted::Elem(bound)) => exact.is_over_approximated_by(bound.eval(at)),
    }
}

// ---------------------------------------------------------------------------
// generators and grids - deterministic, never randomly seeded
// ---------------------------------------------------------------------------

/// A fixed-seed LCG. Deterministic so that a failure reproduces exactly.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    fn below(&mut self, ceiling: u64) -> u64 {
        self.next() % ceiling
    }

    /// A uniform index into a slice. `try_from` rather than `as`: a silent
    /// truncation is denied crate-wide, and this file is not an exception.
    fn index(&mut self, ceiling: usize) -> usize {
        let span = u64::try_from(ceiling).unwrap_or(u64::MAX).max(1);
        usize::try_from(self.next() % span).unwrap_or(0)
    }
}

const VARS: [&str; 3] = ["x", "y", "z"];

/// Literals chosen to straddle the exact regime: `2^40` squared and `2^63`
/// doubled are both outside `u64`, which is where the shipped grid stops.
const LITERALS: [u64; 8] = [0, 1, 2, 3, 1 << 20, 1 << 31, 1 << 40, 1 << 63];

fn generate(rng: &mut Rng, depth: u32) -> Bound {
    if depth == 0 || rng.below(100) < 35 {
        return if rng.below(3) == 1 {
            Bound::constant(LITERALS[rng.index(LITERALS.len())])
        } else {
            Bound::var(VARS[rng.index(VARS.len())])
        };
    }
    match rng.below(6) {
        0 => Bound::sum([generate(rng, depth - 1), generate(rng, depth - 1)]),
        1 => Bound::max_of([generate(rng, depth - 1), generate(rng, depth - 1)]),
        2 | 3 => Bound::prod([generate(rng, depth - 1), generate(rng, depth - 1)]),
        4 => Bound::log(Base::TWO, generate(rng, depth - 1)),
        _ => Bound::pow(Base::TWO, generate(rng, depth - 1)),
    }
}

fn random_substitution(rng: &mut Rng) -> Substitution {
    let mut bindings = Vec::new();
    for name in VARS {
        if rng.below(2) == 0 {
            bindings.push((VarId::new(name), generate(rng, 2)));
        }
    }
    Substitution::from_bindings(bindings)
}

fn at(x: Nat, y: Nat, z: Nat) -> TotalValuation {
    let mut known = BTreeMap::new();
    known.insert(VarId::new("x"), x);
    known.insert(VarId::new("y"), y);
    known.insert(VarId::new("z"), z);
    TotalValuation::with_default(known, Nat::OMEGA)
}

/// A valuation grid that deliberately leaves the exact regime: it carries
/// points where two coordinates sum past `u64` and points where two multiply
/// past it, each of them next to a zero.
///
/// Hand written rather than a cross product: the interesting points are the
/// ones that put a saturating magnitude beside a zero, and a `k^3` grid buys
/// almost none of them while costing a debug-profile CI run minutes.
fn valuation_grid() -> Vec<TotalValuation> {
    const BIG: Nat = Nat::Fin(1 << 40);
    const HALF: Nat = Nat::Fin(1 << 63);
    vec![
        at(Nat::ZERO, Nat::ZERO, Nat::ZERO),
        at(Nat::ONE, Nat::ONE, Nat::ONE),
        at(Nat::Fin(3), Nat::Fin(5), Nat::Fin(7)),
        at(Nat::Fin(1 << 20), Nat::Fin(1 << 31), Nat::Fin(3)),
        at(BIG, BIG, BIG),
        at(HALF, HALF, HALF),
        // Saturation beside a zero, in each coordinate.
        at(Nat::ZERO, HALF, HALF),
        at(HALF, Nat::ZERO, HALF),
        at(HALF, HALF, Nat::ZERO),
        at(Nat::ZERO, BIG, BIG),
        at(BIG, BIG, Nat::ZERO),
        // The top, and the top beside a zero.
        at(Nat::OMEGA, Nat::OMEGA, Nat::OMEGA),
        at(Nat::ZERO, Nat::OMEGA, Nat::ZERO),
        at(Nat::OMEGA, Nat::ZERO, Nat::OMEGA),
        at(Nat::Fin(3), HALF, Nat::ZERO),
    ]
}

/// A carrier grid that is the shipped one **plus** the elements `B`'s grid
/// excludes by construction: literals whose square and whose double leave
/// `u64`.
fn outside_the_exact_regime() -> Vec<Lifted<Bound>> {
    vec![
        Lifted::Bottom,
        Lifted::Elem(Bound::zero()),
        Lifted::Elem(Bound::one()),
        Lifted::Elem(Bound::constant(1 << 31)),
        // The two literals `B`'s grid documents itself as excluding.
        Lifted::Elem(Bound::constant(1 << 40)),
        Lifted::Elem(Bound::constant(1 << 63)),
        Lifted::Elem(Bound::omega()),
        Lifted::Elem(Bound::var("x")),
        Lifted::Elem(Bound::prod([Bound::zero(), Bound::var("x")])),
        Lifted::Elem(Bound::sum([Bound::var("x"), Bound::var("y")])),
        Lifted::Elem(Bound::max_of([Bound::var("x"), Bound::var("y")])),
        Lifted::Elem(Bound::pow(Base::TWO, Bound::var("x"))),
    ]
}

// ---------------------------------------------------------------------------
// 1. is every divergence upward?
// ---------------------------------------------------------------------------

/// **The headline claim of LAN-59, tested where the grid stops.**
///
/// `times` is known not to be associative and known not to distribute once an
/// intermediate leaves `u64`. The shipped grid stays inside the exact regime,
/// so the law suite never sees it; this grid deliberately does not.
///
/// The claim under test is not "the equation holds" - it does not - but "every
/// disagreeing pair over-approximates". So both groupings of every triple, and
/// both sides of every distribution, are checked against the **ideal** value
/// of the corresponding product/sum. A single downward divergence here is a
/// soundness blocker; there are none.
#[test]
fn every_regrouping_over_approximates_the_ideal_value() {
    let grid = outside_the_exact_regime();
    let points = valuation_grid();
    let mut additive_divergences = 0u32;
    let mut peak_divergences = 0u32;

    for a in &grid {
        for b in &grid {
            for c in &grid {
                let left_assoc = B::times(&B::times(a, b), c);
                let right_assoc = B::times(a, &B::times(b, c));
                let distributed = B::plus(&B::times(a, b), &B::times(a, c));
                let factored = B::times(a, &B::plus(b, c));

                let peak_left = MaxPlus::times(&MaxPlus::times(a, b), c);
                let peak_right = MaxPlus::times(a, &MaxPlus::times(b, c));
                let peak_distributed = MaxPlus::plus(&MaxPlus::times(a, b), &MaxPlus::times(a, c));
                let peak_factored = MaxPlus::times(a, &MaxPlus::plus(b, c));

                for point in &points {
                    let (ia, ib, ic) = (
                        ideal_lifted(a, point),
                        ideal_lifted(b, point),
                        ideal_lifted(c, point),
                    );

                    let product = b_times(b_times(ia, ib), ic);
                    for (label, got) in [("(a*b)*c", &left_assoc), ("a*(b*c)", &right_assoc)] {
                        assert!(
                            lifted_over_approximates(product, got, point),
                            "DOWNWARD {label}: a={a:?} b={b:?} c={c:?} at {point:?}: \
                             reported {got:?}, ideal {product:?}"
                        );
                    }

                    let spread = b_plus(b_times(ia, ib), b_times(ia, ic));
                    for (label, got) in [("a*b + a*c", &distributed), ("a*(b + c)", &factored)] {
                        assert!(
                            lifted_over_approximates(spread, got, point),
                            "DOWNWARD {label}: a={a:?} b={b:?} c={c:?} at {point:?}: \
                             reported {got:?}, ideal {spread:?}"
                        );
                    }

                    // `MaxPlus`: `times` is `+` and `plus` is `max`.
                    let peak_product = peak_times(peak_times(ia, ib), ic);
                    for (label, got) in [("(a+b)+c", &peak_left), ("a+(b+c)", &peak_right)] {
                        assert!(
                            lifted_over_approximates(peak_product, got, point),
                            "DOWNWARD peak {label}: a={a:?} b={b:?} c={c:?} at {point:?}"
                        );
                    }
                    let peak_spread = peak_plus(peak_times(ia, ib), peak_times(ia, ic));
                    for (label, got) in [
                        ("max(a+b, a+c)", &peak_distributed),
                        ("a + max(b, c)", &peak_factored),
                    ] {
                        assert!(
                            lifted_over_approximates(peak_spread, got, point),
                            "DOWNWARD peak {label}: a={a:?} b={b:?} c={c:?} at {point:?}"
                        );
                    }

                    // Extensional divergence, which is what the law suite
                    // would report. `B` has it outside the exact regime;
                    // `MaxPlus` claims not to, and that claim is checked at
                    // exactly the magnitudes `B`'s grid excludes.
                    if denotes_differently(&left_assoc, &right_assoc, point)
                        || denotes_differently(&distributed, &factored, point)
                    {
                        additive_divergences += 1;
                    }
                    if denotes_differently(&peak_left, &peak_right, point)
                        || denotes_differently(&peak_distributed, &peak_factored, point)
                    {
                        peak_divergences += 1;
                    }
                }
            }
        }
    }

    // The grid must actually leave the exact regime, or this test asserts
    // nothing: a grid on which the equations hold cannot tell an upward
    // divergence from no divergence at all.
    assert!(
        additive_divergences > 0,
        "the grid never left B's exact regime, so nothing was tested"
    );

    // **`MaxPlus` really has no exact-regime boundary.** `max_plus.rs` delegates
    // its grids to `B`'s, so the shipped suite never checks `MaxPlus` outside
    // the regime `B` needs; the justification is two hand-picked assertions in
    // `peak::saturation_does_not_break_the_equations_here`. Here it is, over the
    // whole extended grid: `times` is `+`, which only grows, so saturation is
    // monotone on both sides of every regrouping.
    assert_eq!(
        peak_divergences, 0,
        "MaxPlus diverged outside the exact regime, so delegating B's grids hides \
         something specific to (max, +)"
    );
}

/// `true` iff two carrier values denote different things at `point`.
fn denotes_differently(a: &Lifted<Bound>, b: &Lifted<Bound>, point: &TotalValuation) -> bool {
    let denote = |value: &Lifted<Bound>| match value {
        Lifted::Bottom => Lifted::Bottom,
        Lifted::Elem(bound) => Lifted::Elem(bound.eval(point)),
    };
    denote(a) != denote(b)
}

/// `Bottom` absorbs - the shape of `times` in both instances (L4).
fn absorbing(a: Lifted<Ideal>, b: Lifted<Ideal>, op: fn(Ideal, Ideal) -> Ideal) -> Lifted<Ideal> {
    match (a, b) {
        (Lifted::Bottom, _) | (_, Lifted::Bottom) => Lifted::Bottom,
        (Lifted::Elem(left), Lifted::Elem(right)) => Lifted::Elem(op(left, right)),
    }
}

/// `Bottom` is the unit - the shape of `plus` in both instances (L1).
fn unital(a: Lifted<Ideal>, b: Lifted<Ideal>, op: fn(Ideal, Ideal) -> Ideal) -> Lifted<Ideal> {
    match (a, b) {
        (Lifted::Bottom, other) | (other, Lifted::Bottom) => other,
        (Lifted::Elem(left), Lifted::Elem(right)) => Lifted::Elem(op(left, right)),
    }
}

fn b_times(a: Lifted<Ideal>, b: Lifted<Ideal>) -> Lifted<Ideal> {
    absorbing(a, b, Ideal::times)
}

fn b_plus(a: Lifted<Ideal>, b: Lifted<Ideal>) -> Lifted<Ideal> {
    unital(a, b, Ideal::plus)
}

fn peak_times(a: Lifted<Ideal>, b: Lifted<Ideal>) -> Lifted<Ideal> {
    absorbing(a, b, Ideal::plus)
}

fn peak_plus(a: Lifted<Ideal>, b: Lifted<Ideal>) -> Lifted<Ideal> {
    unital(a, b, Ideal::join)
}

/// **The two divergences the wave documented, with their ideal values named.**
///
/// `b.rs` pins that the two groupings disagree. What it cannot say, because
/// `Nat` saturates, is *which* of the two answers is the true one. Here it is:
/// the ideal value is `0` in both cases, so `Const(0)` is exact and
/// `Const(omega)` is the loose-but-sound side. The divergence is upward.
#[test]
fn the_two_pinned_divergences_are_both_upward() {
    let point = at(Nat::Fin(3), Nat::Fin(5), Nat::ZERO);

    // (2^40 (*) 2^40) (*) 0  vs  2^40 (*) (2^40 (*) 0)
    let big = Lifted::Elem(Bound::constant(1u64 << 40));
    let zero_cost = Lifted::Elem(Bound::zero());
    let left = B::times(&B::times(&big, &big), &zero_cost);
    let right = B::times(&big, &B::times(&big, &zero_cost));
    assert_eq!(left, Lifted::Elem(Bound::omega()));
    assert_eq!(right, Lifted::Elem(Bound::zero()));

    let ideal = Ideal::Fin(1u128 << 40)
        .times(Ideal::Fin(1u128 << 40))
        .times(Ideal::Fin(0));
    assert_eq!(ideal, Ideal::Fin(0), "the true product is nothing");
    assert!(lifted_over_approximates(Lifted::Elem(ideal), &left, &point));
    assert!(lifted_over_approximates(
        Lifted::Elem(ideal),
        &right,
        &point
    ));

    // 0 (*) (2^63 (+) 2^63)  vs  0(*)2^63 (+) 0(*)2^63
    let half = Lifted::Elem(Bound::constant(1u64 << 63));
    let folded = B::times(&zero_cost, &B::plus(&half, &half));
    let spread = B::plus(&B::times(&zero_cost, &half), &B::times(&zero_cost, &half));
    assert_eq!(folded, Lifted::Elem(Bound::omega()));
    assert_eq!(spread, Lifted::Elem(Bound::zero()));

    let ideal = Ideal::Fin(0).times(Ideal::Fin(1u128 << 63).plus(Ideal::Fin(1u128 << 63)));
    assert_eq!(ideal, Ideal::Fin(0));
    assert!(lifted_over_approximates(
        Lifted::Elem(ideal),
        &folded,
        &point
    ));
    assert!(lifted_over_approximates(
        Lifted::Elem(ideal),
        &spread,
        &point
    ));
}

/// `Bound::eval` never reports a magnitude below the ideal value of the same
/// term. The floor of the whole crate, measured rather than argued.
#[test]
fn eval_never_dips_below_the_ideal_value() {
    let mut rng = Rng(0xDEAD_BEEF);
    let points = valuation_grid();
    for _ in 0..1500 {
        let term = generate(&mut rng, 4);
        for point in &points {
            let reported = term.eval(point);
            let exact = ideal_at(&term, point);
            assert!(
                exact.is_over_approximated_by(reported),
                "UNSOUND eval: {term} at {point:?} reported {reported:?}, ideal {exact:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 2. the `star` theorem
// ---------------------------------------------------------------------------

/// **The theorem `B::star` and `MaxPlus::star` both rest on.**
///
/// `Const(0)` is the only `Bound` denoting `0` at every valuation. Attacked
/// two ways: structurally, by requiring every generated term that mentions a
/// variable to denote `omega` at the all-`omega` point; and extensionally, by
/// requiring any generated term that denotes `0` across the whole grid to
/// *be* `Const(0)`.
#[test]
fn const_zero_is_the_only_bound_denoting_zero_everywhere() {
    let all_omega = at(Nat::OMEGA, Nat::OMEGA, Nat::OMEGA);
    let points = valuation_grid();
    let mut rng = Rng(0x0BAD_F00D);
    let mut everywhere_zero = 0u32;

    for _ in 0..3000 {
        let term = generate(&mut rng, 4);

        // Every constructor propagates `omega` upwards, so a term with a free
        // variable cannot be finite at the all-`omega` point.
        if !term.vars().is_empty() {
            assert_eq!(
                term.eval(&all_omega),
                Nat::OMEGA,
                "{term} mentions a variable but stayed finite at the all-omega point"
            );
        }

        if points.iter().all(|point| term.eval(point) == Nat::ZERO) {
            everywhere_zero += 1;
            assert_eq!(
                term.kind(),
                &BoundKind::Const(Nat::ZERO),
                "{term} denotes zero everywhere but is not Const(0), so star's \
                 syntactic test is not the denotational one"
            );
        }
    }
    assert!(
        everywhere_zero > 0,
        "no generated term denoted zero everywhere, so the theorem was never exercised"
    );

    // The near misses that motivate the theorem: zero somewhere, not zero
    // everywhere, and never `Const(0)`.
    for near_miss in [
        Bound::prod([Bound::zero(), Bound::var("x")]),
        Bound::prod([Bound::zero(), Bound::pow(Base::TWO, Bound::var("x"))]),
        Bound::log(Base::TWO, Bound::log(Base::TWO, Bound::var("x"))),
        Bound::max_of([Bound::zero(), Bound::var("x")]),
    ] {
        assert_eq!(
            near_miss.eval(&all_omega),
            Nat::OMEGA,
            "{near_miss} escaped the all-omega point"
        );
        assert_ne!(near_miss.kind(), &BoundKind::Const(Nat::ZERO));
    }
}

// ---------------------------------------------------------------------------
// 3. normalisation, and normalisation against substitution
// ---------------------------------------------------------------------------

/// **LAN-58's rewrite set is not a set of identities of the *shipped*
/// algebra.**
///
/// `RULE_TABLE`'s documentation says "every rule is an exact identity of the
/// algebra, so extraction is choosing between terms that denote the same
/// function and the cost function is not a soundness surface". The premise is
/// false in both directions, and `b.rs` already contains the proof: `times`
/// does not distribute over a saturating sum under a zero factor.
///
/// Both witnesses are recorded here.
///
/// * **Widening.** `a*b + a*c` normalises to `a*(b + c)`. At `a = 0`,
///   `b = c = 2^63` the input denotes `0` and the output denotes `omega`.
/// * **Narrowing.** `0 * (x + z) * (z + z)` normalises to `0 * z * (x + z)`.
///   At `x = 0`, `z = 2^63` the input denotes `omega` and the output denotes
///   `0`.
///
/// The conclusion - extraction is not a soundness surface - survives, but for
/// the reason written against `mul-assoc` rather than the one written against
/// the table: every rule preserves the **ideal** value, and every term the
/// crate can build lies between the ideal value and `omega`.
#[test]
fn normalisation_moves_the_denotation_in_both_directions() {
    // Widening.
    let spread = Bound::sum([
        Bound::prod([Bound::var("a"), Bound::var("b")]),
        Bound::prod([Bound::var("a"), Bound::var("c")]),
    ]);
    let factored = normalise(&spread)
        .expect("normalise must succeed")
        .into_bound();
    assert_eq!(
        factored,
        Bound::prod([
            Bound::var("a"),
            Bound::sum([Bound::var("b"), Bound::var("c")])
        ]),
        "the cost function prefers the factored form"
    );
    let mut known = BTreeMap::new();
    known.insert(VarId::new("a"), Nat::ZERO);
    known.insert(VarId::new("b"), Nat::Fin(1 << 63));
    known.insert(VarId::new("c"), Nat::Fin(1 << 63));
    let point = TotalValuation::with_default(known, Nat::OMEGA);
    assert_eq!(spread.eval(&point), Nat::ZERO);
    assert_eq!(
        factored.eval(&point),
        Nat::OMEGA,
        "normalisation widened 0 to omega"
    );
    assert_eq!(ideal_at(&spread, &point), Ideal::Fin(0));
    assert_eq!(ideal_at(&factored, &point), Ideal::Fin(0));

    // Narrowing.
    let saturating = Bound::prod([
        Bound::zero(),
        Bound::sum([Bound::var("x"), Bound::var("z")]),
        Bound::sum([Bound::var("z"), Bound::var("z")]),
    ]);
    // A small budget rather than the frozen one, and deliberately: on
    // `NormaliserBudget::FROZEN` this six-wire-node term reaches 101_724
    // e-nodes and takes 22 seconds in a release build. See
    // `a_six_node_bound_does_not_saturate_inside_a_ten_thousand_node_egraph`.
    // The extracted term is the same at every budget tried.
    let tightened = normalise_with(&saturating, NormaliserBudget::new(6, 400))
        .expect("normalise must succeed")
        .into_bound();
    let point = at(Nat::ZERO, Nat::ZERO, Nat::Fin(1 << 63));
    assert_eq!(saturating.eval(&point), Nat::OMEGA);
    assert_eq!(
        tightened.eval(&point),
        Nat::ZERO,
        "normalisation narrowed omega to 0"
    );
    assert_ne!(saturating, tightened);
    // Both are sound: the ideal value is nothing, and neither dips below it.
    assert_eq!(ideal_at(&saturating, &point), Ideal::Fin(0));
    assert_eq!(ideal_at(&tightened, &point), Ideal::Fin(0));
}

/// **A six-wire-node bound does not saturate inside a ten-thousand-node
/// e-graph**, and on the frozen budget it costs seconds and gigabytes.
///
/// `0 * (x + z) * (z + z)` is six wire nodes and nine tree nodes. Measured on
/// this revision, release profile:
///
/// | budget (iters, nodes) | stop | iterations | e-nodes | wall |
/// |---|---|---|---|---|
/// | `(6, 400)` | `NodeLimit` | 6 | 414 | 1.3 ms |
/// | `(10, 2 000)` | `NodeLimit` | 7 | 2 700 | 9.3 ms |
/// | `(20, 10 000)` | `NodeLimit` | 8 | 38 525 | 376 ms |
/// | `FROZEN` `(60, 100 000)` | `NodeLimit` | 9 | **101 724** | **22 s** |
///
/// Two things follow. The node limit is checked *between* iterations, so one
/// iteration overshoots it and the memory high-water mark is set by the
/// overshoot rather than by the limit - `egraph_nodes` finishes *above*
/// `node_limit`. And a three-factor product with two sums never reaches
/// `Saturated`, so `NORMAL_FORM_VERSION` pins "the normal form for this rule
/// set **and this budget**", not "the normal form".
///
/// Pinned at `(20, 10 000)` rather than at the frozen budget so that the
/// regression costs a third of a second instead of twenty-two.
#[test]
fn a_six_node_bound_does_not_saturate_inside_a_ten_thousand_node_egraph() {
    let saturating = Bound::prod([
        Bound::zero(),
        Bound::sum([Bound::var("x"), Bound::var("z")]),
        Bound::sum([Bound::var("z"), Bound::var("z")]),
    ]);
    assert_eq!(saturating.wire_node_count(), 6);

    let run = normalise_with(&saturating, NormaliserBudget::new(20, 10_000))
        .expect("normalise must succeed");
    assert_eq!(
        run.stop(),
        NormaliserStop::NodeLimit,
        "a six-node term was expected to exhaust a ten-thousand-node e-graph"
    );
    assert!(
        run.egraph_nodes() > 10_000,
        "the node limit is checked between iterations, so the graph overshoots \
         it: {} nodes against a limit of 10 000",
        run.egraph_nodes()
    );
}

/// Normalisation may move the denotation, but never below the ideal value of
/// the term it was given. This is the property `RULE_TABLE` needs and the one
/// its documentation does not state.
#[test]
fn normalisation_never_dips_below_the_ideal_value() {
    let mut rng = Rng(0x1234_5678);
    let points = valuation_grid();
    // A small budget rather than the frozen one: the property under test holds
    // for *whatever* the extractor returns, and a 100_000-node e-graph per term
    // costs a debug-profile CI run minutes. The two named witnesses above run
    // on the frozen budget.
    let budget = NormaliserBudget::new(12, 2_000);
    for _ in 0..60 {
        let term = generate(&mut rng, 2);
        let Ok(normal) = normalise_with(&term, budget) else {
            continue;
        };
        for point in &points {
            let exact = ideal_at(&term, point);
            let reported = normal.bound().eval(point);
            assert!(
                exact.is_over_approximated_by(reported),
                "UNSOUND normalise: {term} -> {} at {point:?}: reported {reported:?}, \
                 ideal {exact:?}",
                normal.bound()
            );
        }
    }
}

/// **Substituting and normalising do not commute**, and the divergence is
/// upward for whichever order normalises first.
///
/// LAN-57 and LAN-58 landed in the same wave and nothing pins their
/// interaction. A caller who normalises for a cache key and then instantiates
/// gets a strictly looser bound than one who instantiates first.
#[test]
fn substituting_before_normalising_is_tighter_than_after() {
    let term = Bound::sum([
        Bound::prod([Bound::var("a"), Bound::var("b")]),
        Bound::prod([Bound::var("a"), Bound::var("c")]),
    ]);
    let instantiate = Substitution::from_bindings([
        (VarId::new("a"), Bound::zero()),
        (VarId::new("b"), Bound::constant(1 << 63)),
        (VarId::new("c"), Bound::constant(1 << 63)),
    ]);

    let subst_first = instantiate.apply(&term);
    let normalise_first = instantiate.apply(normalise(&term).expect("normalise").bound());

    assert_eq!(subst_first, Bound::zero(), "instantiating first is exact");
    assert_eq!(
        normalise_first,
        Bound::omega(),
        "normalising first loses the zero"
    );
    assert_ne!(subst_first, normalise_first);

    // Both over-approximate the ideal composed value, which is nothing.
    let anywhere = at(Nat::ZERO, Nat::ZERO, Nat::ZERO);
    let exact = ideal_with(&term, &|var| match instantiate.get(var) {
        Some(image) => ideal_at(image, &anywhere),
        None => Ideal::of_nat(anywhere.value_of(var)),
    });
    assert_eq!(exact, Ideal::Fin(0));
    assert!(exact.is_over_approximated_by(subst_first.eval(&anywhere)));
    assert!(exact.is_over_approximated_by(normalise_first.eval(&anywhere)));
}

// ---------------------------------------------------------------------------
// 4. LAN-57 composition
// ---------------------------------------------------------------------------

/// **`then` is sound**, measured against the ideal rather than against the
/// other order.
///
/// The type documents that `first.then(&second).apply(b)` need not equal
/// `second.apply(&first.apply(b))`, and that both are bounds. The second half
/// of that claim is what this checks, and it checks it where it could fail:
/// images built in isolation, at magnitudes that saturate.
#[test]
fn composition_never_dips_below_the_ideal_value() {
    let mut rng = Rng(0xABCD_1234);
    let points = valuation_grid();
    let mut diverged = 0u32;

    for _ in 0..300 {
        let term = generate(&mut rng, 3);
        let first = random_substitution(&mut rng);
        let second = random_substitution(&mut rng);

        let composed = first.then(&second).apply(&term);
        let sequential = second.apply(&first.apply(&term));
        if composed != sequential {
            diverged += 1;
        }

        for point in &points {
            let through = |var: &VarId| -> Ideal {
                match first.get(var) {
                    Some(image) => ideal_with(image, &|inner| under(&second, inner, point)),
                    None => under(&second, var, point),
                }
            };
            let exact = ideal_with(&term, &through);
            for (label, got) in [("then", &composed), ("sequential", &sequential)] {
                assert!(
                    exact.is_over_approximated_by(got.eval(point)),
                    "UNSOUND {label}: term {term}, first {:?}, second {:?} at {point:?}: \
                     reported {:?}, ideal {exact:?}",
                    first.domain(),
                    second.domain(),
                    got.eval(point)
                );
            }
        }
    }
    let _ = diverged;

    // The documented witness, so the interesting case is reached by name
    // rather than by luck: composing folds `2^40 * 2^40` in isolation, where
    // the enclosing zero cannot rescue it.
    let host = Bound::prod([Bound::zero(), Bound::var("x"), Bound::var("z")]);
    let first = Substitution::of(
        VarId::new("x"),
        Bound::prod([Bound::constant(1 << 40), Bound::var("y")]),
    );
    let second = Substitution::of(VarId::new("y"), Bound::constant(1 << 40));
    let composed = first.then(&second).apply(&host);
    let sequential = second.apply(&first.apply(&host));
    assert_eq!(composed, Bound::omega(), "composing loses the zero");
    assert_ne!(composed, sequential, "the two orders must diverge here");

    let point = at(Nat::Fin(3), Nat::Fin(5), Nat::Fin(7));
    let exact = ideal_with(&host, &|var| match first.get(var) {
        Some(image) => ideal_with(image, &|inner| under(&second, inner, &point)),
        None => under(&second, var, &point),
    });
    assert_eq!(exact, Ideal::Fin(0), "the true composed cost is nothing");
    assert!(exact.is_over_approximated_by(composed.eval(&point)));
    assert!(exact.is_over_approximated_by(sequential.eval(&point)));
    assert_eq!(
        sequential.eval(&point),
        Nat::ZERO,
        "sequential is exact here"
    );
}

fn under<V: Valuation>(sub: &Substitution, var: &VarId, at: &V) -> Ideal {
    match sub.get(var) {
        Some(image) => ideal_at(image, at),
        None => Ideal::of_nat(at.value_of(var)),
    }
}

// ---------------------------------------------------------------------------
// 5. the `subst` bodies that cannot be whole-body mutated
// ---------------------------------------------------------------------------

/// `rebuild` dispatches on the **original** node's shape, and `arity_of` and
/// `operands_of` must agree with it for every shape.
///
/// None of the three can be whole-body mutated - `Bound` has no `Default` -
/// so each arm is pinned by an answer that is wrong in a *visible* way if the
/// arm is wrong: a `Trans` whose arity were `0` rebuilds with `Bound::omega()`
/// as its argument, which folds to `Const(omega)` rather than to the exact
/// magnitude.
#[test]
fn subst_rebuild_is_exact_for_every_shape() {
    let x = VarId::new("x");
    let five = Substitution::of(x.clone(), Bound::constant(5));

    // Trans/Log: `ceil_log2(5)` is 3. An arity of 0 gives `log2(omega)`.
    assert_eq!(
        five.apply(&Bound::log(Base::TWO, Bound::var("x"))),
        Bound::constant(3)
    );
    // Trans/Pow: `2^5` is 32.
    assert_eq!(
        five.apply(&Bound::pow(Base::TWO, Bound::var("x"))),
        Bound::constant(32)
    );
    // Sum, Prod, Max, each folding to a literal only if every operand arrived.
    assert_eq!(
        five.apply(&Bound::sum([Bound::var("x"), Bound::constant(7)])),
        Bound::constant(12)
    );
    assert_eq!(
        five.apply(&Bound::prod([Bound::var("x"), Bound::constant(7)])),
        Bound::constant(35)
    );
    assert_eq!(
        five.apply(&Bound::max_of([Bound::var("x"), Bound::constant(7)])),
        Bound::constant(7)
    );
    // Const and Var arms, including the unbound variable that stays free.
    assert_eq!(five.apply(&Bound::constant(9)), Bound::constant(9));
    assert_eq!(five.apply(&Bound::var("w")), Bound::var("w"));

    // And the same through `apply_checked`, which is the other `OnRefusal`
    // arm of the same `rebuild`.
    assert_eq!(
        five.apply_checked(&Bound::log(Base::TWO, Bound::var("x"))),
        Ok(Bound::constant(3))
    );
    assert_eq!(
        five.apply_checked(&Bound::pow(Base::TWO, Bound::var("x"))),
        Ok(Bound::constant(32))
    );
    assert_eq!(
        five.apply_checked(&Bound::max_of([Bound::var("x"), Bound::constant(7)])),
        Ok(Bound::constant(7))
    );
}

/// The rewrite memo is keyed on **structurally equal** subterms, so a subterm
/// reached by many paths is rewritten once and identically.
///
/// Checked on a doubling DAG: `t(k+1) = t(k) * t(k) + 1` is `2^k` tree nodes
/// at `k` levels. A memo that folded two structurally equal but separately
/// built subterms apart would still be *correct*; one that returned a stale
/// entry for a different subterm would not, so the result is pinned exactly
/// against the single-variable `Bound::subst`.
#[test]
fn subst_memoises_a_shared_dag_without_confusing_two_subterms() {
    let mut left = Bound::var("x");
    let mut right = Bound::var("x");
    for _ in 0..12 {
        left = Bound::sum([Bound::prod([left.clone(), left.clone()]), Bound::one()]);
        // Independently built, so the handles differ while the terms agree.
        right = Bound::sum([Bound::prod([right.clone(), right.clone()]), Bound::one()]);
    }
    assert_eq!(left, right);

    let host = Bound::max_of([left.clone(), Bound::prod([right.clone(), Bound::var("y")])]);
    let sub = Substitution::from_bindings([
        (VarId::new("x"), Bound::constant(2)),
        (VarId::new("y"), Bound::constant(3)),
    ]);

    let by_map = sub.apply(&host);
    let by_hand = host
        .subst(&VarId::new("x"), &Bound::constant(2))
        .subst(&VarId::new("y"), &Bound::constant(3));
    assert_eq!(by_map, by_hand);
    assert_eq!(sub.apply_checked(&host), Ok(by_map));
}

/// `carry_over` adds `next`'s own bindings **without** disturbing the composed
/// ones: the first substitution acts first, so the second's binding for a
/// variable the first also binds is unreachable.
///
/// The map has no `Default`, so this arm cannot be whole-body mutated; an
/// `insert` in place of `or_insert_with` changes composition silently.
#[test]
fn subst_carry_over_lets_the_first_substitution_win() {
    let (x, y, z) = (VarId::new("x"), VarId::new("y"), VarId::new("z"));
    let first = Substitution::of(x.clone(), Bound::var("y"));
    let second = Substitution::from_bindings([
        (x.clone(), Bound::constant(7)),
        (y.clone(), Bound::constant(9)),
        (z.clone(), Bound::constant(11)),
    ]);
    let composed = first.then(&second);

    assert_eq!(
        composed.get(&x),
        Some(&Bound::constant(9)),
        "x takes the composed image, not second's own binding"
    );
    assert_eq!(composed.get(&y), Some(&Bound::constant(9)));
    assert_eq!(composed.get(&z), Some(&Bound::constant(11)));
    assert_eq!(composed.domain(), vec![x.clone(), y.clone(), z.clone()]);

    // And it agrees with applying the two in sequence, on each bound variable.
    for var in [&x, &y, &z] {
        let term = Bound::var(var.symbol().clone());
        assert_eq!(composed.apply(&term), second.apply(&first.apply(&term)));
    }
}

/// Widening at a budget is **upward**: a refused node becomes `Const(omega)`,
/// and every operator above it is monotone, so the whole term can only grow.
///
/// Pinned at the one place it could have gone the other way - a refusal
/// underneath a `Prod` that also carries a zero, where the exact answer is
/// `0` and the widened answer is `omega`.
#[test]
fn subst_budget_widening_is_upward_even_under_a_zero_factor() {
    let limit = landav_bound::MAX_DEPTH;
    // One short of the limit, so that the enclosing `Prod` is exactly at it and
    // one more level of `log` is what tips the term over.
    let mut tower = Bound::var("x");
    for _ in 2..limit {
        tower = Bound::log(Base::TWO, tower);
    }
    assert_eq!(tower.depth(), limit - 1);

    let host = Bound::prod([Bound::zero(), tower.clone(), Bound::var("z")]);
    assert_eq!(
        host.depth(),
        limit,
        "the host must sit exactly at the limit"
    );
    let deepen = Substitution::of(VarId::new("x"), Bound::log(Base::TWO, Bound::var("y")));

    let widened = deepen.apply(&host);
    assert!(
        deepen.apply_checked(&host).is_err(),
        "the checked form must report what the total form widens"
    );

    let point = at(Nat::Fin(16), Nat::Fin(16), Nat::Fin(16));
    // The ideal value is nothing: a zero factor next to two finite ones.
    let exact = ideal_with(&host, &|var| match deepen.get(var) {
        Some(image) => ideal_at(image, &point),
        None => Ideal::of_nat(point.value_of(var)),
    });
    assert_eq!(exact, Ideal::Fin(0));
    assert!(
        exact.is_over_approximated_by(widened.eval(&point)),
        "widening dipped below the ideal value"
    );
}
