//! **LAN-57 acceptance criteria, as executable properties.**
//!
//! | LAN-57 AC | where |
//! |---|---|
//! | 1. `subst(bound, var, bound)` is total | `simultaneous_substitution_is_total_and_returns_a_canonical_bound`, `a_single_binding_agrees_with_bound_subst` |
//! | 2. composition of two bounds is a bound (closed) | `composition_of_two_substitutions_is_closed_and_sound`, and the return types of [`Substitution::apply`] and [`Substitution::then`] |
//! | 3. KoAT's worked example, up to structural equality of the canonical form | `the_koat_worked_example_reproduces_up_to_canonical_form` |
//!
//! # The floor rule, restated because this file is where it gets broken
//!
//! `support::precision_violation` records it: **a floor is computed from the
//! reference denotation of a *recipe*, never from `eval` of a term.** `eval`
//! is not compositional - `Bound::prod` flattens and regroups, and saturating
//! multiplication is not associative - so one term's value carries whatever
//! over-approximation its own grouping accumulated, and using it as a lower
//! bound on another term pins that looseness as a contract. Four properties in
//! this crate made that mistake. Every floor below comes from
//! `naive_eval_ideal` of a recipe built by [`subst_spec_simultaneously`].
//!
//! That is also why the two *equalities* a reader will reach for are absent,
//! and each has a witness rather than a comment:
//!
//! * `sigma(b)` evaluated at `v` is **not** `b` evaluated at the rebound
//!   valuation `v'` - see `the_rebound_valuation_equality_has_a_witness`;
//! * `first.then(&second).apply(b)` is **not**
//!   `second.apply(&first.apply(b))` - see
//!   `one_pass_and_two_pass_composition_can_differ_and_both_are_sound`.
//!
//! Both are inequalities in the sound direction, and both are stated that way.

use landav_bound::{Base, Bound, BoundError, BoundKind, BoundShape, Nat, Substitution, VarId};
use proptest::prelude::*;

use crate::support::{
    BoundSpec, Env, Ideal, REF_OMEGA, VAR_NAMES, arb_env, arb_env_pair, arb_small_env,
    arb_small_spec, arb_spec, build, canonical_violation, ideal_le, ideal_of, ideal_ref,
    naive_eval, naive_eval_ideal, nat_ref, observed_dominates, precision_violation, ref_le,
    saturation_free_envelope,
};

/// How many generated variables a substitution can bind.
const SLOTS: usize = VAR_NAMES.len();

/// An image for each generated variable. `None` leaves that variable **free**,
/// which is the contract: an unbound variable is not a failure.
type Images = [Option<BoundSpec>; SLOTS];

/// Recipe-level **simultaneous** substitution: every `Var` leaf is replaced by
/// its image in one pass, and no image is ever re-scanned.
///
/// This is the reference the soundness floors are computed from. It is
/// deliberately not a fold of `support::subst_spec`, which is single-variable
/// and therefore *sequential*: applying it twice models `x := y` followed by
/// `y := z` as `x := z`, which is precisely the re-scanning that would make
/// `x := x + 1` diverge.
fn subst_spec_simultaneously(spec: &BoundSpec, images: &Images) -> BoundSpec {
    match spec {
        BoundSpec::Const(_) => spec.clone(),
        BoundSpec::Var(index) => match &images[index % SLOTS] {
            Some(image) => image.clone(),
            None => spec.clone(),
        },
        BoundSpec::Sum(operands) => BoundSpec::Sum(
            operands
                .iter()
                .map(|operand| subst_spec_simultaneously(operand, images))
                .collect(),
        ),
        BoundSpec::Max(operands) => BoundSpec::Max(
            operands
                .iter()
                .map(|operand| subst_spec_simultaneously(operand, images))
                .collect(),
        ),
        BoundSpec::Prod(operands) => BoundSpec::Prod(
            operands
                .iter()
                .map(|operand| subst_spec_simultaneously(operand, images))
                .collect(),
        ),
        BoundSpec::Trans { log, base, arg } => BoundSpec::Trans {
            log: *log,
            base: *base,
            arg: Box::new(subst_spec_simultaneously(arg, images)),
        },
    }
}

/// The [`Substitution`] the recipe-level model above describes.
fn substitution_of(images: &Images) -> Substitution {
    Substitution::from_bindings(images.iter().enumerate().filter_map(|(slot, image)| {
        image
            .as_ref()
            .map(|recipe| (VarId::new(VAR_NAMES[slot]), build(recipe)))
    }))
}

/// One image, absent about a quarter of the time so the "stays free" path is
/// exercised on most draws rather than never.
fn arb_image() -> impl Strategy<Value = Option<BoundSpec>> {
    prop_oneof![
        1 => Just(None::<BoundSpec>),
        3 => arb_spec().prop_map(Some),
    ]
}

/// An image for each of the three generated variables.
fn arb_images() -> impl Strategy<Value = Images> {
    (arb_image(), arb_image(), arb_image()).prop_map(|(a, b, c)| [a, b, c])
}

/// Small images, for the regime where nothing can saturate.
fn arb_small_image() -> impl Strategy<Value = Option<BoundSpec>> {
    prop_oneof![
        1 => Just(None::<BoundSpec>),
        3 => arb_small_spec().prop_map(Some),
    ]
}

/// Small images for each generated variable.
fn arb_small_images() -> impl Strategy<Value = Images> {
    (arb_small_image(), arb_small_image(), arb_small_image()).prop_map(|(a, b, c)| [a, b, c])
}

