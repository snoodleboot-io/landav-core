//! LAN-56 AC3 - monotonicity under pointwise argument increase.
//!
//! The guarantee under test: for every `b: Bound` and all valuations
//! `v <= v'` pointwise, `[[b]](v) <= [[b]](v')`. It must hold for **every
//! value of the type**, not for values that happened to be built carefully,
//! which is why the terms here are drawn at random over all six constructors
//! rather than listed.
//!
//! Weak monotonicity is what makes composition-by-substitution sound. It is
//! not tightness: `Const(omega)` is monotone and useless.

use landav_bound::{Base, Bound, BoundShape, Nat};
use proptest::prelude::*;

use crate::support::{
    BoundSpec, Env, Ref, arb_base_u32, arb_env_pair, arb_ordered_refs, arb_ref, arb_spec, base_of,
    build, nat_ref, ref_le, ref_nat, shape_takes_operands, spec_of_shape,
};

/// `Bound::eval` at two valuations, ordered without touching `Nat: Ord`.
fn evals_ordered(bound: &Bound, lo: &Env, hi: &Env) -> (Ref, Ref) {
    (
        nat_ref(bound.eval(&lo.valuation())),
        nat_ref(bound.eval(&hi.valuation())),
    )
}

proptest! {
    /// The generator's promise, checked rather than assumed. If this fails,
    /// every other property in this file is testing a weaker antecedent than
    /// it claims to.
    #[test]
    fn ordered_valuation_pairs_really_are_ordered((lo, hi) in arb_env_pair()) {
        prop_assert!(lo.le(&hi), "generator produced {lo:?} </= {hi:?}");
    }

    /// AC3, over arbitrary terms.
    #[test]
    fn eval_is_monotone_under_pointwise_increase(
        spec in arb_spec(),
        (lo, hi) in arb_env_pair(),
    ) {
        let bound = build(&spec);
        let (at_lo, at_hi) = evals_ordered(&bound, &lo, &hi);
        prop_assert!(
            ref_le(at_lo, at_hi),
            "{bound} evaluated to {at_lo:?} at {lo:?} but {at_hi:?} at the larger {hi:?}"
        );
    }

    /// AC3, driven through `BoundShape::ALL` so that every constructor is
    /// covered by construction. A seventh variant fails to compile in
    /// `spec_of_shape` rather than silently going untested.
    #[test]
    fn every_constructor_is_monotone(
        a in arb_spec(),
        b in arb_spec(),
        base in arb_base_u32(),
        log in any::<bool>(),
        (lo, hi) in arb_env_pair(),
    ) {
        for shape in BoundShape::ALL {
            let spec = spec_of_shape(shape, a.clone(), b.clone(), base, log);
            let bound = build(&spec);
            let (at_lo, at_hi) = evals_ordered(&bound, &lo, &hi);
            prop_assert!(
                ref_le(at_lo, at_hi),
                "{shape:?} node {bound} gave {at_lo:?} at {lo:?} and {at_hi:?} at {hi:?}"
            );
        }
    }

    /// "For random `b1`, `b2` and any constructor, monotonicity holds under
    /// pointwise argument increase" - the argument-wise reading, where the
    /// *operands* grow and the valuation is held fixed.
    #[test]
    fn every_operand_taking_constructor_is_monotone_in_its_operands(
        (small, large) in arb_ordered_refs(),
        other in arb_spec(),
        base in arb_base_u32(),
        log in any::<bool>(),
        env in crate::support::arb_env(),
    ) {
        let valuation = env.valuation();
        let mut exercised = 0usize;
        for shape in BoundShape::ALL.into_iter().filter(|s| shape_takes_operands(*s)) {
            exercised += 1;
            let lesser = build(&spec_of_shape(
                shape,
                BoundSpec::Const(small),
                other.clone(),
                base,
                log,
            ));
            let greater = build(&spec_of_shape(
                shape,
                BoundSpec::Const(large),
                other.clone(),
                base,
                log,
            ));
            let at_lesser = nat_ref(lesser.eval(&valuation));
            let at_greater = nat_ref(greater.eval(&valuation));
            prop_assert!(
                ref_le(at_lesser, at_greater),
                "{shape:?}: raising an operand from {small:?} to {large:?} lowered \
                 {at_lesser:?} to {at_greater:?}"
            );
        }
        // `Const` and `Var` take no operands, so the loop skips them. Pinned
        // so the skip cannot silently grow and hollow the property out.
        prop_assert_eq!(exercised, 4, "the operand-taking constructors are Sum, Max, Prod, Trans");
    }

    /// All argument-wise monotonicity lives in five `Nat` methods. If they are
    /// monotone, the algebra is; if one of them is not, no amount of care in
    /// the constructors recovers it.
    #[test]
    fn the_five_nat_methods_are_monotone(
        (small, large) in arb_ordered_refs(),
        other in arb_ref(),
        base in arb_base_u32(),
    ) {
        let (lo, hi) = (ref_nat(small), ref_nat(large));
        let rhs = ref_nat(other);
        let k = base_of(base);

        prop_assert!(
            ref_le(nat_ref(lo.plus(rhs)), nat_ref(hi.plus(rhs))),
            "plus is not monotone at ({small:?}, {large:?}, {other:?})"
        );
        prop_assert!(
            ref_le(nat_ref(lo.times(rhs)), nat_ref(hi.times(rhs))),
            "times is not monotone at ({small:?}, {large:?}, {other:?})"
        );
        prop_assert!(
            ref_le(nat_ref(lo.join(rhs)), nat_ref(hi.join(rhs))),
            "join is not monotone at ({small:?}, {large:?}, {other:?})"
        );
        prop_assert!(
            ref_le(nat_ref(lo.exp_of(k)), nat_ref(hi.exp_of(k))),
            "exp_of is not monotone in the exponent at ({small:?}, {large:?}, base {base})"
        );
        prop_assert!(
            ref_le(nat_ref(lo.ceil_log(k)), nat_ref(hi.ceil_log(k))),
            "ceil_log is not monotone in the argument at ({small:?}, {large:?}, base {base})"
        );
    }

    /// `Bound::subst` is closed under monotonicity: monotone in, monotone out.
    /// This is the seam LAN-57 builds on, so it is checked directly rather
    /// than inferred from "a composition of monotone functions is monotone".
    #[test]
    fn substitution_preserves_monotonicity(
        spec in arb_spec(),
        replacement in arb_spec(),
        (lo, hi) in arb_env_pair(),
    ) {
        let bound = build(&spec);
        let repl = build(&replacement);
        let substituted = bound.subst(&landav_bound::VarId::new("x0"), &repl);
        let (at_lo, at_hi) = evals_ordered(&substituted, &lo, &hi);
        prop_assert!(
            ref_le(at_lo, at_hi),
            "{substituted} is not monotone: {at_lo:?} at {lo:?}, {at_hi:?} at {hi:?}"
        );
    }
}

