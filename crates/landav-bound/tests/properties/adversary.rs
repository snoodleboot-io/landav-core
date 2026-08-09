//! **Adversary lane: attempts to break the LAN-56 guarantees.**
//!
//! Everything here was written to make `landav-bound` fail, not to describe
//! it. Tests fall into three groups, and the group is named in each doc
//! comment:
//!
//! * **BLOCKER** - a reproduction of a defect that aborts or hangs. These are
//!   pinned as *characterisation* tests: they assert the pathological law that
//!   holds today, so that fixing the defect fails them loudly. Each is paired
//!   with an `#[ignore]`d statement of the property that *should* hold, which
//!   is the test to un-ignore once the budget exists.
//! * **TIGHTNESS** - sound but gratuitously loose, or a documented contract
//!   the implementation does not keep. Pinned as characterisation tests too,
//!   with the true mathematical value stated in the assertion message.
//! * **coverage** - attacks that found nothing. They stay because a pass here
//!   is the evidence that the attack was actually run, and because they cover
//!   regimes the existing generators do not reach (edge magnitudes, products
//!   that both saturate *and* contain a zero, and the evaluator judged against
//!   an ideal semantics of its own *term* rather than of the recipe).

use std::collections::BTreeMap;

use landav_bound::{
    Base, Bound, BoundKind, BoundWire, Lifted, Nat, Origin, TotalValuation, VarId, Verdict,
    WireNode,
};
use proptest::prelude::*;

use crate::support::{
    BoundSpec, Env, Ideal, REF_OMEGA, Ref, VAR_NAMES, build, ideal_ceil_log, ideal_join, ideal_of,
    ideal_plus, ideal_pow, ideal_times, naive_eval_ideal, nat_ref, observed_dominates, ref_le,
};

// ---------------------------------------------------------------------------
// BLOCKER 1 - the arity of an n-ary node is unbudgeted
// ---------------------------------------------------------------------------

/// The number of operands directly under an n-ary node.
fn arity_of(bound: &Bound) -> usize {
    match bound.kind() {
        BoundKind::Sum(terms) | BoundKind::Prod(terms) => terms.len(),
        BoundKind::Max(terms) => terms.len(),
        BoundKind::Const(_) | BoundKind::Var(_) | BoundKind::Trans { .. } => 0,
    }
}

/// **BLOCKER 1, fixed in `0b22c60`.** An n-ary node may not carry more
/// operands than [`landav_bound::MAX_NODES`] allows nodes, because a term with
/// more operands than that has no wire form and no observer that terminates.
///
/// # What this replaced
///
/// `Bound::sum` and `Bound::prod` flatten a nested node of the same operator
/// into the parent's operand list, so `b = op([b, b])` *doubled* the operand
/// vector on every call while leaving the depth at 2 and the DAG at 2 distinct
/// nodes. No budget saw it: `MAX_DEPTH` watches a depth that never left 2,
/// `MAX_NODES` was consulted only by `to_wire`/`try_from_wire`, and the
/// `_checked` constructors had no variant for it. Forty lines of public API
/// asked for a `Vec` of `2^40` handles - 8 TB - and a `Vec` that cannot grow
/// aborts through `handle_alloc_error`, which `#![forbid(unsafe_code)]`,
/// `unwrap_used` and `panic` cannot see.
///
/// A companion test pinned that doubling law as characterisation. It was
/// **deleted** rather than ignored when the budget landed: its central
/// assertion was that `sum_checked` returns `Ok` at arity `2^21`, which is now
/// the exact negation of the contract this test states, and an ignored test
/// asserting the negation of a live contract is an invitation to "fix" the
/// budget away.
#[test]
fn nary_arity_must_be_budgeted() {
    let limit = usize::try_from(landav_bound::MAX_NODES).unwrap_or(usize::MAX);
    let mut summed = Bound::var("x");
    let mut refused = false;
    for _ in 0..40 {
        match Bound::sum_checked([summed.clone(), summed.clone()]) {
            Ok(next) => summed = next,
            Err(_) => {
                refused = true;
                break;
            }
        }
        assert!(
            arity_of(&summed) <= limit,
            "arity {} exceeds the node budget with no error reported",
            arity_of(&summed)
        );
    }
    assert!(refused, "40 doublings produced no budget refusal");
}