/// The arity of an n-ary node, or `None` for the shapes that have none.
fn arity_of(bound: &Bound) -> Option<usize> {
    match bound.kind() {
        BoundKind::Sum(terms) | BoundKind::Prod(terms) => Some(terms.len()),
        BoundKind::Max(terms) => Some(terms.len()),
        BoundKind::Const(_) | BoundKind::Var(_) | BoundKind::Trans { .. } => None,
    }
}

proptest! {
    /// **AC1: substitution is total, and AC2: what comes back is a `Bound`.**
    ///
    /// Totality is asserted by the call returning at all - [`Substitution::apply`]
    /// has no error channel, so a term it cannot build must be widened rather
    /// than reported. Closure is asserted properly rather than by the type:
    /// `canonical_violation` walks the result and checks flatness, arity,
    /// canonical operand order, `Max` deduplication and the depth limit, which
    /// is what "is a bound" has to mean for a type whose invariants live in its
    /// smart constructors.
    ///
    /// The checked form is pinned against the total one in the same breath. It
    /// must agree wherever it succeeds, and where it refuses the total form
    /// must have widened all the way to `omega` - not to some intermediate
    /// term that quietly lost the failure.
    #[test]
    fn simultaneous_substitution_is_total_and_returns_a_canonical_bound(
        spec in arb_spec(),
        images in arb_images(),
    ) {
        let sigma = substitution_of(&images);
        let bound = build(&spec);
        let applied = sigma.apply(&bound);

        prop_assert_eq!(
            canonical_violation(&applied),
            None,
            "{} is not in canonical form after substituting into {}",
            applied,
            bound
        );
        prop_assert!(
            applied.depth() <= landav_bound::MAX_DEPTH,
            "{applied} has depth {} past the limit",
            applied.depth()
        );

        match sigma.apply_checked(&bound) {
            Ok(same) => prop_assert_eq!(
                same,
                applied,
                "apply_checked disagreed with apply on {}",
                bound
            ),
            Err(refusal) => prop_assert_eq!(
                applied,
                Bound::omega(),
                "apply_checked refused with {:?}, so apply must have widened to omega",
                refusal
            ),
        }
    }

    /// **AC1, tied to the seam it builds on.** A one-binding [`Substitution`]
    /// is exactly [`Bound::subst`], for every generated variable.
    ///
    /// This is what stops `subst.rs` becoming a second, subtly different
    /// substitution: `Bound::subst` recurses from the root and this one drives
    /// a worklist from the leaves, and the two must be the same function of
    /// the term, not merely two sound ones.
    #[test]
    fn a_single_binding_agrees_with_bound_subst(
        spec in arb_spec(),
        replacement in arb_spec(),
    ) {
        let bound = build(&spec);
        let repl = build(&replacement);
        for name in VAR_NAMES {
            let var = VarId::new(name);
            prop_assert_eq!(
                Substitution::of(var.clone(), repl.clone()).apply(&bound),
                bound.subst(&var, &repl),
                "Substitution::of({}) disagreed with Bound::subst on {}",
                name,
                bound
            );
        }
    }

    /// **The soundness statement callers rely on when they compose.**
    ///
    /// The floor is the composed *recipe's* exact denotation, in the ideal
    /// domain. Rebinding through a saturating magnitude would inflate it: an
    /// image whose true value merely leaves `u64` reads as `omega`, and
    /// `omega * 0` is `omega`, so the floor would demand `omega` where the
    /// composed term is exactly `0`.
    #[test]
    fn simultaneous_substitution_over_approximates_the_composed_denotation(
        spec in arb_spec(),
        images in arb_images(),
        env in arb_env(),
    ) {
        let applied = substitution_of(&images).apply(&build(&spec));
        let composed = subst_spec_simultaneously(&spec, &images);
        prop_assert_eq!(
            precision_violation(&composed, &env, nat_ref(applied.eval(&env.valuation()))),
            None,
            "{} is unsound for the composed recipe at {:?}",
            applied,
            env
        );
    }

    /// **Monotone in, monotone out.** A composition of weakly monotone
    /// functions is weakly monotone, and that is the whole reason
    /// composition-by-substitution is sound. The valuation pair is ordered
    /// *by construction*, so the antecedent is never merely assumed.
    #[test]
    fn simultaneous_substitution_preserves_monotonicity(
        spec in arb_spec(),
        images in arb_images(),
        (lo, hi) in arb_env_pair(),
    ) {
        let applied = substitution_of(&images).apply(&build(&spec));
        prop_assert!(
            ref_le(
                nat_ref(applied.eval(&lo.valuation())),
                nat_ref(applied.eval(&hi.valuation())),
            ),
            "{applied} fell from {lo:?} to {hi:?}"
        );
    }

    /// **Substitution commutes with evaluation under a rebound valuation - as
    /// an inequality.**
    ///
    /// Two halves, and the first is a check on the *checking apparatus*:
    ///
    /// 1. the reference's own substitution lemma. The composed recipe's ideal
    ///    denotation at `v` equals the original recipe's at the rebound
    ///    valuation `v'`, where `v'(x)` is the image's ideal denotation at
    ///    `v`. That is an equality, and it holds - in the *reference*, which
    ///    has no regrouping. It is asserted only where every image's value is
    ///    representable, because an [`Ideal::Beyond`] cannot be written into
    ///    an [`Env`] without saturating to `omega` first;
    /// 2. the implementation sits above that value. **Not on it.** See
    ///    `the_rebound_valuation_equality_has_a_witness` for the counterexample
    ///    to the equality form.
    #[test]
    fn substitution_commutes_with_a_rebound_valuation_as_an_inequality(
        spec in arb_spec(),
        images in arb_images(),
        env in arb_env(),
    ) {
        let composed = subst_spec_simultaneously(&spec, &images);
        let exact = naive_eval_ideal(&composed, &env);

        let mut rebound = env.clone();
        let mut representable = true;
        for (slot, image) in images.iter().enumerate() {
            if let Some(recipe) = image {
                let value = naive_eval_ideal(recipe, &env);
                representable &= value != Ideal::Beyond;
                rebound.vals[slot] = ideal_ref(value);
            }
        }
        if representable {
            prop_assert_eq!(
                exact,
                naive_eval_ideal(&spec, &rebound),
                "the reference's own substitution lemma failed: {:?} at {:?}",
                spec,
                env
            );
        }

        let applied = substitution_of(&images).apply(&build(&spec));
        prop_assert!(
            observed_dominates(nat_ref(applied.eval(&env.valuation())), exact),
            "{applied} is below the rebound denotation {exact:?} at {env:?}"
        );
    }

    /// **AC2: the composition of two bounds is a bound, and it is closed.**
    ///
    /// [`Substitution::then`] returns a `Substitution` and
    /// [`Substitution::apply`] returns a `Bound`, so closure is in the types -
    /// but a type does not say the images are *canonical* bounds, and
    /// composition is exactly where a non-canonical one would be minted, since
    /// each image is itself the result of a substitution. So every image is
    /// walked, and so are both applications.
    ///
    /// The soundness floor is the doubly-composed recipe, and **both** orders
    /// are held to it: composing first and applying once, and applying twice.
    /// They are not required to agree - see
    /// `one_pass_and_two_pass_composition_can_differ_and_both_are_sound`.
    #[test]
    fn composition_of_two_substitutions_is_closed_and_sound(
        spec in arb_spec(),
        first_images in arb_images(),
        second_images in arb_images(),
        env in arb_env(),
    ) {
        let first = substitution_of(&first_images);
        let second = substitution_of(&second_images);
        let composed = first.then(&second);

        for var in composed.domain() {
            match composed.get(&var) {
                Some(image) => prop_assert_eq!(
                    canonical_violation(image),
                    None,
                    "the composed image of {} is not a canonical bound",
                    var
                ),
                None => prop_assert!(false, "domain() listed {var}, which get() denies"),
            }
        }

        let bound = build(&spec);
        let one_pass = composed.apply(&bound);
        let two_pass = second.apply(&first.apply(&bound));
        prop_assert_eq!(canonical_violation(&one_pass), None, "{}", one_pass);
        prop_assert_eq!(canonical_violation(&two_pass), None, "{}", two_pass);

        let recipe = subst_spec_simultaneously(
            &subst_spec_simultaneously(&spec, &first_images),
            &second_images,
        );
        let at = env.valuation();
        prop_assert_eq!(
            precision_violation(&recipe, &env, nat_ref(one_pass.eval(&at))),
            None,
            "the one-pass composition {} is unsound at {:?}",
            one_pass,
            env
        );
        prop_assert_eq!(
            precision_violation(&recipe, &env, nat_ref(two_pass.eval(&at))),
            None,
            "the two-pass composition {} is unsound at {:?}",
            two_pass,
            env
        );
    }

    /// Substituting a variable that does not occur returns an **equal** term,
    /// and the O(1) skip may never fire on a variable that does occur.
    ///
    /// Both directions of [`Substitution::may_rewrite`] are pinned, because
    /// the free summary it consults is allowed to over-approximate: a `true`
    /// costs a walk, a `false` on a variable that is present would silently
    /// leave a stale free variable in a term reported as composed.
    #[test]
    fn substituting_an_absent_variable_returns_an_equal_term(spec in arb_spec()) {
        let bound = build(&spec);
        let absent = VarId::new("not-a-generated-name");
        let sigma = Substitution::of(absent.clone(), Bound::omega());

        prop_assert!(!sigma.may_rewrite(&bound));
        prop_assert_eq!(sigma.apply(&bound), bound.clone());
        prop_assert_eq!(sigma.apply_checked(&bound), Ok(bound.clone()));

        for var in bound.vars() {
            let hits = Substitution::of(var.clone(), Bound::zero());
            prop_assert!(
                hits.may_rewrite(&bound),
                "{bound} contains {var} but may_rewrite denied it"
            );
        }

        prop_assert_eq!(Substitution::identity().apply(&bound), bound.clone());
        prop_assert_eq!(
            Substitution::identity().apply_checked(&bound),
            Ok(bound.clone())
        );
    }

    /// An image that mentions the variable it replaces is spliced in **once**.
    ///
    /// The single-pass rule stated over generated terms: after substituting
    /// `x := image`, every variable left in the result either survived from
    /// the original outside the substitution's domain, or came from an image.
    /// A second pass would put images of images in there.
    #[test]
    fn no_variable_is_substituted_twice(
        spec in arb_spec(),
        images in arb_images(),
    ) {
        let sigma = substitution_of(&images);
        let bound = build(&spec);
        let applied = sigma.apply(&bound);

        let mut admissible: Vec<VarId> = bound
            .vars()
            .into_iter()
            .filter(|var| sigma.get(var).is_none())
            .collect();
        for name in VAR_NAMES {
            if let Some(image) = sigma.get(&VarId::new(name)) {
                admissible.extend(image.vars());
            }
        }

        for var in applied.vars() {
            prop_assert!(
                admissible.contains(&var),
                "{applied} carries {var}, which is neither free in {bound} nor in an image"
            );
        }
    }
}

