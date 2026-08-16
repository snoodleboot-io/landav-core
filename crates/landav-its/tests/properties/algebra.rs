//! The polynomial and guard algebra, checked against the reference evaluator.
//!
//! # Why this file exists at all
//!
//! `reference` deliberately re-implements polynomial evaluation rather than
//! calling [`landav_its::Polynomial::evaluate`], so that a soundness failure is
//! a real disagreement and not two copies of one mistake. The cost of that
//! independence is that the crate's *own* evaluator — and
//! [`landav_its::Constraint::holds`] and [`landav_its::Guard::holds`] with it —
//! is then exercised by nothing at all. Mutation testing said so: replacing
//! `Constraint::holds` with `Some(true)`, `Some(false)` or `None` changed no
//! test.
//!
//! That is a genuine gap rather than a scoring artefact. These are public
//! methods a consumer will reach for — `landav-solvers` reading back a witness,
//! `LAN-68` deciding whether a transition is live — and "nothing uses it yet"
//! is exactly when a wrong answer gets baked in.
//!
//! So this file pins them *against the reference*, which turns the redundancy
//! into a cross-check: the two implementations must agree at every valuation,
//! and if they ever diverge one of them is wrong and the suite says which
//! inputs separated them.

use std::collections::BTreeMap;

use landav_its::{
    Constraint, Construct, Guard, ItsVar, MAX_DEGREE, MAX_MONOMIALS, Monomial, Polynomial,
    Relation, Update,
};
use proptest::prelude::*;

use crate::reference::{State, evaluate};

/// The variables generated polynomials draw from.
const NAMES: [&str; 3] = ["x", "y", "z"];

fn var(name: &str) -> ItsVar {
    ItsVar::new(name)
}

/// A valuation as both shapes: the reference's map, and the crate's closure.
fn valuation(values: &[i128]) -> State {
    NAMES
        .iter()
        .zip(values.iter())
        .map(|(name, value)| ((*name).to_owned(), *value))
        .collect::<BTreeMap<String, i128>>()
}

/// The crate's own evaluation of `polynomial`.
fn crate_eval(polynomial: &Polynomial, state: &State) -> Option<i128> {
    polynomial.evaluate(&|var: &ItsVar| state.get(var.as_str()).copied())
}

/// A small polynomial.
fn arb_polynomial() -> impl Strategy<Value = Polynomial> {
    prop::collection::vec((-5_i64..=5, 0_usize..NAMES.len(), 0_u32..3), 0..4).prop_map(|terms| {
        let monomials: Vec<Monomial> = terms
            .into_iter()
            .filter_map(|(coefficient, index, exponent)| {
                Monomial::new(coefficient, [(var(NAMES[index]), exponent)])
            })
            .collect();
        Polynomial::from_monomials(monomials).unwrap_or_else(|_| Polynomial::zero())
    })
}