/// `Base >= 2` is what makes `Pow` monotone at all: `0^x` is `1, 0, 0, ...`,
/// and `1^x` is constant. Both are unrepresentable, and that is the property.
#[test]
fn bases_below_two_are_unrepresentable() {
    assert!(
        Base::new(0).is_err(),
        "base 0 must be rejected: 0^x is not monotone"
    );
    assert!(
        Base::new(1).is_err(),
        "base 1 must be rejected: log_1 is undefined"
    );
    assert!(Base::new(2).is_ok());
    assert_eq!(Base::TWO.get(), 2);
    assert_eq!(Base::TEN.get(), 10);
}

/// Monotonicity of the whole algebra rests on `omega` being the top. A
/// valuation that sends everything to `omega` must dominate every other
/// valuation, for every term.
#[test]
fn the_omega_valuation_dominates_every_finite_one() {
    let spec = BoundSpec::Prod(vec![
        BoundSpec::Var(0),
        BoundSpec::Sum(vec![
            BoundSpec::Var(1),
            BoundSpec::Trans {
                log: true,
                base: 2,
                arg: Box::new(BoundSpec::Var(0)),
            },
        ]),
    ]);
    let bound = build(&spec);
    let finite = Env {
        vals: [Some(5), Some(7), Some(0)],
        default: Some(3),
    };
    let top = Env::all_omega();
    assert!(finite.le(&top));
    // `ref_le(_, None)` is true for every left-hand side, so asserting it
    // proves nothing. The value is pinned instead:
    // `x0 * (x1 + log2(x0))` at `x0 = 5, x1 = 7` is `5 * (7 + 3) = 50`,
    // because `ceil(log2(5))` is 3.
    let at_finite = nat_ref(bound.eval(&finite.valuation()));
    assert_eq!(at_finite, Some(50), "the finite valuation must be exact");
    assert_eq!(bound.eval(&top.valuation()), Nat::OMEGA);
    assert!(
        ref_le(at_finite, nat_ref(bound.eval(&top.valuation()))) && at_finite.is_some(),
        "the omega valuation must strictly dominate a finite one here"
    );
}