// A separate block so the reject budget can be raised: both properties below
// filter on an antecedent, and proptest's default cap of 1024 *global* rejects
// is reached long before the case budget is. Raising it changes no assertion.
proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        max_global_rejects: 1 << 20,
        ..ProptestConfig::default()
    })]

    /// **A dominating replacement over-approximates.** If every image is worth
    /// at least the variable it replaces, the substituted term is worth at
    /// least the original - which is the obligation a caller discharges when
    /// it substitutes a size bound into a runtime bound.
    ///
    /// Domination is tested in the **ideal** domain, where `Beyond` is
    /// strictly below `Omega`: an image whose true value merely leaves `u64`
    /// does not dominate a genuinely unbounded variable, and reading both
    /// through a saturating magnitude would wrongly say it does.
    #[test]
    fn a_dominating_replacement_over_approximates(
        spec in arb_spec(),
        images in arb_images(),
        env in arb_env(),
    ) {
        for (slot, image) in images.iter().enumerate() {
            if let Some(recipe) = image {
                prop_assume!(ideal_le(
                    ideal_of(env.value_of(slot)),
                    naive_eval_ideal(recipe, &env),
                ));
            }
        }
        let applied = substitution_of(&images).apply(&build(&spec));
        prop_assert!(
            observed_dominates(
                nat_ref(applied.eval(&env.valuation())),
                naive_eval_ideal(&spec, &env),
            ),
            "{applied} under-approximates the denotation of the term it replaced"
        );
    }

    /// **The anti-vacuity guard.** Take saturation away and every regrouping
    /// agrees, so substitution must be **exactly** denotation preserving.
    ///
    /// Without this, an `apply` that returned `omega` for everything would
    /// satisfy every `<=` in this file. The antecedent is
    /// `saturation_free_envelope`, which is `Some` exactly when *no* grouping
    /// of the composed recipe can leave `u64` - the condition under which the
    /// constructors' regrouping is not merely sound but exact. It is a filter
    /// rather than a generator invariant because composing two small recipes
    /// squares the term, and the arithmetic argument that makes
    /// `arb_small_spec` saturation-free on its own does not survive that.
    #[test]
    fn substitution_is_exact_when_nothing_can_saturate(
        spec in arb_small_spec(),
        images in arb_small_images(),
        env in arb_small_env(),
    ) {
        let composed = subst_spec_simultaneously(&spec, &images);
        prop_assume!(saturation_free_envelope(&composed, &env).is_some());

        let applied = substitution_of(&images).apply(&build(&spec));
        prop_assert_eq!(
            nat_ref(applied.eval(&env.valuation())),
            naive_eval(&composed, &env),
            "{} is not exact although no grouping of the composed recipe could saturate",
            applied
        );
    }
}

