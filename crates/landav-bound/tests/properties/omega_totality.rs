//! LAN-56 AC4 - **no panics on `omega` in any operator** - and the saturation
//! rules that keep the top of the lattice reachable.
//!
//! Every property here is really two properties at once. The assertion states
//! the value, and the *absence of a panic* states totality: proptest reports a
//! panicking case as a failure, so a `todo!()`, an `unwrap` on `None`, an
//! arithmetic overflow or a non-terminating exponentiation all surface as a
//! failed case rather than as a green run.
//!
//! Two regressions are pinned by name here because both were real:
//!
//! * `0 * omega = 0` let `prod([var n, var m])` with `n |-> 0` and `m` unbound
//!   evaluate to a *finite* `0`, which `Verdict` then published as `Proved`
//!   for a program with no bound at all;
//! * `4294967296u64 as u32` is `0`, which made `pow(2, 2^32)` report `1`.

use landav_bound::{Base, Bound, BoundError, BoundKind, BoundShape, Lifted, Nat, Origin, Verdict};
use proptest::prelude::*;

use crate::support::{
    BoundSpec, Env, REF_OMEGA, REFERENCE_MAX_FINITE_EXPONENT, arb_base_u32, arb_env, arb_ref,
    arb_spec, base_of, build, nat_ref, ref_ceil_log, ref_join, ref_nat, ref_plus, ref_pow,
    ref_times, spec_of_shape,
};

proptest! {
    /// The scalar operators are total on `N u {omega}` and agree with the
    /// reference arithmetic on every pair, including every pair involving
    /// `omega`.
    #[test]
    fn nat_operators_are_total_on_omega(a in arb_ref(), b in arb_ref(), base in arb_base_u32()) {
        let (x, y) = (ref_nat(a), ref_nat(b));
        let k = base_of(base);

        // `prop_assert_eq!` runs its message through `concat!`, which blocks
        // implicit format captures, so every operand is passed positionally.
        prop_assert_eq!(nat_ref(x.plus(y)), ref_plus(a, b), "plus({:?}, {:?})", a, b);
        prop_assert_eq!(nat_ref(x.times(y)), ref_times(a, b), "times({:?}, {:?})", a, b);
        prop_assert_eq!(nat_ref(x.join(y)), ref_join(a, b), "join({:?}, {:?})", a, b);
        prop_assert_eq!(nat_ref(x.exp_of(k)), ref_pow(base, a), "{}^{:?}", base, a);
        prop_assert_eq!(
            nat_ref(x.ceil_log(k)),
            ref_ceil_log(base, a),
            "log_{}({:?})",
            base,
            a
        );
        prop_assert_eq!(x.is_finite(), a.is_some(), "is_finite disagrees at {:?}", a);
    }

    /// `omega` absorbs unconditionally in every scalar operator.
    #[test]
    fn omega_absorbs_unconditionally(other in arb_ref(), base in arb_base_u32()) {
        let x = ref_nat(other);
        let k = base_of(base);

        prop_assert_eq!(Nat::OMEGA.plus(x), Nat::OMEGA);
        prop_assert_eq!(x.plus(Nat::OMEGA), Nat::OMEGA);
        prop_assert_eq!(
            Nat::OMEGA.times(x),
            Nat::OMEGA,
            "omega * {:?} must be omega",
            other
        );
        prop_assert_eq!(
            x.times(Nat::OMEGA),
            Nat::OMEGA,
            "{:?} * omega must be omega",
            other
        );
        prop_assert_eq!(Nat::OMEGA.join(x), Nat::OMEGA);
        prop_assert_eq!(Nat::OMEGA.exp_of(k), Nat::OMEGA);
        prop_assert_eq!(Nat::OMEGA.ceil_log(k), Nat::OMEGA);
    }

    /// Every constructor accepts `omega` operands and returns `omega`, at
    /// every valuation. `BoundShape::ALL` again, so no constructor can be
    /// forgotten.
    #[test]
    fn every_constructor_is_total_on_omega(
        base in arb_base_u32(),
        log in any::<bool>(),
        env in arb_env(),
    ) {
        let omega_spec = BoundSpec::Const(REF_OMEGA);
        let valuation = env.valuation();
        for shape in BoundShape::ALL {
            let bound = build(&spec_of_shape(
                shape,
                omega_spec.clone(),
                omega_spec.clone(),
                base,
                log,
            ));
            // `Var` is the one shape that takes no operand, so it is the one
            // shape whose value is the valuation's, not omega's.
            let expected = if shape == BoundShape::Var {
                env.value_of(0)
            } else {
                REF_OMEGA
            };
            prop_assert_eq!(
                nat_ref(bound.eval(&valuation)),
                expected,
                "{:?} node {} did not absorb omega",
                shape,
                bound
            );
        }
    }

    /// Evaluation is total for arbitrary terms at the all-`omega` valuation -
    /// the valuation `TotalValuation::saturating` produces, and therefore the
    /// one every real analysis run hits first.
    #[test]
    fn eval_is_total_at_the_omega_valuation(spec in arb_spec()) {
        let bound = build(&spec);
        let got = bound.eval(&Env::all_omega().valuation());
        prop_assert_eq!(
            got,
            ref_nat(crate::support::naive_eval(&spec, &Env::all_omega())),
            "{} at the omega valuation",
            bound
        );
    }

    /// Overflow **saturates to `omega`**, never truncates and never lands on
    /// `u64::MAX`. `u64::MAX` is finite, so `FiniteBound` would accept it and
    /// `Verdict` would publish it as `Proved` while under-reporting the truth.
    #[test]
    fn overflow_saturates_to_omega_never_to_u64_max(a in 1u64..=u64::MAX, b in 1u64..=u64::MAX) {
        let sum = Nat::Fin(a).plus(Nat::Fin(b));
        match a.checked_add(b) {
            Some(exact) => prop_assert_eq!(sum, Nat::Fin(exact)),
            None => prop_assert_eq!(sum, Nat::OMEGA, "{} + {} must saturate to omega", a, b),
        }

        let product = Nat::Fin(a).times(Nat::Fin(b));
        match a.checked_mul(b) {
            Some(exact) => prop_assert_eq!(product, Nat::Fin(exact)),
            None => prop_assert_eq!(product, Nat::OMEGA, "{} * {} must saturate to omega", a, b),
        }
    }

    /// A term built entirely from finite constants either folds to the exact
    /// value or to `omega`. It may never fold to some other finite value,
    /// which is the shape every truncation bug takes.
    #[test]
    fn constant_folding_is_exact_or_omega(spec in arb_spec(), env in arb_env()) {
        let bound = build(&spec);
        let expected = crate::support::naive_eval(&spec, &env);
        prop_assert_eq!(
            nat_ref(bound.eval(&env.valuation())),
            expected,
            "{} folded away from its denotation",
            bound
        );
    }
}