// ---------------------------------------------------------------------------
// BLOCKER 2 - every observer walks the tree, not the DAG
// ---------------------------------------------------------------------------

/// `b_{i+1} = (b_i * b_i) + 1`.
///
/// Two new DAG nodes per level, depth `+2` per level, and a *tree* that
/// doubles. The `Prod` child is a `Sum` and the `Sum` child is a `Prod`, so
/// the flattening rule never fires and the sharing survives - which is exactly
/// the shape [`landav_bound::WireNode`]'s own doc comment names as the reason
/// the wire form is a DAG.
fn shared_ladder(levels: u32) -> Bound {
    let mut bound = Bound::var("x");
    for _ in 0..levels {
        let squared = Bound::prod([bound.clone(), bound.clone()]);
        bound = Bound::sum([squared, Bound::constant(1)]);
    }
    bound
}

/// **BLOCKER 2, fixed in `0b22c60`.** The canonical encoding of a term must be
/// bounded by its DAG size, not by its tree size: a 42-node DAG must not
/// produce 40 MB of bytes.
///
/// # What this replaced
///
/// The wire form preserved sharing; nothing else did. `eval`,
/// `canonical_bytes`, `to_wire`, `wire_node_count`, `Hash`, `PartialEq`,
/// `canonical_cmp` and `Display` all walked the *tree* with no memoisation
/// over the shared `Arc`s, so on [`shared_ladder`] each cost `2^levels`:
///
/// ```text
/// level  depth  DAG nodes  canonical_bytes  wire_node_count   after 0b22c60
///    12     25         26        155 624 B            50 ms
///    16     33         34      2 490 344 B         1 055 ms
///    20     41         42     39 845 864 B        17 460 ms   1 040 B / 0.01 ms
/// ```
///
/// `MAX_DEPTH` is 512, so 255 levels were constructible in budget and every
/// observer needed `2^255` steps; `canonical_bytes` allocates its output, so
/// it OOM-aborted around level 30. `wire_node_count` was the sharpest
/// instance - offered by its own doc as the cheap pre-check "so a caller can
/// check the serialised size *before* serialising", it returned 34 after doing
/// `2^16` work.
///
/// The companion test pinned that doubling law. It was **deleted** rather than
/// ignored: it required `canonical_bytes` to keep doubling to ~2.5 MB at level
/// 16, this one requires ~1 KB at level 20, and the encoding is monotone in
/// term size - so the two cannot both hold, and keeping the losing one around
/// as documentation of a fixed bug is not worth a contradictory assertion in
/// the tree.
#[test]
fn observers_must_be_polynomial_in_the_dag() {
    let bound = shared_ladder(20);
    let nodes = usize::try_from(bound.wire_node_count()).unwrap_or(usize::MAX);
    let bytes = bound.canonical_bytes().as_bytes().len();
    assert!(
        bytes <= nodes * 4096,
        "a {nodes}-node DAG encoded to {bytes} bytes"
    );
}