// ---------------------------------------------------------------------------
// AC3 - KoAT's worked example
// ---------------------------------------------------------------------------

/// A valuation binding `x1`, with every other variable at `omega`.
fn at_x1(value: Option<u64>) -> Env {
    Env {
        vals: [REF_OMEGA, value, REF_OMEGA],
        default: REF_OMEGA,
    }
}

/// **AC3.** KoAT's worked example, reproduced by composition and asserted **up
/// to structural equality of the canonical form**.
///
/// KoAT derives a runtime bound for each transition separately and composes
/// them: the outer loop runs `x1` times, the inner one `log2(x1) + 2` times,
/// and the program's runtime bound is their product. The composition is a
/// substitution of two *symbolic* runtime bounds into a product skeleton, in
/// one simultaneous step.
///
/// KoAT prints `x1 * (log2(x1) + 2)`, in source order. This crate has exactly
/// one order - the canonical one - and it puts `Const` (tag 0) before `Trans`
/// (tag 5), so the same term renders `x1 * (2 + log2(x1))`. Adding a second
/// presentation order to match KoAT's string would give one value two
/// renderings and make every LAN-58 golden test pin the wrong artefact, so the
/// criterion is restated here against the term:
///
/// * **structural equality** against a hand-built expected term, which is the
///   assertion the acceptance criterion names;
/// * **canonical byte equality**, which is what the F-008 cache keys on and is
///   a strictly finer test than `==`;
/// * **denotational equality**, at eight valuations including `0`, `1` and
///   `omega`, with the expected magnitudes written out rather than computed
///   from either term.
///
/// The rendering is asserted too, but as a *record* of the one order that
/// exists, not as the criterion.
#[test]
fn the_koat_worked_example_reproduces_up_to_canonical_form() {
    let x1 = Bound::var("x1");
    let outer = VarId::new("rt_outer");
    let inner = VarId::new("rt_inner");

    // The skeleton: the program's runtime is the outer loop's runtime times
    // the inner loop's, both still symbolic.
    let skeleton = Bound::prod([Bound::var("rt_outer"), Bound::var("rt_inner")]);

    // The two derived bounds, substituted simultaneously.
    let sigma = Substitution::from_bindings([
        (outer, x1.clone()),
        (
            inner,
            Bound::sum([Bound::log(Base::TWO, x1.clone()), Bound::constant(2)]),
        ),
    ]);

    let composed = sigma.apply(&skeleton);
    let expected = Bound::prod([
        x1.clone(),
        Bound::sum([Bound::log(Base::TWO, x1.clone()), Bound::constant(2)]),
    ]);

    assert_eq!(
        composed, expected,
        "the composed bound is not structurally equal to prod([x1, sum([log2(x1), 2])])"
    );
    assert_eq!(
        composed.canonical_bytes().as_bytes(),
        expected.canonical_bytes().as_bytes(),
        "the composed bound has a different canonical byte form, so it would be a second \
         cache key for one program"
    );
    assert_eq!(composed.shape(), BoundShape::Prod);
    assert_eq!(canonical_violation(&composed), None);

    // Denotation, at magnitudes written out rather than computed from a term.
    for (argument, expected_value) in [
        (Some(0u64), Nat::Fin(0)),
        (Some(1), Nat::Fin(2)),
        (Some(2), Nat::Fin(6)),
        (Some(3), Nat::Fin(12)),
        (Some(7), Nat::Fin(35)),
        (Some(8), Nat::Fin(40)),
        (Some(1024), Nat::Fin(12288)),
        (REF_OMEGA, Nat::OMEGA),
    ] {
        let at = at_x1(argument).valuation();
        assert_eq!(
            composed.eval(&at),
            expected_value,
            "x1 * (2 + log2(x1)) at x1 = {argument:?}"
        );
    }

    // The one rendering there is, recorded. KoAT's own string is
    // `x1 * (log2(x1) + 2)`; a second presentation order to reproduce it is
    // explicitly out of scope.
    assert_eq!(composed.to_string(), "(x1 * (2 + log2(x1)))");
}