/// **The `0 * omega` regression.**
///
/// `prod([var n, var m])` with `n |-> 0` and `m` absent. Under
/// `0 * omega = 0` this evaluated to a finite `0`, `FiniteBound::try_new`
/// accepted it, and `Verdict::classify` published `Proved(0)` for a program
/// with no established bound at all.
#[test]
fn zero_times_unbound_variable_is_omega_not_a_proved_zero() -> Result<(), BoundError> {
    use std::collections::BTreeMap;

    use landav_bound::{TotalValuation, VarId};

    let bound = Bound::prod([Bound::var("n"), Bound::var("m")]);

    // `n` is proved zero; `m` was never sized, so `saturating` sends it to the
    // top of the lattice.
    let mut known = BTreeMap::new();
    known.insert(VarId::new("n"), Nat::ZERO);
    let valuation = TotalValuation::saturating(known);

    assert_eq!(
        bound.eval(&valuation),
        Nat::OMEGA,
        "0 * omega must be omega; a finite 0 here is the unsound fold"
    );

    // And the verdict layer must refuse it rather than dress it up.
    let verdict = Verdict::classify(
        Lifted::Elem(Bound::magnitude(bound.eval(&valuation))),
        Origin::new("regression.rs:1"),
        None,
    );
    assert_eq!(verdict, Err(BoundError::UnblamedOmega));
    Ok(())
}

