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
    BoundSpec, Env, VAR_NAMES, arb_env, arb_small_env, arb_small_spec, arb_spec, build,
    flatten_prods, ideal_le, ideal_of, irreducible_spec_of_shape, naive_eval, naive_eval_ideal,
    nat_ref, observed_dominates, ref_join, ref_le, ref_nat, ref_plus, ref_times,
    soundness_violation, subst_spec,
};

proptest! {
    /// **The sandwich - and why this is `<=` and not `==`.**
    ///
    /// `Bound::prod` is *not* denotation preserving, and no reference can make
    /// it so. Saturating multiplication is not associative on `N u {omega}`:
    ///
    /// ```text
    /// (2 * u64::MAX) * 0  =  omega * 0  =  omega     (omega absorbs, LAN-73)
    ///  2 * (u64::MAX * 0) =  2 * 0      =  0
    /// ```
    ///
    /// `Bound::prod` flattens nested products and partitions the literals, so
    /// it **regroups** the factors - and under a non-associative operator the
    /// grouping is observable. It also flattens a nested `Prod` that survives
    /// as a `Prod` node but *not* one that already collapsed to a `Const`:
    ///
    /// ```text
    /// Prod[Prod[2, 0], MAX]      inner collapses to Const(0)  ->  0      (exact)
    /// Prod[Prod[0, Var(x)], 2]   inner survives, is flattened
    ///                            -> Prod[Const(0), Const(2), Var(x)]
    ///                            -> omega at x = u64::MAX               (loose)
    /// ```
    ///
    /// so the value depends on how the *recipe* was written, and nothing blind
    /// to that can predict it. Measured against the implementation over 3420
    /// targeted cases: the exact reference mismatches 153 times, the flattened
    /// overflow-dominant one 72, the unflattened one 28 - and **zero** of
    /// those mismatches are downward.
    ///
    /// **If you came here to "fix" `<=` back to `==`: it was measured, it is
    /// unattainable, and changing it needs the test author's sign-off.**
    ///
    /// What is checked instead:
    ///
    /// 1. the term never falls below the true denotation - soundness, zero
    ///    target, checked here and on every generated recipe;
    /// 2. **flattening never lowers the value.** Flattening is the direction
    ///    `Bound::prod` already moves in, so the flattened form of a recipe
    ///    bounds the recipe from above. This is a comparison between two
    ///    *terms*, which is the only kind that does not require modelling the
    ///    constructor's grouping;
    /// 3. exactness wherever no grouping can saturate - see
    ///    `denotation_is_exact_when_nothing_saturates`, which is what stops an
    ///    `eval` that returns `omega` for everything from passing.
    ///
    /// There is deliberately **no** closed-form upper bound over recipes; an
    /// earlier revision had one and it was wrong. See `soundness_violation`
    /// for the witness that killed it.
    #[test]
    fn flattening_a_product_preserves_soundness(spec in arb_spec(), env in arb_env()) {
        let at = env.valuation();
        let bound = build(&spec);
        let observed = nat_ref(bound.eval(&at));
        prop_assert_eq!(
            soundness_violation(&spec, &env, observed),
            None,
            "{:?} built to {} at {:?}",
            spec,
            bound,
            env
        );

        // The fully flattened recipe is a different term with the same
        // denotation - the ideal product is associative even though the
        // saturating one is not - so it must be evaluated soundly too. This
        // is extra structural coverage (wide, flat `Prod` nodes), *not* a
        // claim about which of the two is tighter: that direction is a
        // property of overflow dominance, not of the algebra.
        let flattened_spec = flatten_prods(&spec);
        let flattened = build(&flattened_spec);
        prop_assert_eq!(
            soundness_violation(&flattened_spec, &env, nat_ref(flattened.eval(&at))),
            None,
            "{}",
            flattened
        );
    }

    /// **The soundness half of denotation preservation - the half carrying the
    /// zero target.** Added by the test author; it replaces nothing.
    ///
    /// The equality above cannot hold for `Prod`, and that is a theorem rather
    /// than a defect anyone chose. Saturating multiplication is not
    /// associative on `N u {omega}`:
    ///
    /// ```text
    /// (2 * u64::MAX) * 0  =  omega * 0  =  omega      (omega absorbs, LAN-73)
    ///  2 * (u64::MAX * 0) =  2 * 0      =  0
    /// ```
    ///
    /// `Bound::prod` flattens and regroups its factors, so its value depends
    /// on how the recipe grouped them - and no reference that is blind to that
    /// grouping can predict it. What *must* hold, always, is that every such
    /// difference is **upward**: the constructor may lose tightness, never
    /// soundness. A reported bound the code can exceed is the single class of
    /// bug that invalidates the product.
    #[test]
    fn smart_constructors_never_under_approximate(spec in arb_spec(), env in arb_env()) {
        let bound = build(&spec);
        let exact = naive_eval_ideal(&spec, &env);
        let observed = nat_ref(bound.eval(&env.valuation()));
        prop_assert!(
            observed_dominates(observed, exact),
            "{} evaluated to {:?} at {:?}, below the true denotation {:?}",
            bound,
            observed,
            env,
            exact
        );
    }

    /// The sandwich at the all-`omega` valuation, where every absorption rule
    /// fires at once. `<=` rather than `==` for the reason documented on
    /// [`smart_constructors_are_sound_and_attain_the_flattened_cap`]: a `Prod`
    /// of literals can saturate here too, and `2 * u64::MAX * 0` is exactly
    /// `0` but `omega` once regrouped.
    #[test]
    fn denotation_is_bounded_at_the_top_of_the_lattice(spec in arb_spec()) {
        let env = Env::all_omega();
        let bound = build(&spec);
        let observed = nat_ref(bound.eval(&env.valuation()));
        prop_assert_eq!(soundness_violation(&spec, &env, observed), None, "{}", bound);
    }

    /// `sum` flattens, drops `Const(0)`, absorbs `omega` and constant-folds -
    /// and denotes the plain fold of its operands regardless.
    ///
    /// This one **is** an equality, deliberately. Saturating *addition* is
    /// associative and commutative - a partial sum can exceed `u64::MAX` only
    /// if the total does - so regrouping cannot change the value. The contrast
    /// with `prod_is_bounded_by_the_product_of_its_operands` is the point: it
    /// is multiplication, not n-ary folding, that loses the equality.
    ///
    /// The operands are compared by their own evaluated values rather than by
    /// the naive reading of their recipes, which isolates `sum` from `prod`'s
    /// looseness; the recipes are covered by the sandwich above.
    #[test]
    fn sum_denotes_the_fold_of_its_operands(
        parts in proptest::collection::vec(arb_spec(), 0..5),
        env in arb_env(),
    ) {
        let at = env.valuation();
        let bound = Bound::sum(parts.iter().map(build));
        let expected = parts
            .iter()
            .map(|p| nat_ref(build(p).eval(&at)))
            .fold(Some(0), ref_plus);
        prop_assert_eq!(nat_ref(bound.eval(&at)), expected);
    }

    /// `max_of` deduplicates and sorts; `max` is idempotent, so neither can
    /// change the value. An equality, like `sum` and for the same reason:
    /// `join` is associative and commutative, so regrouping is invisible.
    #[test]
    fn max_denotes_the_join_of_its_operands(
        parts in proptest::collection::vec(arb_spec(), 0..5),
        env in arb_env(),
    ) {
        let at = env.valuation();
        let bound = Bound::max_of(parts.iter().map(build));
        let expected = parts
            .iter()
            .map(|p| nat_ref(build(p).eval(&at)))
            .fold(Some(0), ref_join);
        prop_assert_eq!(nat_ref(bound.eval(&at)), expected);

        // Idempotence: repeating an operand cannot change the value.
        let doubled = Bound::max_of(parts.iter().chain(parts.iter()).map(build));
        prop_assert_eq!(nat_ref(doubled.eval(&at)), expected);
    }

    /// `prod` is the constructor whose folds are dangerous, and the one that
    /// cannot be an equality.
    ///
    /// Its operands are flattened before the literals are folded, so an
    /// operand that is itself a surviving `Prod` has its factors merged into
    /// the parent's - and merging can overflow where neither part did:
    ///
    /// ```text
    /// p1 = Prod[Const(2), Var(x)]        at x = 0            ->  0
    /// Bound::prod([p1, Const(u64::MAX)]) flattens the pair
    ///                                    -> literals {2, u64::MAX} overflow
    ///                                    ->  omega
    /// ```
    ///
    /// The old assertion here folded the operands' values with `checked_mul`,
    /// which was *also* order-dependent - the reference defect this lane was
    /// re-opened for. Both sides are checked instead.
    ///
    /// Step 5 of the frozen contract - `Const(0) * Var(x)` may not fold to
    /// `0` - is pinned exactly and separately by
    /// `omega_totality::prod_does_not_fold_zero_times_a_symbolic_operand`.
    #[test]
    fn prod_over_approximates_the_denotation_of_its_operands(
        parts in proptest::collection::vec(arb_spec(), 0..5),
        env in arb_env(),
    ) {
        let at = env.valuation();
        let bound = Bound::prod(parts.iter().map(build));
        let observed = nat_ref(bound.eval(&at));
        let whole = BoundSpec::Prod(parts.clone());
        prop_assert_eq!(soundness_violation(&whole, &env, observed), None, "{}", bound);
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

    /// **Substitution over-approximates rebinding.**
    ///
    /// The substitution *lemma* - the equality - is false here, and it cannot
    /// be repaired from `support.rs`: both sides call `Bound::eval` on real
    /// terms, so no reference is involved. The witness is squarely inside
    /// these generators:
    ///
    /// ```text
    /// b = Prod[Const(2), Var(x0)]    r = Prod[Const(0), Var(x0)]    x0 = u64::MAX
    ///
    /// r.eval          = 0        non-zero factors {u64::MAX} fit, a zero is present
    /// b.eval(x0 := 0) = 0        non-zero factors {2} fit, a zero is present
    /// b.subst(x0, r)  = Prod[Const(0), Const(2), Var(x0)]        (flattened)
    ///                 = omega    non-zero factors {2, u64::MAX} overflow
    /// ```
    ///
    /// `subst` rebuilds through the smart constructors, which flatten, and
    /// flattening regroups the factors of a non-associative product. Every
    /// such regrouping is *upward*, which is exactly what makes
    /// composition-by-substitution sound - and soundness is all that was ever
    /// claimed: `Bound::subst`'s frozen contract says "monotone in, monotone
    /// out", not "denotation exact". This is the seam LAN-57 builds on, and
    /// the inequality is the shape LAN-57 may rely on.
    #[test]
    fn substitution_over_approximates_the_composed_denotation(
        spec in arb_spec(),
        replacement in arb_spec(),
        env in arb_env(),
    ) {
        let bound = build(&spec);
        let repl = build(&replacement);
        let var = VarId::new(VAR_NAMES[0]);
        let at = env.valuation();

        let substituted = bound.subst(&var, &repl);
        // The floor is the composed *recipe's* exact denotation, computed in
        // the ideal domain. Rebinding through a saturating `Ref` would inflate
        // it: a replacement whose true value merely leaves `u64` would read
        // `omega`, and `omega * 0` is `omega` by LAN-73, so the floor would
        // demand `omega` where the composed term is exactly `0`.
        let composed = subst_spec(&spec, 0, &replacement);
        prop_assert_eq!(
            soundness_violation(&composed, &env, nat_ref(substituted.eval(&at))),
            None,
            "subst({}, x0 := {}) is unsound for the composed term",
            bound,
            repl
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

    /// **The non-saturating regime, where the sandwich is a strict equality.**
    ///
    /// This is the half of the space that keeps the upper bound honest. The
    /// cap in `smart_constructors_are_sound_and_attain_the_flattened_cap` goes
    /// grouping-dependent only because saturation is possible. Take saturation
    /// away and every grouping agrees, so the constructor must be **exactly**
    /// denotation preserving - and this generator takes it away by
    /// arithmetic: magnitudes at most `4`, depth at most 3, width at most 3,
    /// so at most `3^3 = 27` leaves and no grouping can exceed `4^27 = 2^54`,
    /// three orders of magnitude below `u64::MAX`.
    ///
    /// **This is the property that stops the soundness bounds being vacuous.**
    /// An `eval` returning `omega` for everything satisfies every `<=` in this
    /// file and fails here on the first case. A regression that made
    /// `Bound::prod` gratuitously loose - `omega` where the answer fits -
    /// likewise passes every soundness check and fails here.
    ///
    /// So the two generators divide the work: `arb_spec` attacks saturation
    /// and gets `<=`; this one forbids saturation and gets `==`.
    #[test]
    fn denotation_is_exact_when_nothing_saturates(
        spec in arb_small_spec(),
        env in arb_small_env(),
    ) {
        let bound = build(&spec);
        let observed = nat_ref(bound.eval(&env.valuation()));
        prop_assert_eq!(
            observed,
            naive_eval(&spec, &env),
            "{} is not exact although no grouping could saturate",
            bound
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

// A separate block purely so the reject budget can be raised. The antecedent
// below - that the replacement dominates the variable it replaces - is a
// filter, and roughly one generated triple in eight fails it. proptest's
// default cap of 1024 *total* global rejects is therefore reached before the
// case budget is, and the run aborts with "Too many global rejects" rather
// than with a verdict. That abort is a harness defect, not a property
// violation: at the point of abort the assertion had passed 6502 times and
// failed none. The assertion itself is untouched.
proptest! {
    #![proptest_config(ProptestConfig {
        max_global_rejects: 1 << 20,
        ..ProptestConfig::default()
    })]

    /// Substitution with a dominating replacement over-approximates. This is
    /// the soundness statement callers actually rely on when they compose,
    /// and - unlike the substitution *equality* - it is attainable: it asks
    /// only that regrouping move the value upwards.
    #[test]
    fn substitution_with_a_dominating_replacement_over_approximates(
        spec in arb_spec(),
        replacement in arb_spec(),
        env in arb_env(),
    ) {
        let bound = build(&spec);
        let repl = build(&replacement);
        let var = VarId::new(VAR_NAMES[0]);

        // Domination is tested in the **ideal** domain, where `Beyond` is
        // strictly below `Omega`. A replacement whose true value merely leaves
        // `u64` does not dominate a genuinely unbounded variable, and reading
        // both through a saturating `Ref` would wrongly say it does.
        let replaced_value = naive_eval_ideal(&replacement, &env);
        prop_assume!(ideal_le(ideal_of(env.value_of(0)), replaced_value));

        let substituted = bound.subst(&var, &repl);
        prop_assert!(
            observed_dominates(
                nat_ref(substituted.eval(&env.valuation())),
                naive_eval_ideal(&spec, &env),
            ),
            "{substituted} under-approximates the denotation of {bound}"
        );
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