// ---------------------------------------------------------------------------
// the single-pass rule, and the two equalities that are false
// ---------------------------------------------------------------------------

/// **Substitution is simultaneous and single-pass.**
///
/// `x := x + 1` terminates and produces `x + 1`, not a divergence and not
/// `x + 2`: the image is spliced in and never re-scanned. And a chain
/// `{x0 := x1, x1 := x2}` maps `x0` to `x1`, not to `x2` - which is the
/// difference between a simultaneous substitution and two sequential ones, and
/// the property no amount of soundness checking would notice.
#[test]
fn substitution_is_simultaneous_and_never_rescans_an_image() {
    let x = VarId::new("x");
    let incremented = Bound::sum([Bound::var("x"), Bound::one()]);
    let sigma = Substitution::of(x.clone(), incremented.clone());

    assert_eq!(sigma.apply(&Bound::var("x")), incremented);
    assert_eq!(
        sigma.apply(&incremented),
        Bound::sum([Bound::var("x"), Bound::constant(2)]),
        "one pass over `x + 1` gives `(x + 1) + 1`, flattened and folded"
    );

    let chained = Substitution::from_bindings([
        (VarId::new("x0"), Bound::var("x1")),
        (VarId::new("x1"), Bound::var("x2")),
    ]);
    assert_eq!(
        chained.apply(&Bound::var("x0")),
        Bound::var("x1"),
        "a simultaneous substitution maps x0 to x1; x2 would mean the image was re-scanned"
    );
    assert_eq!(chained.apply(&Bound::var("x1")), Bound::var("x2"));
    assert_eq!(
        chained.apply(&Bound::max_of([Bound::var("x0"), Bound::var("x1")])),
        Bound::max_of([Bound::var("x1"), Bound::var("x2")])
    );
}

/// **An unbound variable is not a failure - it stays free.**
///
/// Making it an error would push callers toward `unwrap_or(omega)`, which is a
/// sound bound with the blame thrown away. So the checked form succeeds too,
/// and the free variable is still there afterwards.
#[test]
fn an_unbound_variable_stays_free() {
    let term = Bound::sum([Bound::var("bound"), Bound::var("free")]);
    let sigma = Substitution::of(VarId::new("bound"), Bound::constant(3));

    let applied = sigma.apply(&term);
    assert_eq!(
        applied,
        Bound::sum([Bound::constant(3), Bound::var("free")])
    );
    assert!(applied.vars().contains(&VarId::new("free")));
    assert!(!applied.vars().contains(&VarId::new("bound")));
    assert_eq!(sigma.apply_checked(&term), Ok(applied));

    // And nothing in the error surface is reachable from an unbound variable.
    let untouched = Bound::prod([Bound::var("p"), Bound::var("q")]);
    assert_eq!(sigma.apply_checked(&untouched), Ok(untouched.clone()));
    assert_eq!(sigma.apply(&untouched), untouched);
}

