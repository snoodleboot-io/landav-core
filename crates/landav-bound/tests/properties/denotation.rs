//! **Smart constructors must be denotation preserving.**
//!
//! Every fold in `Bound::sum`, `Bound::max_of`, `Bound::prod`, `Bound::pow`
//! and `Bound::log` is an optimisation of the *representation*, and none of
//! them may change the *function*. The property that catches an over-eager
//! fold is stated once, over arbitrary terms and arbitrary valuations: the
//! constructed term evaluates identically to the naive interpretation of the
//! recipe it was built from.
//!
//! `crate::support::naive_eval` is the naive interpretation. It does not
//! flatten, fold, absorb, deduplicate or sort, and it never calls a method on
//! `Nat`, so a constructor and its reference cannot be wrong in the same way.
//!
//! This file also carries LAN-56 AC1 - all six constructors reachable and
//! observable - and the substitution lemma that
//! composition-by-substitution rests on.

use landav_bound::{Bound, BoundShape, Nat, VarId};
use proptest::prelude::*;

use crate::support::{
    BoundSpec, Env, VAR_NAMES, arb_env, arb_spec, build, irreducible_spec_of_shape, naive_eval,
    nat_ref, ref_join, ref_le, ref_nat, ref_plus, ref_times,
};

proptest! {
    /// The property. For any term and any valuation, the constructed term
    /// evaluates identically to the naive interpretation.
    #[test]
    fn smart_constructors_are_denotation_preserving(spec in arb_spec(), env in arb_env()) {
        let bound = build(&spec);
        prop_assert_eq!(
            nat_ref(bound.eval(&env.valuation())),
            naive_eval(&spec, &env),
            "{:?} built to {} which denotes something else at {:?}",
            spec,
            bound,
            env
        );
    }

    /// Same property, at the all-`omega` valuation, where every absorption
    /// rule fires at once.
    #[test]
    fn denotation_is_preserved_at_the_top_of_the_lattice(spec in arb_spec()) {
        let env = Env::all_omega();
        let bound = build(&spec);
        prop_assert_eq!(nat_ref(bound.eval(&env.valuation())), naive_eval(&spec, &env));
    }

    /// `sum` folds, flattens, drops zeros and absorbs `omega` - and denotes
    /// the plain fold of its operands regardless.
    #[test]
    fn sum_denotes_the_fold_of_its_operands(
        parts in proptest::collection::vec(arb_spec(), 0..5),
        env in arb_env(),
    ) {
        let bound = Bound::sum(parts.iter().map(build));
        let expected = parts
            .iter()
            .map(|p| naive_eval(p, &env))
            .fold(Some(0), ref_plus);
        prop_assert_eq!(nat_ref(bound.eval(&env.valuation())), expected);
    }

    /// `max_of` deduplicates and sorts; `max` is idempotent, so neither can
    /// change the value.
    #[test]
    fn max_denotes_the_join_of_its_operands(
        parts in proptest::collection::vec(arb_spec(), 0..5),
        env in arb_env(),
    ) {
        let bound = Bound::max_of(parts.iter().map(build));
        let expected = parts
            .iter()
            .map(|p| naive_eval(p, &env))
            .fold(Some(0), ref_join);
        prop_assert_eq!(nat_ref(bound.eval(&env.valuation())), expected);

        // Idempotence: repeating an operand cannot change the value.
        let doubled = Bound::max_of(parts.iter().chain(parts.iter()).map(build));
        prop_assert_eq!(nat_ref(doubled.eval(&env.valuation())), expected);
    }

    /// `prod` is the constructor whose folds are dangerous: step 5 may not
    /// collapse `Const(0) * anything` to zero.
    #[test]
    fn prod_denotes_the_product_of_its_operands(
        parts in proptest::collection::vec(arb_spec(), 0..5),
        env in arb_env(),
    ) {
        let bound = Bound::prod(parts.iter().map(build));
        let expected = parts
            .iter()
            .map(|p| naive_eval(p, &env))
            .fold(Some(1), ref_times);
        prop_assert_eq!(nat_ref(bound.eval(&env.valuation())), expected);
    }

    /// Arity 0 and arity 1 collapse to the documented values, and the
    /// collapses are denotation preserving.
    #[test]
    fn empty_and_singleton_nodes_collapse_as_documented(spec in arb_spec(), env in arb_env()) {
        let only = build(&spec);
        prop_assert_eq!(Bound::sum([]), Bound::zero());
        prop_assert_eq!(Bound::max_of([]), Bound::zero());
        prop_assert_eq!(Bound::prod([]), Bound::one());
        prop_assert_eq!(Bound::sum([only.clone()]), only.clone());
        prop_assert_eq!(Bound::max_of([only.clone()]), only.clone());
        prop_assert_eq!(Bound::prod([only.clone()]), only.clone());

        let at = env.valuation();
        prop_assert_eq!(Bound::zero().eval(&at), Nat::ZERO);
        prop_assert_eq!(Bound::one().eval(&at), Nat::ONE);
        prop_assert_eq!(Bound::omega().eval(&at), Nat::OMEGA);
    }

    /// **The substitution lemma.** Substituting a term for a variable is the
    /// same as evaluating in an environment where that variable already has
    /// the term's value. This is what makes composition-by-substitution sound
    /// rather than merely plausible, and it is the seam LAN-57 builds on.
    #[test]
    fn substitution_agrees_with_rebinding(
        spec in arb_spec(),
        replacement in arb_spec(),
        env in arb_env(),
    ) {
        let bound = build(&spec);
        let repl = build(&replacement);
        let var = VarId::new(VAR_NAMES[0]);

        let substituted = bound.subst(&var, &repl);
        let value_of_replacement = naive_eval(&replacement, &env);
        let rebound = env.with(0, value_of_replacement);

        prop_assert_eq!(
            nat_ref(substituted.eval(&env.valuation())),
            nat_ref(bound.eval(&rebound.valuation())),
            "subst({}, x0 := {}) does not agree with rebinding x0",
            bound,
            repl
        );
    }

    /// Substitution with a dominating replacement over-approximates. This is
    /// the soundness statement callers actually rely on when they compose.
    #[test]
    fn substitution_with_a_dominating_replacement_over_approximates(
        spec in arb_spec(),
        replacement in arb_spec(),
        env in arb_env(),
    ) {
        let bound = build(&spec);
        let repl = build(&replacement);
        let var = VarId::new(VAR_NAMES[0]);

        let replaced_value = naive_eval(&replacement, &env);
        prop_assume!(ref_le(env.value_of(0), replaced_value));

        let substituted = bound.subst(&var, &repl);
        prop_assert!(
            ref_le(
                nat_ref(bound.eval(&env.valuation())),
                nat_ref(substituted.eval(&env.valuation())),
            ),
            "{substituted} under-approximates {bound}"
        );
    }

    /// Substituting a variable that cannot occur returns the same term - and
    /// `may_contain_var` may never produce a false negative, which would let
    /// `subst` skip a subtree that needed rewriting and leave a stale free
    /// variable in a term reported as closed form.
    #[test]
    fn substitution_of_an_absent_variable_is_the_identity(spec in arb_spec()) {
        let bound = build(&spec);
        let absent = VarId::new("not-a-generated-name");
        prop_assert!(!bound.may_contain_var(&absent) || bound.vars().contains(&absent));
        prop_assert_eq!(bound.subst(&absent, &Bound::omega()), bound.clone());

        for var in bound.vars() {
            prop_assert!(
                bound.may_contain_var(&var),
                "{bound} lists {var} in vars() but denies it in may_contain_var"
            );
        }
    }

    /// `vars()` is sorted ascending, deduplicated, and drawn only from the
    /// recipe. Sorted rather than merely canonical because it reaches
    /// `BoundError::UnboundVariable`, which reaches a CI log diff.
    #[test]
    fn vars_is_sorted_deduplicated_and_contained(spec in arb_spec()) {
        let bound = build(&spec);
        let listed = bound.vars();

        for pair in listed.windows(2) {
            prop_assert!(pair[0] < pair[1], "vars() is not sorted and deduplicated: {listed:?}");
        }

        // Absorption may drop a variable (`omega + x` is `omega`), so
        // containment runs one way only: every reported variable must come
        // from the recipe, and none may be invented.
        let mut indices = Vec::new();
        spec.var_indices(&mut indices);
        let from_recipe: Vec<VarId> = indices
            .iter()
            .map(|i| VarId::new(VAR_NAMES[*i % VAR_NAMES.len()]))
            .collect();
        for var in &listed {
            prop_assert!(
                from_recipe.contains(var),
                "vars() reported {var}, which {spec:?} does not mention"
            );
        }
    }

    /// `is_finite` is a syntactic property, and a term whose recipe mentions
    /// `omega` can never lose it, because `omega` absorbs unconditionally.
    #[test]
    fn omega_in_the_recipe_survives_into_the_term(spec in arb_spec()) {
        let bound = build(&spec);
        if spec.mentions_omega() {
            prop_assert!(
                !bound.is_finite(),
                "{bound} lost the omega that {spec:?} contains"
            );
        }
    }

    /// Denotation preservation again, restricted to small finite valuations,
    /// where nothing saturates and every fold is exercised on values the
    /// reference can also compute exactly.
    #[test]
    fn denotation_is_preserved_at_small_finite_valuations(
        spec in arb_spec(),
        env in arb_env(),
    ) {
        let bound = build(&spec);
        let finite_env = Env {
            vals: [
                Some(env.value_of(0).unwrap_or(3) % 16),
                Some(env.value_of(1).unwrap_or(5) % 16),
                Some(env.value_of(2).unwrap_or(7) % 16),
            ],
            default: Some(1),
        };
        prop_assert_eq!(
            nat_ref(bound.eval(&finite_env.valuation())),
            naive_eval(&spec, &finite_env)
        );
    }

    /// `wire_node_count` reports what `to_wire` will emit, so a caller can
    /// check the serialised size before serialising.
    #[test]
    fn wire_round_trip_rebuilds_the_same_term(spec in arb_spec()) {
        let bound = build(&spec);
        prop_assert!(bound.wire_node_count() >= 1);
        // A budget refusal from `to_wire` is a legitimate outcome; a panic is
        // not, and neither is emitting a document this crate cannot read back.
        if let Ok(wire) = bound.to_wire() {
            match Bound::try_from_wire(&wire) {
                Ok(rebuilt) => {
                    let (there, back) = (bound.canonical_bytes(), rebuilt.canonical_bytes());
                    prop_assert_eq!(back.as_bytes(), there.as_bytes());
                }
                Err(e) => prop_assert!(false, "a wire form this crate emitted was rejected: {e}"),
            }
        }
    }
}