fn arb_values() -> impl Strategy<Value = Vec<i128>> {
    prop::collection::vec(-6_i128..=6, NAMES.len())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 384, max_global_rejects: 0, ..ProptestConfig::default() })]

    /// The crate's evaluator and the reference's agree, everywhere.
    ///
    /// This is what licenses every other property in the suite to use the
    /// reference: if the two ever disagree, one is wrong, and the shrunk case
    /// says which inputs separate them.
    #[test]
    fn the_two_evaluators_agree(polynomial in arb_polynomial(), values in arb_values()) {
        let state = valuation(&values);
        prop_assert_eq!(
            crate_eval(&polynomial, &state),
            evaluate(&polynomial, &state),
            "the crate and the reference disagree on `{}`", polynomial
        );
    }

    /// A constraint holds exactly when its polynomial compares to zero the way
    /// the relation says.
    #[test]
    fn a_constraint_holds_when_its_relation_does(
        polynomial in arb_polynomial(),
        values in arb_values(),
        relation in prop_oneof![Just(Relation::Ge), Just(Relation::Gt), Just(Relation::Eq)],
    ) {
        let state = valuation(&values);
        let constraint = Constraint::new(polynomial.clone(), relation);
        let value = evaluate(&polynomial, &state).expect("small values do not overflow");
        // The expected answer, from the definition of the relation rather than
        // from `Relation::holds`.
        let expected = match relation {
            Relation::Ge => value >= 0,
            Relation::Gt => value > 0,
            Relation::Eq => value == 0,
        };
        prop_assert_eq!(
            constraint.holds(&|var: &ItsVar| state.get(var.as_str()).copied()),
            Some(expected),
            "`{}` at {:?}", constraint, values
        );
    }

    /// A guard holds exactly when every conjunct does.
    #[test]
    fn a_guard_holds_when_all_its_conjuncts_do(
        left in arb_polynomial(),
        right in arb_polynomial(),
        values in arb_values(),
    ) {
        let state = valuation(&values);
        let lookup = |var: &ItsVar| state.get(var.as_str()).copied();
        let first = Constraint::new(left, Relation::Ge);
        let second = Constraint::new(right, Relation::Gt);
        let guard = Guard::new([first.clone(), second.clone()]);

        let expected = first.holds(&lookup).and_then(|one| {
            second.holds(&lookup).map(|two| one && two)
        });
        prop_assert_eq!(guard.holds(&lookup), expected, "`{}`", guard);
    }

    /// **Arithmetic is denotation preserving.**
    ///
    /// The constructors normalise, collect like terms and drop zeroes, and none
    /// of that may change the function the polynomial denotes. Evaluated with
    /// the reference on both sides, so a normalisation bug cannot hide behind
    /// the same bug in the evaluator.
    #[test]
    fn arithmetic_preserves_the_denotation(
        left in arb_polynomial(),
        right in arb_polynomial(),
        values in arb_values(),
    ) {
        let state = valuation(&values);
        let at = |polynomial: &Polynomial| evaluate(polynomial, &state);

        let (Some(left_value), Some(right_value)) = (at(&left), at(&right)) else {
            return Ok(());
        };

        if let Ok(sum) = left.add(&right) {
            prop_assert_eq!(at(&sum), left_value.checked_add(right_value));
        }
        if let Ok(difference) = left.sub(&right) {
            prop_assert_eq!(at(&difference), left_value.checked_sub(right_value));
        }
        if let Ok(product) = left.multiply(&right) {
            prop_assert_eq!(at(&product), left_value.checked_mul(right_value));
        }
        if let Ok(negated) = left.negate() {
            prop_assert_eq!(at(&negated), left_value.checked_neg());
        }
        if let Ok(squared) = left.power(2) {
            prop_assert_eq!(at(&squared), left_value.checked_mul(left_value));
        }
        if let Ok(zeroth) = left.power(0) {
            prop_assert_eq!(at(&zeroth), Some(1), "anything to the zero is one");
        }
    }
}

// ---------------------------------------------------------------------------
// the edges, pinned by hand
// ---------------------------------------------------------------------------

/// `Guard::new` drops constraints that constrain nothing.
///
/// Not cosmetic: a guard is compared and deduplicated, so two guards that mean
/// the same thing must *be* the same value, and `1 >= 0` tagging along would
/// make them differ.
#[test]
fn a_guard_drops_trivially_true_constraints() {
    let always = Constraint::new(Polynomial::constant(1), Relation::Gt);
    assert!(always.is_trivially_true(), "1 > 0 is trivially true");
    assert!(!always.is_trivially_false());

    let real = Constraint::new(Polynomial::var(var("x")), Relation::Ge);
    assert!(!real.is_trivially_true(), "x >= 0 depends on x");

    let guard = Guard::new([always, real.clone()]);
    assert_eq!(
        guard.constraints(),
        &[real],
        "a constraint that constrains nothing was kept: {guard}"
    );
    assert!(Guard::new([]).is_always());
    assert_eq!(Guard::always().to_string(), "true");
}

/// A contradiction is recognised, and only a real one.
#[test]
fn a_trivially_false_guard_is_recognised() {
    let never = Constraint::new(Polynomial::constant(1), Relation::Eq);
    assert!(never.is_trivially_false(), "1 = 0 is a contradiction");
    assert!(Guard::new([never]).is_trivially_unsatisfiable());

    // `x = 0` is satisfiable, and the check must not claim otherwise: a guard
    // wrongly called unsatisfiable has its transition dropped, which removes
    // executions the program has.
    let maybe = Constraint::new(Polynomial::var(var("x")), Relation::Eq);
    assert!(!maybe.is_trivially_false());
    assert!(!Guard::new([maybe]).is_trivially_unsatisfiable());
}