/// **BLOCKER 2, delivered by untrusted input - fixed in `0b22c60`, kept live.**
///
/// [`Bound::try_from_wire`] is the documented ingest for "a hand-edited or
/// platform-supplied document". This builds one by hand: every child index
/// strictly below its parent, every operator in-vocabulary, node count far
/// inside [`landav_bound::MAX_NODES`] and root depth inside
/// [`landav_bound::MAX_DEPTH`]. `try_from_wire`'s three guards - version, node
/// budget, depth - are all satisfied, because none of them measures the tree.
///
/// Before the fix the term that came back cost `2^levels` steps to evaluate,
/// print, hash or serialise, so a document under 15 KB of JSON produced a term
/// that was unobservable forever. This test was originally a
/// *characterisation* that asserted the blow-up was still present, and stopped
/// at 12 levels so the suite would finish.
///
/// It is kept - rather than deleted with the other two - because the ingest
/// path is attack surface that no other test covers: [`shared_ladder`] reaches
/// the same shape through the public constructors, which a hostile party does
/// not have to use. So it is inverted into the regression it should always
/// have been.
///
/// The contract asserted is **not** "the document is accepted". Refusing it
/// with `TreeSizeExceeded` is an equally sound answer, and pinning acceptance
/// would pin one of two valid choices - the mistake this suite has now made
/// four times. What must hold is narrower and is the actual security
/// property: *ingest may never accept a term no observer can finish.* A small
/// document must still round-trip, and a large one must be either refused or
/// cheap to observe - never accepted and unobservable.
#[test]
fn a_hand_built_wire_document_cannot_smuggle_in_an_unobservable_term() {
    // 12 levels is a legitimate small document and must still round-trip; 200
    // is depth 401 against a limit of 512 and is the size the old doc called
    // "unobservable forever".
    for (levels, must_be_accepted) in [(12u32, true), (200u32, false)] {
        let mut nodes = vec![
            WireNode::Var {
                name: "x".to_owned(),
            },
            WireNode::Const { fin: Some(1) },
        ];
        let mut previous: u32 = 0;
        for _ in 0..levels {
            let at = u32::try_from(nodes.len()).unwrap_or(u32::MAX);
            nodes.push(WireNode::Prod {
                args: vec![previous, previous],
            });
            nodes.push(WireNode::Sum { args: vec![at, 1] });
            previous = at + 1;
        }
        let wire = BoundWire {
            version: landav_bound::WIRE_VERSION,
            nodes,
            root: previous,
        };
        let declared = wire.nodes.len();
        assert!(u32::try_from(declared).unwrap_or(u32::MAX) < landav_bound::MAX_NODES);

        match Bound::try_from_wire(&wire) {
            Ok(term) => {
                assert!(term.depth() <= landav_bound::MAX_DEPTH);
                // Accepted, so every observer must be polynomial in the DAG
                // the document declared, not exponential in the tree it
                // unfolds to. Reaching this line at all is half the assertion:
                // before `0b22c60` `canonical_bytes` allocated its output and
                // OOM-aborted here.
                let dag = usize::try_from(term.wire_node_count()).unwrap_or(usize::MAX);
                let bytes = term.canonical_bytes().as_bytes().len();
                assert!(
                    bytes <= dag * 4096,
                    "a {declared}-node document at {levels} levels rebuilt to a term \
                     encoding to {bytes} bytes over {dag} DAG nodes"
                );
            }
            Err(refusal) => {
                assert!(
                    !must_be_accepted,
                    "a {declared}-node document at {levels} levels was refused: {refusal:?}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TIGHTNESS - the saturating-with-a-zero regime
// ---------------------------------------------------------------------------
//
// THESE THREE WERE CHARACTERISATION TESTS. THE FIX LANDED; THEY ARE INVERTED.
//
// They used to be, and are still findable under, these names:
//
//   a_closed_product_denoting_zero_becomes_an_unblamed_omega
//     -> a_closed_product_denoting_zero_is_proved_zero
//   prod_keeps_a_redundant_literal_beside_the_zero_and_pays_for_it
//     -> prod_keeps_only_the_zero_beside_a_symbolic_operand
//   closed_terms_denoting_zero_do_not_all_fold_to_const_zero
//     -> a_saturating_subgroup_still_defeats_an_enclosing_zero
//
// Each pinned the *unfixed* behaviour and said so in its assertion messages
// ("the tightness gap is closed", "the deviation has been fixed"). Each now
// asserts the corrected behaviour, and each doc comment names what it used to
// pin and why it changed. The third is only *narrowed*, not closed: see it.
//
// Nothing in `denotation.rs` blocks the fix any more. It used to: an
// overflow-dominant floor under `Bound::prod`, a flattening-direction claim
// and two substitution floors all required `omega` in exactly the cases the
// fix makes exact. All four now take their floor from `naive_eval_ideal` of a
// recipe, which no sound implementation can violate. See `precision_violation`
// in `support.rs` for why that is the only admissible kind of floor.

/// A valuation binding `x` and defaulting everything else to `default`.
fn at_x(value: Nat, default: Nat) -> TotalValuation {
    let mut known = BTreeMap::new();
    known.insert(VarId::new("x"), value);
    TotalValuation::with_default(known, default)
}

/// **TIGHTNESS 1, fixed. Inverted from
/// `a_closed_product_denoting_zero_becomes_an_unblamed_omega`.**
///
/// It used to pin the defect: `Bound::prod([0, 2^40, 2^40])` has no symbolic
/// operand and no `omega` anywhere and denotes `0` under any reading, but
/// `assemble` multiplied the *non-zero* literals first, overflowed, and
/// returned `Bound::omega()` before it ever looked at `has_zero`. That was
/// not merely loose: [`Verdict::classify`] refuses an unblamed `omega`, so a
/// program proved to cost nothing produced `Err(BoundError::UnblamedOmega)`,
/// a tool error with a non-clean exit.
///
/// `assemble` now consults the zero first. The overflow-first ordering is
/// kept for the *symbolic* case, where a `Var` may be `omega` and a zero
/// therefore may not pre-empt it - which is why
/// `omega_totality::prod_does_not_fold_zero_times_a_symbolic_operand` still
/// holds.
#[test]
fn a_closed_product_denoting_zero_is_proved_zero() {
    let big = 1u64 << 40;
    let closed = Bound::prod([Bound::zero(), Bound::constant(big), Bound::constant(big)]);

    assert_eq!(
        closed,
        Bound::zero(),
        "prod([0, 2^40, 2^40]) is closed and denotes 0"
    );
    assert!(closed.is_finite());
    assert_eq!(closed.eval(&at_x(Nat::ZERO, Nat::ZERO)), Nat::ZERO);

    // And the consequence: a proved-zero cost is publishable again.
    let verdict = Verdict::classify(Lifted::Elem(closed.clone()), Origin::new("f"), None);
    assert!(
        matches!(&verdict, Ok(Verdict::Proved(finite)) if finite.get() == &Bound::zero()),
        "a cost proved to be nothing must classify as Proved(0), got {verdict:?}"
    );

    // The same multiset, grouped so the zero is folded first, agrees - the
    // grouping no longer decides once every factor is a literal.
    let regrouped = Bound::prod([
        Bound::prod([Bound::zero(), Bound::constant(big)]),
        Bound::constant(big),
    ]);
    assert_eq!(regrouped, Bound::zero());
    assert_eq!(regrouped, closed);
}

/// **TIGHTNESS 2, fixed. Inverted from
/// `prod_keeps_a_redundant_literal_beside_the_zero_and_pays_for_it`.**
///
/// It used to pin a deviation from `Bound::prod`'s own doc comment: step 3
/// says the finite literals are constant-folded and step 5 says a folded `0`
/// collapses only when there are no other operands, but the implementation
/// pushed the zero **and** the folded product of the other literals as *two
/// separate operands*. `Prod[0, k, x]` was then `omega` whenever `k * x` left
/// `u64`, where `Prod[0, x]` - the same denotation - was exact.
///
/// The extra literal was pure looseness, and dropping it cannot cost
/// soundness: it only removes a factor from the overflow test, and the zero
/// already decides every case that test would have caught. `omega` still
/// absorbs at `x = omega`, which the last two assertions pin.
#[test]
fn prod_keeps_only_the_zero_beside_a_symbolic_operand() {
    let folded = Bound::prod([Bound::zero(), Bound::constant(3), Bound::var("x")]);
    let direct = Bound::prod([Bound::zero(), Bound::var("x")]);

    assert_eq!(
        arity_of(&folded),
        2,
        "only the zero survives beside the symbolic operand"
    );
    assert_eq!(arity_of(&direct), 2);
    assert_eq!(folded, direct, "the redundant literal is gone");

    let at = at_x(Nat::Fin(u64::MAX), Nat::OMEGA);
    assert_eq!(folded.eval(&at), Nat::ZERO, "0 * 3 * u64::MAX is 0");
    assert_eq!(direct.eval(&at), Nat::ZERO, "0 * u64::MAX is 0");

    // And `omega` still absorbs unconditionally at the top of the lattice,
    // so the zero has not been allowed to pre-empt an unbounded operand.
    let top = at_x(Nat::OMEGA, Nat::OMEGA);
    assert_eq!(folded.eval(&top), Nat::OMEGA);
    assert_eq!(direct.eval(&top), Nat::OMEGA);
}

/// **TIGHTNESS 3: narrowed by the T1 fix, and still open. Inverted from
/// `closed_terms_denoting_zero_do_not_all_fold_to_const_zero`.**
///
/// [`landav_bound::B::star`]'s doc comment justifies its syntactic zero test
/// like this: *"it is well defined only because the smart constructors
/// constant-fold: any closed term denoting zero folds to `Const(0)`, so `star`
/// is a function of the denotation rather than of the spelling."*
///
/// The old witness for that premise being false was
/// `prod([0, 2^40, 2^40])` folding to `Const(omega)` while
/// `prod([0, 3])` folded to `Const(0)`. T1 closed that one: a product whose
/// factors are **all literals** now folds to `Const(0)` whenever one of them
/// is zero, however the recipe grouped them.
///
/// What T1 did not close, and could not: a subgroup that saturates *on its
/// own* becomes `Const(omega)` as a term, and `omega` then absorbs
/// unconditionally - the frozen `Nat::times` rule - so an enclosing zero can
/// no longer rescue it. `prod([prod([2^40, 2^40]), 0])` and
/// `prod([0, 2^40, 2^40])` are both closed and both denote `0`, and they
/// still fold to different constants.
///
/// So the premise remains false and `B::star` would still return `one` for
/// one spelling and `Elem(omega)` for the other. `b.rs`'s comment is left
/// exactly as it is on purpose: LAN-59 owns `B::star`, and the finding is
/// carried to wave 2 rather than papered over here.
#[test]
fn a_saturating_subgroup_still_defeats_an_enclosing_zero() {
    let big = 1u64 << 40;

    // Closed by T1: every grouping whose factors are all literals is exact.
    let flat = Bound::prod([Bound::zero(), Bound::constant(big), Bound::constant(big)]);
    let zero_first = Bound::prod([
        Bound::prod([Bound::zero(), Bound::constant(big)]),
        Bound::constant(big),
    ]);
    assert_eq!(flat.kind(), &BoundKind::Const(Nat::ZERO));
    assert_eq!(zero_first.kind(), &BoundKind::Const(Nat::ZERO));
    assert_eq!(
        Bound::prod([Bound::zero(), Bound::constant(3)]).kind(),
        &BoundKind::Const(Nat::ZERO)
    );

    // Still open: the subgroup saturates before the zero is ever in scope,
    // and `omega` absorbs unconditionally from there on.
    let saturating_subgroup = Bound::prod([Bound::constant(big), Bound::constant(big)]);
    assert_eq!(
        saturating_subgroup.kind(),
        &BoundKind::Const(Nat::OMEGA),
        "2^40 * 2^40 leaves u64, so the subgroup is omega as a term"
    );
    let poisoned = Bound::prod([Bound::zero(), saturating_subgroup]);
    assert_eq!(
        poisoned.kind(),
        &BoundKind::Const(Nat::OMEGA),
        "star's well-definedness premise now holds for every closed term"
    );

    // Two closed spellings of one denotation, still folding to two constants.
    assert_ne!(flat, poisoned);
}

// ---------------------------------------------------------------------------
// coverage - attacks that found nothing
// ---------------------------------------------------------------------------

/// Magnitudes chosen to make an n-ary product straddle the `u64` edge while a
/// zero is present - the regime the suite's author named as its soft spot.
fn arb_edge_ref() -> impl Strategy<Value = Ref> {
    prop_oneof![
        5 => Just(Some(0u64)),
        2 => Just(Some(1u64)),
        2 => Just(Some(2u64)),
        4 => prop_oneof![
            Just(Some(1u64 << 21)),
            Just(Some(1u64 << 32)),
            Just(Some(1u64 << 40)),
            Just(Some(1u64 << 63)),
            // just above and just below the square root of u64::MAX
            Just(Some(4_294_967_295u64)),
            Just(Some(4_294_967_296u64)),
            Just(Some(u64::MAX)),
            Just(Some(u64::MAX - 1)),
        ],
        2 => Just(REF_OMEGA),
    ]
}

/// Product-heavy terms over [`arb_edge_ref`]: recipes that saturate *and*
/// carry a zero.
fn arb_saturating_zero_spec() -> impl Strategy<Value = BoundSpec> {
    let leaf = prop_oneof![
        5 => arb_edge_ref().prop_map(BoundSpec::Const),
        3 => (0usize..VAR_NAMES.len()).prop_map(BoundSpec::Var),
    ];
    leaf.prop_recursive(4, 48, 4, |inner| {
        prop_oneof![
            6 => proptest::collection::vec(inner.clone(), 2..5).prop_map(BoundSpec::Prod),
            2 => proptest::collection::vec(inner.clone(), 2..4).prop_map(BoundSpec::Sum),
            2 => proptest::collection::vec(inner.clone(), 2..4).prop_map(BoundSpec::Max),
            1 => (any::<bool>(), 2u32..=8, inner).prop_map(|(log, base, arg)| BoundSpec::Trans {
                log,
                base,
                arg: Box::new(arg),
            }),
        ]
    })
}

/// Valuations over the same edge magnitudes.
fn arb_edge_env() -> impl Strategy<Value = Env> {
    (proptest::array::uniform3(arb_edge_ref()), arb_edge_ref())
        .prop_map(|(vals, default)| Env { vals, default })
}

/// The index of a generated variable, if `var` is one.
fn generated_index(var: &VarId) -> Option<usize> {
    VAR_NAMES
        .iter()
        .position(|name| *name == var.symbol().as_str())
}

/// The ideal denotation of a **built term**, walking the `Bound` itself.
///
/// The suite's `naive_eval_ideal` interprets the *recipe*, so it judges the
/// smart constructors. This judges [`Bound::eval`] against the ideal reading
/// of the very node structure it is folding - a different obligation, and the
/// one that would catch a mis-folded `Prod` or `Trans` arm in the evaluator
/// even if every constructor were perfect.
fn ideal_eval_term(bound: &Bound, env: &Env) -> Ideal {
    match bound.kind() {
        BoundKind::Const(magnitude) => ideal_of(nat_ref(*magnitude)),
        BoundKind::Var(var) => match generated_index(var) {
            Some(index) => ideal_of(env.value_of(index)),
            None => ideal_of(env.default),
        },
        BoundKind::Sum(terms) => terms
            .as_slice()
            .iter()
            .map(|operand| ideal_eval_term(operand, env))
            .fold(Ideal::Fin(0), ideal_plus),
        BoundKind::Max(terms) => terms
            .as_slice()
            .iter()
            .map(|operand| ideal_eval_term(operand, env))
            .fold(Ideal::Fin(0), ideal_join),
        BoundKind::Prod(terms) => terms
            .as_slice()
            .iter()
            .map(|operand| ideal_eval_term(operand, env))
            .fold(Ideal::Fin(1), ideal_times),
        BoundKind::Trans { kind, base, arg } => {
            let inner = ideal_eval_term(arg, env);
            match kind {
                landav_bound::TransKind::Pow => ideal_pow(base.get(), inner),
                landav_bound::TransKind::Log => ideal_ceil_log(base.get(), inner),
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4096,
        ..ProptestConfig::default()
    })]

    /// **Attack 1, in the regime the author flagged.** Products that saturate
    /// *and* contain a zero, at valuations drawn from the same edge
    /// magnitudes, four thousand cases deep. Found nothing.
    #[test]
    fn saturating_products_with_zeros_never_under_approximate(
        spec in arb_saturating_zero_spec(),
        env in arb_edge_env(),
    ) {
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

    /// **Attack 1, aimed at the evaluator rather than the constructors.**
    /// `Bound::eval` must dominate the ideal reading of the term it is
    /// actually folding, node for node. Found nothing.
    #[test]
    fn eval_dominates_the_ideal_semantics_of_its_own_term(
        spec in arb_saturating_zero_spec(),
        env in arb_edge_env(),
    ) {
        let bound = build(&spec);
        let observed = nat_ref(bound.eval(&env.valuation()));
        let exact = ideal_eval_term(&bound, &env);
        prop_assert!(
            observed_dominates(observed, exact),
            "eval({bound}) = {observed:?} is below the term's own denotation {exact:?} at {env:?}"
        );
    }

    /// **Attack 3, the anti-vacuity guard.** `arb_small_spec` is claimed to be
    /// saturation-free by arithmetic (magnitudes <= 4, depth <= 3, width <= 3,
    /// so `4^27 = 2^54`). If any generated term could reach `Ideal::Beyond`,
    /// `denotation_is_exact_when_nothing_saturates` would be comparing an
    /// exact `omega` against a finite implementation answer and the "== is
    /// attainable" argument would be gone.
    ///
    /// Checked directly rather than trusted: no term from that generator, at
    /// any valuation from `arb_small_env`, leaves `u64`. Found nothing.
    #[test]
    fn the_small_generator_really_cannot_saturate(
        spec in crate::support::arb_small_spec(),
        env in crate::support::arb_small_env(),
    ) {
        prop_assert!(
            naive_eval_ideal(&spec, &env) != Ideal::Beyond,
            "arb_small_spec produced a term that leaves u64: {spec:?} at {env:?}"
        );
    }

    /// **Attack 7.** A substitution that makes the value *fall* under a
    /// pointwise-increased valuation would break composition. Checked over the
    /// edge generators, where saturation regroups aggressively. Found nothing.
    #[test]
    fn substitution_stays_monotone_over_edge_magnitudes(
        spec in arb_saturating_zero_spec(),
        replacement in arb_saturating_zero_spec(),
        (lo, hi) in crate::support::arb_env_pair(),
    ) {
        let substituted = build(&spec).subst(&VarId::new(VAR_NAMES[0]), &build(&replacement));
        prop_assert!(
            ref_le(
                nat_ref(substituted.eval(&lo.valuation())),
                nat_ref(substituted.eval(&hi.valuation())),
            ),
            "{substituted} fell from {lo:?} to {hi:?}"
        );
    }
}

/// The least `i` with `k^i >= max(1, value)`, in `u128` so it cannot saturate.
fn ceil_log_u128(k: u64, value: u64) -> u64 {
    let target = u128::from(value.max(1));
    let mut reached: u128 = 1;
    let mut exponent: u64 = 0;
    while reached < target {
        exponent += 1;
        reached *= u128::from(k);
    }
    exponent
}

/// **Attack 6.** `ceil_log` at `0`, `1`, `k-1`, `k`, `k+1`, powers of `k`,
/// `u64::MAX` and `omega`, for bases from 2 to `u32::MAX`, against a `u128`
/// reference that cannot saturate. Found nothing.
#[test]
fn ceil_log_matches_a_u128_reference_at_every_edge() {
    let bases: [u32; 13] = [
        2,
        3,
        4,
        5,
        7,
        10,
        16,
        255,
        256,
        65_535,
        65_536,
        1 << 31,
        u32::MAX,
    ];
    for k in bases {
        let Ok(base) = Base::new(k) else {
            unreachable!("base {k} is at least 2");
        };
        let wide = u64::from(k);
        let mut arguments: Vec<u64> = vec![0, 1, 2, 3, u64::MAX, u64::MAX - 1, 1 << 63];
        for exponent in 0..5u32 {
            if let Some(power) = wide.checked_pow(exponent) {
                arguments.push(power.saturating_sub(1));
                arguments.push(power);
                arguments.push(power.saturating_add(1));
            }
        }
        for value in arguments {
            assert_eq!(
                Nat::Fin(value).ceil_log(base),
                Nat::Fin(ceil_log_u128(wide, value)),
                "ceil_log_{k}({value})"
            );
        }
        assert_eq!(Nat::OMEGA.ceil_log(base), Nat::OMEGA);
    }
}

/// **Attack 6.** `exp_of` around the exponent cap and above `u32::MAX`, where
/// narrowing before the cap test is the historical defect
/// (`4294967296u64 as u32 == 0` made `pow(2, 2^32)` report `1`). Found
/// nothing: every exponent that does not fit `u64` reports `omega`, and no
/// exponent wraps.
#[test]
fn exp_of_never_wraps_or_truncates_the_exponent() {
    for k in [2u32, 3, 10, 65_535, 1 << 31, u32::MAX] {
        let Ok(base) = Base::new(k) else {
            unreachable!("base {k} is at least 2");
        };
        for exponent in [
            0u64,
            1,
            2,
            62,
            63,
            64,
            65,
            u64::from(u32::MAX),
            u64::from(u32::MAX) + 1,
            1 << 32,
            u64::MAX,
        ] {
            let expected = u32::try_from(exponent)
                .ok()
                .and_then(|narrowed| u64::from(k).checked_pow(narrowed))
                .map_or(Nat::OMEGA, Nat::Fin);
            assert_eq!(Nat::Fin(exponent).exp_of(base), expected, "{k}^{exponent}");
        }
        assert_eq!(Nat::OMEGA.exp_of(base), Nat::OMEGA);
    }
}

/// **Attack 2, the `Ideal::Beyond` reference.** `ideal_ceil_log` collapses
/// `Beyond` to a single finite number - the least `i` with `base^i >= 2^64` -
/// which is a *lower* bound on the truth, not the truth.
///
/// `log_2(2^100)` is exactly `100`; the reference reports `64`. The direction
/// is safe (`observed_dominates` only ever gets weaker, never noisier) but it
/// means `smart_constructors_never_under_approximate` cannot see an
/// under-approximation of a `ceil_log` applied to a `Beyond` argument. The
/// only thing standing between that and a missed unsoundness is the
/// hand-written `log_edges::ceil_log_of_omega_is_omega`, not the property.
///
/// Recorded so the gap is a known, named one. The implementation itself is
/// sound here: it reports `omega`.
#[test]
fn the_beyond_reference_is_a_lower_bound_not_the_denotation() {
    let spec = BoundSpec::Trans {
        log: true,
        base: 2,
        arg: Box::new(BoundSpec::Trans {
            log: false,
            base: 2,
            arg: Box::new(BoundSpec::Var(0)),
        }),
    };
    let env = Env {
        vals: [Some(100), Some(0), Some(0)],
        default: REF_OMEGA,
    };

    // The truth is 100. The reference says 64.
    assert_eq!(
        naive_eval_ideal(&spec, &env),
        Ideal::Fin(64),
        "the reference now tracks Beyond through ceil_log"
    );

    // The implementation over-approximates rather than under-approximates, so
    // nothing is unsound today.
    let bound = build(&spec);
    assert_eq!(bound.eval(&env.valuation()), Nat::OMEGA);
    assert!(observed_dominates(
        nat_ref(bound.eval(&env.valuation())),
        naive_eval_ideal(&spec, &env)
    ));
}