/// **The rebound-valuation equality is false.** Here is the witness.
///
/// ```text
/// b = 0 * x0        image(x0) = 2^40 * x1        at x1 = 2^40
///
/// sigma(b)  = 0 * x1             flattened, the zero decides the literals
/// sigma(b) evaluated at x1 = 2^40  ->  0        (exact: the product is zero)
///
/// image evaluated at x1 = 2^40     ->  omega    (2^40 * 2^40 leaves u64)
/// b evaluated at x0 := omega       ->  omega    (omega absorbs, LAN-73)
/// ```
///
/// `0 != omega`, and the gap is the full width of the lattice. Rebinding
/// routes the image's value through a saturating magnitude *before* the
/// enclosing zero is consulted; substituting keeps the zero and the image in
/// one term, where the zero wins. Both answers are sound; only one is tight,
/// and it is the substitution.
///
/// This is why `substitution_commutes_with_a_rebound_valuation_as_an_inequality`
/// is an inequality, and why no floor in this file is ever computed by
/// evaluating a term.
#[test]
fn the_rebound_valuation_equality_has_a_witness() {
    let big = 1u64 << 40;
    let x0 = VarId::new("x0");
    let bound = Bound::prod([Bound::zero(), Bound::var("x0")]);
    let image = Bound::prod([Bound::constant(big), Bound::var("x1")]);

    let env = at_x1(Some(big));
    let at = env.valuation();
    let substituted = Substitution::of(x0, image.clone()).apply(&bound);
    assert_eq!(substituted, Bound::prod([Bound::zero(), Bound::var("x1")]));
    assert_eq!(substituted.eval(&at), Nat::ZERO);

    // The rebound valuation reads the image through a saturating magnitude
    // *before* the enclosing zero is ever consulted.
    let rebound_value = image.eval(&at);
    assert_eq!(rebound_value, Nat::OMEGA);
    let mut rebound = env.clone();
    // `x0` is slot 0 of the generated names, so writing the image's magnitude
    // there is exactly `v[x0 := [[image]](v)]`.
    rebound.vals[0] = REF_OMEGA;
    assert_eq!(bound.eval(&rebound.valuation()), Nat::OMEGA);

    assert_ne!(
        substituted.eval(&at),
        bound.eval(&rebound.valuation()),
        "the substitution lemma's equality form must be false here"
    );
}

/// **One-pass composition and two-pass application can differ**, and both are
/// sound. Here is the witness.
///
/// ```text
/// b     = 0 * x * z        sigma = { x := 2^40 * y }        tau = { y := 2^40 }
///
/// sigma.then(tau)          x := tau(2^40 * y) = 2^40 * 2^40 = omega
///   applied to b           0 * omega * z                    = omega
///
/// tau(sigma(b))            sigma(b) = 0 * y * z             (flattened, zero wins)
///                          tau(that) = 0 * z                = 0 at any finite z
/// ```
///
/// Composing first folds `2^40 * 2^40` in isolation, where nothing can rescue
/// the overflow; applying twice keeps the enclosing zero in scope through both
/// passes, and a zero factor decides a product exactly. `omega` and `0` are
/// the two ends of the lattice, so this is not a rounding difference - which
/// is precisely why `then` is specified as *sound*, never as *equal*.
#[test]
fn one_pass_and_two_pass_composition_can_differ_and_both_are_sound() {
    let big = 1u64 << 40;
    let bound = Bound::prod([Bound::var("x"), Bound::zero(), Bound::var("z")]);
    let sigma = Substitution::of(
        VarId::new("x"),
        Bound::prod([Bound::constant(big), Bound::var("y")]),
    );
    let tau = Substitution::of(VarId::new("y"), Bound::constant(big));

    let one_pass = sigma.then(&tau).apply(&bound);
    let two_pass = tau.apply(&sigma.apply(&bound));

    assert_eq!(one_pass, Bound::omega());
    assert_eq!(two_pass, Bound::prod([Bound::zero(), Bound::var("z")]));
    assert_ne!(one_pass, two_pass);

    // Both are sound: the true denotation of `0 * 2^40 * 2^40 * z` is `0` at
    // every finite `z`, and both answers dominate it.
    let at = Env {
        vals: [REF_OMEGA; SLOTS],
        default: Some(3),
    }
    .valuation();
    assert_eq!(two_pass.eval(&at), Nat::ZERO);
    assert_eq!(one_pass.eval(&at), Nat::OMEGA);
    assert!(observed_dominates(
        nat_ref(one_pass.eval(&at)),
        Ideal::Fin(0)
    ));
    assert!(observed_dominates(
        nat_ref(two_pass.eval(&at)),
        Ideal::Fin(0)
    ));
}

// ---------------------------------------------------------------------------
// composition, as an algebra on substitutions
// ---------------------------------------------------------------------------

/// [`Substitution::then`] composes the images and carries over the second
/// substitution's own bindings, and a binding in the first **shadows** the
/// second's for the same variable.
///
/// Written out entry by entry rather than checked through `apply`, because
/// `apply` cannot see a binding for a variable the term does not mention - and
/// that binding is exactly what the *next* composition will consume.
#[test]
fn then_composes_the_images_and_carries_over_the_second() {
    let (x, y, z) = (VarId::new("x"), VarId::new("y"), VarId::new("z"));

    let first = Substitution::of(x.clone(), Bound::var("y"));
    let second = Substitution::from_bindings([
        (y.clone(), Bound::constant(3)),
        (z.clone(), Bound::constant(5)),
    ]);
    let composed = first.then(&second);

    assert_eq!(composed.len(), 3);
    assert_eq!(composed.domain(), vec![x.clone(), y.clone(), z.clone()]);
    assert_eq!(
        composed.get(&x),
        Some(&Bound::constant(3)),
        "x := y then y := 3 composes to x := 3"
    );
    assert_eq!(composed.get(&y), Some(&Bound::constant(3)));
    assert_eq!(composed.get(&z), Some(&Bound::constant(5)));

    // Shadowing: the first substitution's binding wins, after the second has
    // been applied *to its image*.
    let shadowing = Substitution::of(x.clone(), Bound::constant(1))
        .then(&Substitution::of(x.clone(), Bound::constant(2)));
    assert_eq!(shadowing.get(&x), Some(&Bound::constant(1)));
    assert_eq!(shadowing.len(), 1);

    // And where nothing regroups, composing and applying twice agree.
    let term = Bound::sum([Bound::var("x"), Bound::var("z")]);
    assert_eq!(composed.apply(&term), second.apply(&first.apply(&term)));
    assert_eq!(composed.apply(&term), Bound::constant(8));
}