/// A guard renders its conjuncts separated, exactly once each.
///
/// The separator logic is an index comparison, and every off-by-one variant of
/// it produces text that still contains both conjuncts -- so only pinning the
/// whole string catches it.
#[test]
fn guard_rendering_is_pinned() {
    let first = Constraint::new(Polynomial::var(var("x")), Relation::Ge);
    let second = Constraint::new(Polynomial::var(var("y")), Relation::Gt);

    assert_eq!(Guard::new([first.clone()]).to_string(), "x >= 0");
    assert_eq!(
        Guard::new([first, second]).to_string(),
        "x >= 0 && y > 0",
        "two conjuncts render with exactly one separator between them"
    );

    assert_eq!(
        Constraint::new(Polynomial::constant(-2), Relation::Eq).to_string(),
        "-2 = 0"
    );
}

/// A guard reports the variables it mentions, and no others.
#[test]
fn a_guard_reports_its_variables() {
    let constraint = Constraint::new(
        Polynomial::var(var("x"))
            .add(&Polynomial::var(var("y")))
            .expect("small"),
        Relation::Ge,
    );
    let guard = Guard::new([constraint]);
    let mentioned: Vec<String> = guard.vars().iter().map(|v| v.as_str().to_owned()).collect();
    assert_eq!(mentioned, vec!["x".to_owned(), "y".to_owned()]);
    assert!(Guard::always().vars().is_empty());
}

/// Monomials normalise: repeated variables have their exponents summed.
#[test]
fn a_monomial_collects_repeated_variables() {
    let squared = Monomial::new(3, [(var("x"), 1), (var("x"), 1)]).expect("in range");
    assert_eq!(squared.degree(), 2, "x * x is degree two");
    assert_eq!(squared.powers().len(), 1, "one variable, one entry");
    assert_eq!(squared.coefficient(), 3);
    assert!(!squared.is_constant());

    let dropped = Monomial::new(5, [(var("x"), 0)]).expect("in range");
    assert!(dropped.is_constant(), "a zero exponent is not a factor");
    assert_eq!(dropped.degree(), 0);

    // An exponent sum that would leave `u32` is refused rather than wrapped.
    assert!(Monomial::new(1, [(var("x"), u32::MAX), (var("x"), 1)]).is_none());
}

/// Polynomials collect like terms and drop the ones that cancel.
#[test]
fn a_polynomial_collects_and_cancels() {
    let collected =
        Polynomial::from_monomials([Monomial::linear(2, var("x")), Monomial::linear(3, var("x"))])
            .expect("in range");
    assert_eq!(collected.monomials().len(), 1, "2x + 3x is one term");
    assert_eq!(collected.to_string(), "5*x");

    let cancelled = Polynomial::from_monomials([
        Monomial::linear(4, var("x")),
        Monomial::linear(-4, var("x")),
    ])
    .expect("in range");
    assert!(cancelled.is_zero(), "4x - 4x is zero: {cancelled}");
    assert_eq!(cancelled.to_string(), "0");
    assert_eq!(cancelled.as_constant(), Some(0));

    assert_eq!(Polynomial::constant(7).as_constant(), Some(7));
    assert_eq!(Polynomial::var(var("x")).as_constant(), None);
    assert_eq!(Polynomial::constant(0), Polynomial::zero());
    assert_eq!(Polynomial::var(var("x")).degree(), 1);
}

/// Both size caps refuse rather than wrapping or grinding.
///
/// Neither implies the other, which is the whole reason there are two.
#[test]
fn the_polynomial_caps_refuse() {
    let base = Polynomial::var(var("x"));
    assert!(
        base.power(MAX_DEGREE).is_ok(),
        "a polynomial exactly at the degree cap is allowed"
    );
    assert!(
        base.power(MAX_DEGREE + 1).is_err(),
        "past the degree cap must refuse"
    );

    // A sum of many distinct terms, raised to a power, expands past the term
    // cap while staying well inside the degree cap.
    let wide = Polynomial::from_monomials(
        (0..NAMES.len())
            .map(|index| Monomial::linear(1, var(NAMES[index])))
            .chain([Monomial::constant(1)]),
    )
    .expect("four terms");
    let mut grown = wide.clone();
    let mut refused = false;
    for _ in 0..MAX_DEGREE {
        match grown.multiply(&wide) {
            Ok(bigger) => grown = bigger,
            Err(_) => {
                refused = true;
                break;
            }
        }
    }
    assert!(
        refused || grown.monomials().len() <= MAX_MONOMIALS,
        "the term cap was neither enforced nor respected"
    );

    // Overflow refuses rather than wrapping.
    let huge = Polynomial::constant(i64::MAX);
    assert!(huge.add(&huge).is_err(), "i64::MAX + i64::MAX must refuse");
    assert!(
        Polynomial::constant(i64::MIN).negate().is_err(),
        "negating i64::MIN must refuse"
    );
}

