//! **Normalisation must be denotation preserving, and reproducible.**
//!
//! LAN-58 rewrites a bound through an e-graph and extracts a different term.
//! A rewrite rule that changes the meaning is the worst possible bug in this
//! crate - it makes an analyser report a bound that the program does not
//! satisfy - so the headline property here is the same one the smart
//! constructors are held to, stated against the same reference semantics in
//! `crate::support`.
//!
//! # The floor is the recipe's reference denotation, never `eval` of a term
//!
//! `Bound::eval` is not compositional: `Bound::prod` regroups factors and
//! saturating multiplication is not associative, so a term's value carries
//! whatever over-approximation its own grouping accumulated. Four properties
//! in this crate have been written against such a floor and all four were
//! wrong. Every floor below comes from `naive_eval_ideal` of a **recipe**.
//!
//! # Why re-association of `*` is sound but not exact
//!
//! `(2 * u64::MAX) * 0` is `omega` and `2 * (u64::MAX * 0)` is `0`, so the
//! `mul-assoc` rules can move what `eval` reports. They cannot move it below
//! the true denotation: a zero factor makes the ideal product zero unless an
//! `omega` is present, and an `omega` propagates through *every* grouping. So
//! every grouping lies between the ideal value and `omega`, which is exactly
//! the window `precision_violation` allows - and outside the saturating regime
//! the window closes to a point, which is what
//! `normalisation_is_exact_when_nothing_saturates` asserts.
//!
//! # Why these properties do not run at the frozen budget
//!
//! Soundness may not depend on how long the runner was allowed to think, so
//! asserting it at a *small* budget is strictly stronger than asserting it at
//! the frozen one: it covers the partially-normalised terms that
//! `NormaliserStop::IterationLimit` and `NormaliserStop::NodeLimit` produce,
//! which are the ones a loaded machine would have produced anyway. The frozen
//! budget is where the *normal form* is pinned, and that is `tests/normalisation.rs`.

use landav_bound::{Bound, NormaliserBudget, NormaliserStop, normalise_with};
use proptest::prelude::*;

use crate::support::{
    Env, arb_env, arb_small_env, arb_small_spec, arb_spec, build, canonical_violation, ideal_ref,
    naive_eval_ideal, nat_ref, observed_dominates, precision_violation,
};

/// The budget these properties run at. See the module documentation.
const PROPERTY_BUDGET: NormaliserBudget = NormaliserBudget::new(8, 500);