/// **LAN-56 AC1.** All six constructors are reachable and observable, and
/// `BoundShape::ALL` drives the check, so a seventh variant is a compile error
/// here as well as in the algebra.
#[test]
fn all_six_constructors_are_reachable_and_observable() {
    assert_eq!(BoundShape::ALL.len(), 6);

    for shape in BoundShape::ALL {
        let bound = build(&irreducible_spec_of_shape(shape));
        assert_eq!(bound.shape(), shape, "{bound} did not observe as {shape:?}");
    }

    // And the six are pairwise distinct terms.
    let terms: Vec<Bound> = BoundShape::ALL
        .iter()
        .map(|shape| build(&irreducible_spec_of_shape(*shape)))
        .collect();
    for (i, left) in terms.iter().enumerate() {
        for (j, right) in terms.iter().enumerate() {
            assert_eq!(i == j, left == right, "{left} vs {right}");
        }
    }
}

/// Depth is derived data, computed by the constructors, never accepted as a
/// parameter, and always within the limit.
#[test]
fn depth_is_derived_and_bounded() {
    assert_eq!(Bound::zero().depth(), Bound::one().depth());
    assert_eq!(Bound::omega().depth(), Bound::zero().depth());
    assert_eq!(Bound::var("x").depth(), Bound::zero().depth());

    let nested = Bound::sum([
        Bound::var("x"),
        Bound::prod([Bound::var("y"), Bound::var("z")]),
    ]);
    assert!(nested.depth() > Bound::var("x").depth());
    assert!(nested.depth() <= landav_bound::MAX_DEPTH);

    // Derived data must not reach equality: two structurally identical terms
    // are equal whatever path built them.
    let direct = Bound::sum([Bound::var("x"), Bound::var("y")]);
    let via_nested = Bound::sum([Bound::sum([Bound::var("x")]), Bound::var("y")]);
    assert_eq!(direct, via_nested);
}