/// An update is a simultaneous assignment, and silence means identity.
#[test]
fn an_update_reports_what_it_writes() {
    assert!(Update::identity().is_identity());
    assert_eq!(Update::identity().to_string(), "skip");

    let update = Update::new([
        (var("x"), Polynomial::var(var("y"))),
        (var("y"), Polynomial::var(var("x"))),
    ]);
    assert!(!update.is_identity());
    assert_eq!(update.get(&var("x")), Some(&Polynomial::var(var("y"))));
    assert_eq!(
        update.get(&var("z")),
        None,
        "an unmentioned variable is unchanged"
    );

    let targets: Vec<String> = update.targets().map(|v| v.as_str().to_owned()).collect();
    assert_eq!(targets, vec!["x".to_owned(), "y".to_owned()]);
    assert_eq!(update.assignments().len(), 2);
}

/// A polynomial renders the way the emitter and the diagnostics expect.
#[test]
fn polynomial_rendering_is_pinned() {
    assert_eq!(Polynomial::zero().to_string(), "0");
    assert_eq!(Polynomial::constant(-3).to_string(), "-3");
    assert_eq!(Polynomial::var(var("x")).to_string(), "x");

    let squared = Polynomial::var(var("x")).power(2).expect("in range");
    assert_eq!(squared.to_string(), "x^2");

    let mixed = Polynomial::var(var("x"))
        .sub(&Polynomial::constant(4))
        .expect("in range");
    // Canonical order puts the constant first.
    assert_eq!(mixed.to_string(), "-4 + x");
}

/// **The polynomial caps are inclusive, and each is checked on its own.**
///
/// `the_polynomial_caps_refuse` above establishes that the caps bite; what it
/// deliberately does not do is say *where*. Its term-cap assertion is the
/// disjunction `refused || len <= MAX_MONOMIALS`, which a `>=` comparison
/// satisfies just as well as a `>` one — and mutation testing found exactly
/// that: swapping `>` for `>=` or `==` in `Polynomial::check_limits` changed
/// no test, and neither did replacing the whole function with `Ok(())`.
///
/// An off-by-one here is not cosmetic. Refusing at the cap turns a
/// representable program into a refusal, which shows up as a *missing* bound;
/// admitting one past it defeats the reason the cap exists, which is that the
/// emitter expands every power into repeated multiplication. So the boundary
/// is stated as a boundary: at the cap, accepted; one past it, refused, and
/// with the construct that names which cap was hit.
#[test]
fn the_polynomial_caps_are_inclusive_and_separately_enforced() {
    let wide = |count: usize| -> Vec<Monomial> {
        (0..count)
            .map(|index| Monomial::linear(1, var(&format!("w{index}"))))
            .collect()
    };

    let at_cap = Polynomial::from_monomials(wide(MAX_MONOMIALS))
        .expect("a polynomial with exactly MAX_MONOMIALS terms is representable");
    assert_eq!(
        at_cap.monomials().len(),
        MAX_MONOMIALS,
        "the terms were silently dropped rather than accepted"
    );

    assert_eq!(
        Polynomial::from_monomials(wide(MAX_MONOMIALS + 1)),
        Err(Construct::PolynomialSize),
        "one term past the cap must refuse, and must say it was the size cap"
    );

    // The degree cap is independent: one term, well inside the term cap, and
    // past the degree cap on its own.
    let steep = Monomial::new(1, [(var("x"), MAX_DEGREE + 1)]).expect("exponents do not overflow");
    assert_eq!(
        Polynomial::from_monomials([steep]),
        Err(Construct::PolynomialDegree),
        "a single term past the degree cap must refuse, naming the degree cap"
    );
    let level = Monomial::new(1, [(var("x"), MAX_DEGREE)]).expect("exponents do not overflow");
    assert!(
        Polynomial::from_monomials([level]).is_ok(),
        "a term exactly at the degree cap is representable"
    );
}