/// The identity substitution is a two-sided identity for [`Substitution::then`]
/// and the identity function for [`Substitution::apply`].
#[test]
fn the_identity_substitution_is_a_two_sided_identity() {
    let identity = Substitution::identity();
    assert!(identity.is_empty());
    assert_eq!(identity.len(), 0);
    assert_eq!(identity.domain(), Vec::<VarId>::new());

    let sigma = Substitution::from_bindings([
        (VarId::new("x"), Bound::var("y")),
        (VarId::new("w"), Bound::constant(4)),
    ]);
    assert!(!sigma.is_empty());
    assert_eq!(sigma.len(), 2);

    assert_eq!(sigma.then(&identity), sigma);
    assert_eq!(identity.then(&sigma), sigma);

    let term = Bound::prod([Bound::var("x"), Bound::sum([Bound::var("w"), Bound::one()])]);
    assert_eq!(identity.apply(&term), term);
    assert_eq!(identity.apply_checked(&term), Ok(term.clone()));
    assert!(!identity.may_rewrite(&term));
}

/// The accessors report the bindings they were given, and nothing else.
///
/// Asserted directly rather than inferred from `apply`, because an
/// over-approximating [`Substitution::may_rewrite`] - one that always says
/// `true` - produces identical results through `apply` and merely costs a walk
/// of every term. Only a direct assertion distinguishes it.
#[test]
fn the_accessors_report_the_bindings() {
    let x = VarId::new("x");
    let sigma = Substitution::of(x.clone(), Bound::constant(7));

    assert_eq!(sigma.get(&x), Some(&Bound::constant(7)));
    assert_eq!(sigma.get(&VarId::new("other")), None);
    assert_eq!(sigma.len(), 1);
    assert!(!sigma.is_empty());
    assert_eq!(sigma.domain(), vec![x.clone()]);

    assert!(sigma.may_rewrite(&Bound::var("x")));
    assert!(sigma.may_rewrite(&Bound::sum([Bound::var("x"), Bound::one()])));
    assert!(
        !sigma.may_rewrite(&Bound::var("other")),
        "may_rewrite must not answer `true` for a term the substitution cannot touch"
    );
    assert!(!sigma.may_rewrite(&Bound::constant(1)));

    // `from_bindings` takes the **last** binding for a repeated variable.
    let repeated = Substitution::from_bindings([
        (x.clone(), Bound::constant(1)),
        (x.clone(), Bound::constant(2)),
    ]);
    assert_eq!(repeated.len(), 1);
    assert_eq!(repeated.get(&x), Some(&Bound::constant(2)));

    // `domain()` is sorted ascending, whatever order the bindings arrived in.
    let unsorted = Substitution::from_bindings([
        (VarId::new("z"), Bound::one()),
        (VarId::new("a"), Bound::one()),
        (VarId::new("m"), Bound::one()),
    ]);
    assert_eq!(
        unsorted.domain(),
        vec![VarId::new("a"), VarId::new("m"), VarId::new("z")]
    );
}

// ---------------------------------------------------------------------------
// the budgets - substitution grows terms
// ---------------------------------------------------------------------------

/// **The depth budget, at its boundary.** Substitution grows a term, so the
/// limit that the constructors enforce has to be reachable through it.
///
/// A tower of `MAX_DEPTH` nested `log`s over `x` is the tallest term there is.
/// Replacing `x` by another leaf keeps it at exactly `MAX_DEPTH` and must be
/// accepted; replacing it by a term one level deep pushes it to
/// `MAX_DEPTH + 1` and must not be. The total form widens to `omega`, which is
/// sound and monotone; the checked form reports
/// [`BoundError::DepthExceeded`] with the limit it enforced.
#[test]
fn the_depth_budget_is_reachable_through_substitution() {
    let limit = landav_bound::MAX_DEPTH;
    let mut tower = Bound::var("x");
    for _ in 1..limit {
        tower = Bound::log(Base::TWO, tower);
    }
    assert_eq!(tower.depth(), limit, "the tower must start at MAX_DEPTH");

    let x = VarId::new("x");

    // A leaf for a leaf: still exactly at the limit, and accepted.
    let flat = Substitution::of(x.clone(), Bound::var("y"));
    let same_height = flat.apply(&tower);
    assert_eq!(same_height.depth(), limit);
    assert_eq!(flat.apply_checked(&tower), Ok(same_height));

    // One level deeper: refused, with the limit named.
    let taller = Substitution::of(x.clone(), Bound::log(Base::TWO, Bound::var("y")));
    assert_eq!(
        taller.apply(&tower),
        Bound::omega(),
        "the total form widens where the checked one refuses"
    );
    let refusal = taller.apply_checked(&tower);
    assert!(
        matches!(&refusal, Err(BoundError::DepthExceeded { limit: named }) if *named == limit),
        "depth {} must be refused as DepthExceeded({limit}), not {refusal:?}",
        limit + 1
    );
}