/// `Bound::prod` may not fold `Const(0) * Var(x)` to `0`, because variables
/// range over `N u {omega}` and the product is `omega` at `x = omega`. Folding
/// it makes the constructor non denotation-preserving in the unsound
/// direction, and makes `?a * 0 -> 0` an unsound e-graph congruence.
#[test]
fn prod_does_not_fold_zero_times_a_symbolic_operand() {
    let bound = Bound::prod([Bound::zero(), Bound::var("x")]);

    assert_ne!(
        bound,
        Bound::zero(),
        "Prod[Const(0), Var(x)] must not collapse to Const(0)"
    );
    assert_ne!(bound.kind(), &BoundKind::Const(Nat::ZERO));

    let at_omega = Env {
        vals: [REF_OMEGA; 3],
        default: REF_OMEGA,
    };
    assert_eq!(bound.eval(&at_omega.valuation()), Nat::OMEGA);
}

/// **The `as u32` regression.** `4294967296u64 as u32` is `0`, so a `pow` that
/// narrowed before testing the exponent computed `2^0 = 1` for `2^(2^32)`.
///
/// The contract is: any exponent at or above `Nat::MAX_FINITE_EXPONENT` is
/// `omega`, tested *before* any narrowing.
#[test]
fn pow_saturates_rather_than_truncating_the_exponent() {
    assert_eq!(Nat::MAX_FINITE_EXPONENT, REFERENCE_MAX_FINITE_EXPONENT);

    // The exact value that the truncating implementation reported as 1.
    let two_to_the_thirty_two: u64 = 1 << 32;
    assert_eq!(
        Nat::Fin(two_to_the_thirty_two).exp_of(Base::TWO),
        Nat::OMEGA,
        "2^(2^32) must be omega, not 1"
    );
    assert_eq!(
        Bound::pow(Base::TWO, Bound::constant(two_to_the_thirty_two)).kind(),
        &BoundKind::Const(Nat::OMEGA)
    );

    // Multiples of 2^32 are the whole family the truncation collapsed.
    for multiple in 1u64..=4 {
        assert_eq!(
            Nat::Fin(multiple * two_to_the_thirty_two).exp_of(Base::TWO),
            Nat::OMEGA
        );
    }

    // The boundary itself.
    assert_eq!(Nat::Fin(63).exp_of(Base::TWO), Nat::Fin(1u64 << 63));
    assert_eq!(Nat::Fin(64).exp_of(Base::TWO), Nat::OMEGA);
    assert_eq!(Nat::Fin(u64::MAX).exp_of(Base::TWO), Nat::OMEGA);

    // `base^0 == 1` and `base^omega == omega`, for a base that is not 2.
    assert_eq!(Nat::ZERO.exp_of(Base::TEN), Nat::ONE);
    assert_eq!(Nat::OMEGA.exp_of(Base::TEN), Nat::OMEGA);
    assert_eq!(
        Nat::Fin(19).exp_of(Base::TEN),
        Nat::Fin(10_000_000_000_000_000_000)
    );
    assert_eq!(Nat::Fin(20).exp_of(Base::TEN), Nat::OMEGA);
}

/// Saturation at the additive and multiplicative ceiling, stated at the exact
/// values where `saturating_add`/`saturating_mul` would differ.
#[test]
fn arithmetic_saturates_to_omega_not_to_u64_max() {
    assert_eq!(Nat::Fin(u64::MAX).plus(Nat::ONE), Nat::OMEGA);
    assert_eq!(Nat::Fin(u64::MAX).times(Nat::Fin(2)), Nat::OMEGA);
    assert_eq!(Nat::Fin(u64::MAX).plus(Nat::ZERO), Nat::Fin(u64::MAX));
    assert_eq!(Nat::Fin(u64::MAX).times(Nat::ONE), Nat::Fin(u64::MAX));
    // `Fin(u64::MAX)` is a distinct value from `Omega`, not a spelling of it.
    assert_ne!(Nat::Fin(u64::MAX), Nat::OMEGA);
    assert!(Nat::Fin(u64::MAX).is_finite());
    assert!(!Nat::OMEGA.is_finite());
}

/// `omega` lives inside `Const`. There is no seventh variant, so every `match`
/// arm that handles a constant handles `omega` and no operator can be written
/// that forgets the case.
#[test]
fn omega_is_a_const_not_a_seventh_constructor() {
    let omega = Bound::omega();
    assert_eq!(omega.shape(), BoundShape::Const);
    assert_eq!(omega.kind(), &BoundKind::Const(Nat::OMEGA));
    assert_eq!(omega, Bound::magnitude(Nat::OMEGA));
    assert!(!omega.is_finite());
    assert_eq!(BoundShape::ALL.len(), 6);
}