/// **`multiply` checks the term count *before* forming the products.**
///
/// The doc comment claims it, and it is the difference between refusing a
/// hostile pair of operands and materialising their product first. The claim
/// is only observable because the two failures carry different constructs: the
/// pre-check reports [`Construct::PolynomialSize`], whereas forming the
/// products first would hit a coefficient overflow and report
/// [`Construct::ArithmeticOverflow`]. Operands chosen so that the two answers
/// disagree.
#[test]
fn multiply_refuses_on_size_before_it_multiplies_anything() {
    let saturated = |count: usize, prefix: &str| -> Polynomial {
        Polynomial::from_monomials(
            (0..count).map(|index| Monomial::linear(i64::MAX, var(&format!("{prefix}{index}")))),
        )
        .expect("well inside both caps")
    };

    // 17 * 16 = 272 pairs, past MAX_MONOMIALS; every individual product would
    // overflow, so the *reported* construct says which check ran first.
    let left = saturated(17, "l");
    let right = saturated(16, "r");
    assert_eq!(
        left.multiply(&right),
        Err(Construct::PolynomialSize),
        "the size cap must be reached before any product is formed"
    );

    // 16 * 16 = 256 pairs, exactly the cap: representable, and formed. The
    // coefficients are 1 here so the arithmetic cannot be what refuses.
    let ones = |count: usize, prefix: &str| -> Polynomial {
        Polynomial::from_monomials(
            (0..count).map(|index| Monomial::linear(1, var(&format!("{prefix}{index}")))),
        )
        .expect("well inside both caps")
    };
    let product = ones(16, "a")
        .multiply(&ones(16, "b"))
        .expect("a product landing exactly on the term cap is representable");
    assert_eq!(
        product.monomials().len(),
        MAX_MONOMIALS,
        "16 distinct terms times 16 distinct terms is 256 distinct terms"
    );
}

/// **Rendering: the sign of a term that is not the first, and the `*` between
/// two variables of an implicit-coefficient term.**
///
/// `polynomial_rendering_is_pinned` covers a leading negative (`-4 + x`) and a
/// single power (`x^2`). Neither reaches the two branches below, and mutation
/// testing said so: the comparison choosing `" - "` from `" + "` for a
/// *non-leading* term, and the one that decides whether a `*` separates two
/// variables of a coefficient-1 term, both survived.
///
/// A dropped `*` is not a cosmetic defect — `x*y` and `xy` are different
/// variables to a reader and to any parser downstream of this crate — and a
/// dropped minus sign renders a subtraction as an addition.
#[test]
fn polynomial_rendering_separates_signs_and_factors() {
    let difference = Polynomial::var(var("x"))
        .sub(&Polynomial::var(var("y")))
        .expect("in range");
    assert_eq!(
        difference.to_string(),
        "x - y",
        "a negative term after the first must render as a subtraction"
    );

    let sum = Polynomial::var(var("x"))
        .add(&Polynomial::var(var("y")))
        .expect("in range");
    assert_eq!(
        sum.to_string(),
        "x + y",
        "and a positive one as an addition"
    );
    assert_ne!(
        difference.to_string(),
        sum.to_string(),
        "`x - y` and `x + y` must not render alike"
    );

    let product = Polynomial::var(var("x"))
        .multiply(&Polynomial::var(var("y")))
        .expect("in range");
    assert_eq!(
        product.to_string(),
        "x*y",
        "two variables in one term must be separated, not run together"
    );

    let three = Polynomial::from_monomials([Monomial::new(
        1,
        [(var("x"), 2), (var("y"), 1), (var("z"), 1)],
    )
    .expect("in range")])
    .expect("in range");
    assert_eq!(
        three.to_string(),
        "x^2*y*z",
        "every factor after the first takes a separator, exponent or not"
    );

    // With an explicit coefficient the separator appears before the *first*
    // factor too, which is the other side of the same branch.
    let scaled = Polynomial::from_monomials([
        Monomial::new(3, [(var("x"), 1), (var("y"), 1)]).expect("in range")
    ])
    .expect("in range");
    assert_eq!(scaled.to_string(), "3*x*y");
}

/// What separates two assignments in an update's rendering.
const SEPARATOR: &str = ", ";