/// Normalises, or turns the failure into a test-case failure rather than a
/// panic - `landav-bound` never panics and neither does its suite.
fn normalised(term: &Bound) -> Result<Bound, TestCaseError> {
    match normalise_with(term, PROPERTY_BUDGET) {
        Ok(form) => {
            if NormaliserStop::ALL.contains(&form.stop()) {
                Ok(form.into_bound())
            } else {
                Err(TestCaseError::fail(format!(
                    "stop reason {} is not count based",
                    form.stop()
                )))
            }
        }
        Err(error) => Err(TestCaseError::fail(format!(
            "normalisation failed on `{term}`: {error}"
        ))),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// **The soundness property. Zero target.**
    ///
    /// The normalised term is judged against the *recipe's* reference
    /// denotation, computed in the ideal domain with no saturation at
    /// intermediate steps. `precision_violation` has no vacuous arm: a finite
    /// observation must equal the true denotation exactly, and an `omega` must
    /// be arithmetically justified by some grouping being able to leave `u64`.
    ///
    /// A failure here is a rewrite rule that changes meaning.
    #[test]
    fn normalisation_never_moves_the_denotation_off_the_reference(
        spec in arb_spec(),
        env in arb_env(),
    ) {
        let term = build(&spec);
        let normal = normalised(&term)?;
        let observed = nat_ref(normal.eval(&env.valuation()));
        if let Some(complaint) = precision_violation(&spec, &env, observed) {
            return Err(TestCaseError::fail(format!(
                "{complaint}\n  recipe    : {spec:?}\n  term      : {term}\n  normalised: {normal}"
            )));
        }
    }

    /// The same statement in its weakest, most direct form: the normalised
    /// term may never sit *below* the true denotation.
    ///
    /// Redundant with the property above only while that one stays exactly as
    /// strong as it is now. Under-approximation is the single class of defect
    /// that invalidates the product, so it is asserted in its own right rather
    /// than as a corollary.
    #[test]
    fn normalisation_never_under_approximates(
        spec in arb_spec(),
        env in arb_env(),
    ) {
        let term = build(&spec);
        let normal = normalised(&term)?;
        let observed = nat_ref(normal.eval(&env.valuation()));
        let exact = naive_eval_ideal(&spec, &env);
        prop_assert!(
            observed_dominates(observed, exact),
            "normalised `{normal}` evaluated to {observed:?}, below the true denotation {exact:?} \
             (from `{term}`)"
        );
    }

    /// Where no grouping of the recipe can leave `u64`, saturation is
    /// unreachable and normalisation must be **exactly** denotation
    /// preserving - not merely sound.
    ///
    /// This is the property that stops the rule set from buying "soundness" by
    /// widening everything to `omega`.
    #[test]
    fn normalisation_is_exact_when_nothing_saturates(
        spec in arb_small_spec(),
        env in arb_small_env(),
    ) {
        let term = build(&spec);
        let normal = normalised(&term)?;
        let observed = nat_ref(normal.eval(&env.valuation()));
        let exact = ideal_ref(naive_eval_ideal(&spec, &env));
        prop_assert_eq!(
            observed,
            exact,
            "in the saturation-free regime normalisation must be exact: `{}` normalised to \
             `{}` and evaluated to {:?} where the denotation is {:?}",
            term,
            normal,
            observed,
            exact
        );
    }

    /// Normalisation must not change the value at the only valuation an
    /// analyser is ever entitled to assume - everything unbounded.
    #[test]
    fn normalisation_is_sound_at_the_all_omega_valuation(spec in arb_spec()) {
        let env = Env::all_omega();
        let term = build(&spec);
        let normal = normalised(&term)?;
        let observed = nat_ref(normal.eval(&env.valuation()));
        prop_assert!(
            observed_dominates(observed, naive_eval_ideal(&spec, &env)),
            "normalised `{}` under-approximates at the all-omega valuation",
            normal
        );
    }

    /// The normal form is a `Bound` like any other, so every structural
    /// invariant the smart constructors maintain must still hold: flatness,
    /// arity, canonical operand order, `Max` distinctness, depth.
    ///
    /// Not implied by the denotational properties. A term that is flat and one
    /// that is not denote the same function; only this catches an extraction
    /// path that rebuilt a `Bound` without going through the constructors.
    #[test]
    fn the_normal_form_satisfies_every_canonical_invariant(spec in arb_spec()) {
        let normal = normalised(&build(&spec))?;
        if let Some(violation) = canonical_violation(&normal) {
            return Err(TestCaseError::fail(format!(
                "the normal form `{normal}` violates a canonical invariant: {violation}"
            )));
        }
    }

    /// **AC3, in the small.** Two runs of the same input in the same process
    /// must agree on both artefacts: the rendered bound and the cache-key
    /// material.
    #[test]
    fn normalisation_is_reproducible(spec in arb_spec()) {
        let term = build(&spec);
        let first = normalised(&term)?;
        let second = normalised(&term)?;
        prop_assert_eq!(
            format!("{}", &first),
            format!("{}", &second),
            "two runs rendered differently"
        );
        prop_assert_eq!(
            first.canonical_bytes().as_bytes().to_vec(),
            second.canonical_bytes().as_bytes().to_vec(),
            "two runs produced different cache-key material"
        );
    }

    /// Normalising a normal form is the identity, so a caller who normalises
    /// twice cannot get a second cache key for one program.
    ///
    /// Stated at the property budget, where a run may stop on a count rather
    /// than on saturation - which is the harder case, and the one a loaded
    /// machine would have produced.
    #[test]
    fn normalisation_is_idempotent(spec in arb_small_spec()) {
        let once = normalised(&build(&spec))?;
        let twice = normalised(&once)?;
        prop_assert_eq!(
            once.canonical_bytes().as_bytes().to_vec(),
            twice.canonical_bytes().as_bytes().to_vec(),
            "normalising `{}` a second time moved it to `{}`",
            &once,
            &twice
        );
    }
}