/// `Const(0)` means exactly one thing: proved to cost nothing. It is not the
/// additive identity of any registered semiring, and it must never arise by
/// accident - there is no `Default` for `Bound`.
#[test]
fn zero_one_and_omega_are_distinct_meaning_critical_values() {
    assert_ne!(Bound::zero(), Bound::one());
    assert_ne!(Bound::zero(), Bound::omega());
    assert_ne!(Bound::one(), Bound::omega());
    assert_eq!(Bound::zero(), Bound::constant(0));
    assert_eq!(Bound::one(), Bound::constant(1));
    assert_eq!(Bound::omega(), Bound::magnitude(Nat::OMEGA));
    assert!(Bound::zero().is_finite());
    assert!(!Bound::omega().is_finite());
}

/// `Display` renders in **canonical** operand order. There is deliberately no
/// second presentation order, so LAN-57's acceptance criterion is restated
/// against this rendering rather than against KoAT's source-order string.
#[test]
fn display_uses_canonical_operand_order() {
    let x1 = Bound::var("x1");
    let term = Bound::prod([
        x1.clone(),
        Bound::sum([Bound::log(landav_bound::Base::TWO, x1), Bound::constant(2)]),
    ]);
    let rendered = term.to_string();
    let rebuilt_the_other_way = Bound::prod([
        Bound::sum([
            Bound::constant(2),
            Bound::log(landav_bound::Base::TWO, Bound::var("x1")),
        ]),
        Bound::var("x1"),
    ]);
    assert_eq!(
        rendered,
        rebuilt_the_other_way.to_string(),
        "Display must not expose the order operands were supplied in"
    );
    assert!(!rendered.is_empty());
}

/// The reference semantics used by every property in this file, checked
/// against hand-computed values so a bug in the reference cannot silently
/// excuse a bug in the algebra.
#[test]
fn the_reference_semantics_is_itself_correct() {
    let env = Env {
        vals: [Some(3), Some(0), None],
        default: Some(1),
    };
    let spec = BoundSpec::Sum(vec![
        BoundSpec::Var(0),
        BoundSpec::Prod(vec![BoundSpec::Const(Some(0)), BoundSpec::Var(1)]),
    ]);
    assert_eq!(naive_eval(&spec, &env), Some(3));

    let with_omega = BoundSpec::Prod(vec![BoundSpec::Const(Some(0)), BoundSpec::Var(2)]);
    assert_eq!(
        naive_eval(&with_omega, &env),
        None,
        "0 * omega is omega in the reference too"
    );

    assert_eq!(ref_plus(Some(1), Some(2)), Some(3));
    assert_eq!(ref_times(Some(0), None), None);
    assert_eq!(ref_join(Some(9), None), None);
    assert_eq!(ref_nat(None), Nat::OMEGA);
    assert!(ref_le(Some(u64::MAX), None));
    assert!(!ref_le(None, Some(u64::MAX)));
}