/// **An update renders as a comma-separated simultaneous assignment.**
///
/// `an_update_reports_what_it_writes` pins the *identity* rendering ("skip")
/// and every accessor, but never renders an update with two assignments in it
/// — so the comparison that decides where the separator goes was unobserved,
/// and survived being changed three ways. Running `x := y` into `y := x` with
/// no separator, or emitting a leading one, produces text a reader has to
/// guess at.
#[test]
fn update_rendering_separates_its_assignments() {
    let single = Update::new([(var("x"), Polynomial::var(var("y")))]);
    let rendered = single.to_string();
    assert_eq!(rendered, "x := y");
    assert!(
        !rendered.starts_with(SEPARATOR),
        "a single assignment must not be preceded by a separator: {rendered:?}"
    );

    let pair = Update::new([
        (var("x"), Polynomial::var(var("y"))),
        (var("y"), Polynomial::var(var("x"))),
    ]);
    let rendered = pair.to_string();
    assert_eq!(
        rendered, "x := y, y := x",
        "two assignments take exactly one separator, between them"
    );
    assert_eq!(
        rendered.matches(SEPARATOR).count(),
        1,
        "n assignments take n-1 separators: {rendered:?}"
    );

    let three = Update::new([
        (var("x"), Polynomial::constant(1)),
        (var("y"), Polynomial::constant(2)),
        (var("z"), Polynomial::constant(3)),
    ]);
    assert_eq!(three.to_string(), "x := 1, y := 2, z := 3");
}

/// **A polynomial never carries a zero coefficient**, which is what makes four
/// surviving mutants genuinely equivalent rather than untested.
///
/// Both [`Polynomial`]'s `Display` and `koat::render_polynomial` choose a sign
/// with `coefficient < 0`. Swapping that for `<= 0` survives mutation testing,
/// and it survives because the two agree on every input the function can
/// receive: a zero coefficient is dropped on construction and cancelled on
/// collection, so it never reaches the comparison. `koat.rs` says as much in a
/// comment; this test is the executable half of that claim.
///
/// It is worth having as a test rather than a note because the equivalence is
/// **contingent**. Make a zero coefficient representable — a constructor that
/// skips normalisation, a `with_coefficient(0)` that survives into a
/// polynomial — and those mutants stop being equivalent, silently. Then this
/// fails, and the note is no longer true.
#[test]
fn no_polynomial_can_carry_a_zero_coefficient() {
    let zeroed = Polynomial::from_monomials([
        Monomial::linear(0, var("x")),
        Monomial::constant(0),
        Monomial::linear(5, var("y")),
    ])
    .expect("in range");
    assert_eq!(
        zeroed.monomials().len(),
        1,
        "zero coefficients must be dropped on construction: {zeroed}"
    );

    let cancelled = Polynomial::var(var("x"))
        .sub(&Polynomial::var(var("x")))
        .expect("in range");
    assert!(
        cancelled.is_zero(),
        "x - x must collect to the zero polynomial, not to a zero term"
    );
    assert!(cancelled.monomials().is_empty());

    assert_eq!(Polynomial::constant(0), Polynomial::zero());
    assert!(Polynomial::constant(0).monomials().is_empty());

    // The invariant, over every polynomial the constructors can reach.
    let built = [
        Polynomial::zero(),
        Polynomial::constant(-7),
        Polynomial::var(var("x")),
        zeroed,
        cancelled,
        Polynomial::var(var("x"))
            .multiply(&Polynomial::var(var("y")))
            .expect("in range"),
        Polynomial::var(var("x")).power(3).expect("in range"),
    ];
    for polynomial in &built {
        for term in polynomial.monomials() {
            assert_ne!(
                term.coefficient(),
                0,
                "{polynomial} carries a zero coefficient, which makes the sign \
                 comparisons in Display and koat::render_polynomial reachable at zero"
            );
        }
    }
}

/// **The identity update and the default update are the same value**, which is
/// why `Update::identity -> Default::default()` survives mutation.
///
/// Recorded rather than chased. The two are required to agree — a `Default`
/// that differed from the identity would mean `Update::default()` silently
/// wrote something — so no test can separate them, and the surviving mutant is
/// the correct outcome rather than a gap.
#[test]
fn the_identity_update_is_the_default_update() {
    assert_eq!(Update::identity(), Update::default());
    assert!(Update::default().is_identity());
    assert_eq!(Update::default().assignments().len(), 0);
    assert_eq!(
        Update::identity().to_string(),
        Update::default().to_string()
    );
}
