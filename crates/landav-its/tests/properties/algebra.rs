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
    Constraint, Guard, ItsVar, MAX_DEGREE, MAX_MONOMIALS, Monomial, Polynomial, Relation, Update,
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