/// **The arity budget, reachable through substitution.**
///
/// Substituting a `Sum` into a `Sum` flattens, so the operand list grows
/// without the depth or the DAG changing - the failure mode `MAX_DEPTH` cannot
/// see, and the one whose unbudgeted version asks for a `Vec` that cannot grow
/// and aborts. Growth inside the budget must be permitted and must actually
/// happen; growth past it must be refused with the operator and the count.
#[test]
fn the_arity_budget_is_reachable_through_substitution() {
    let host = Bound::sum([Bound::var("x"), Bound::var("z")]);
    let x = VarId::new("x");

    // Inside the budget: 4096 operands become 4097, and that is the point -
    // a substitution that silently dropped the growth would also "not exceed"
    // the budget.
    let mut small = Bound::var("y");
    for _ in 0..12 {
        small = Bound::sum_checked([small.clone(), small.clone()]).unwrap_or(small);
    }
    assert_eq!(arity_of(&small), Some(4096));
    let grown = Substitution::of(x.clone(), small.clone()).apply(&host);
    assert_eq!(arity_of(&grown), Some(4097));
    assert_eq!(
        Substitution::of(x.clone(), small).apply_checked(&host),
        Ok(grown)
    );

    // Past it: MAX_NODES operands plus one.
    let mut wide = Bound::var("y");
    for _ in 0..20 {
        wide = Bound::sum_checked([wide.clone(), wide.clone()]).unwrap_or(wide);
    }
    let budget = usize::try_from(landav_bound::MAX_NODES).unwrap_or(usize::MAX);
    assert_eq!(arity_of(&wide), Some(budget));

    let overflowing = Substitution::of(x, wide);
    assert_eq!(
        overflowing.apply(&host),
        Bound::omega(),
        "the total form widens where the checked one refuses"
    );
    let refusal = overflowing.apply_checked(&host);
    assert!(
        matches!(
            &refusal,
            Err(BoundError::ArityExceeded { op, got, limit })
                if *op == BoundShape::Sum
                    && *got == u64::from(landav_bound::MAX_NODES) + 1
                    && *limit == landav_bound::MAX_NODES
        ),
        "an arity of MAX_NODES + 1 must be ArityExceeded {{ op: Sum, got: {}, limit: {} }}, \
         not {refusal:?}",
        u64::from(landav_bound::MAX_NODES) + 1,
        landav_bound::MAX_NODES
    );
}

/// **The traversal is worklist driven.** A term at `MAX_DEPTH` must be
/// substitutable without touching the stack depth of the process.
///
/// A recursive `apply` overflows here rather than failing a comparison, and a
/// stack overflow is an abort that `#![forbid(unsafe_code)]`, `unwrap_used`
/// and `panic` cannot see. The result is pinned exactly rather than merely
/// "something came back": `log2` of `4` is `2`, of `2` is `1`, of `1` is `0`,
/// and of `0` is `0` - so a tower of five hundred folds all the way down to
/// `Const(0)`, and a traversal that stopped early would show up as a `Trans`
/// node that never folded.
#[test]
fn a_term_at_the_depth_limit_substitutes_without_recursing() {
    let limit = landav_bound::MAX_DEPTH;
    let mut tower = Bound::var("x");
    for _ in 1..limit {
        tower = Bound::log(Base::TWO, tower);
    }
    assert_eq!(tower.depth(), limit);

    let folded = Substitution::of(VarId::new("x"), Bound::constant(4)).apply(&tower);
    assert_eq!(folded, Bound::zero());
    assert_eq!(folded.kind(), &BoundKind::Const(Nat::ZERO));

    // And composition of two substitutions over the same term.
    let staged = Substitution::of(VarId::new("x"), Bound::var("w"))
        .then(&Substitution::of(VarId::new("w"), Bound::constant(4)));
    assert_eq!(staged.apply(&tower), Bound::zero());
    assert_eq!(staged.apply_checked(&tower), Ok(Bound::zero()));
}

/// [`Substitution::then_checked`] reports what [`Substitution::then`] widens.
///
/// Composition builds each image by substituting into it, so it has the same
/// budgets as `apply` and needs the same blame channel. Without this, a caller
/// composing two substitutions loses the reason its bound became `omega`.
#[test]
fn then_checked_reports_what_then_widens() {
    let limit = landav_bound::MAX_DEPTH;
    let mut tower = Bound::var("x");
    for _ in 1..limit {
        tower = Bound::log(Base::TWO, tower);
    }

    let first = Substitution::of(VarId::new("a"), tower);
    let second = Substitution::of(VarId::new("x"), Bound::log(Base::TWO, Bound::var("y")));

    let widened = first.then(&second);
    assert_eq!(widened.get(&VarId::new("a")), Some(&Bound::omega()));

    let refusal = first.then_checked(&second);
    assert!(
        matches!(&refusal, Err(BoundError::DepthExceeded { limit: named }) if *named == limit),
        "then_checked must report the depth refusal, not {refusal:?}"
    );

    // And where nothing exceeds a budget, the two agree.
    let plain = Substitution::of(VarId::new("a"), Bound::var("b"));
    let benign = Substitution::of(VarId::new("b"), Bound::constant(9));
    assert_eq!(plain.then_checked(&benign), Ok(plain.then(&benign)));
}
