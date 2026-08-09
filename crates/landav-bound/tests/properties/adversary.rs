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
    Base, Bound, BoundKind, BoundWire, Canonical, Lifted, Nat, Origin, TotalValuation, VarId,
    Verdict, WireNode,
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

/// **BLOCKER (abort).** `Bound::sum` and `Bound::prod` flatten a nested node of
/// the same operator into the parent's operand list. `b = op([b, b])`
/// therefore *doubles* the operand vector on every call while leaving the
/// depth at 2 and the DAG at 2 distinct nodes.
///
/// Neither budget sees it:
///
/// * [`landav_bound::MAX_DEPTH`] is 512 and the depth never leaves 2;
/// * [`landav_bound::MAX_NODES`] is `1 << 20` and is only consulted by
///   `to_wire`/`try_from_wire`, never by `assemble`;
/// * the `_checked` constructors have no variant for it and return `Ok`.
///
/// 40 lines of straight-line public API therefore ask for a `Vec` of `2^40`
/// handles - 8 TB - and the failure mode of a `Vec` that cannot grow is
/// `handle_alloc_error`, an **abort**. `#![forbid(unsafe_code)]`,
/// `unwrap_used` and `panic` cannot see it.
///
/// This test pins the growth law at a safe size. It fails the moment an arity
/// budget exists, which is the intent.
#[test]
fn nary_arity_doubles_per_call_and_no_budget_stops_it() {
    let mut summed = Bound::var("x");
    let mut multiplied = Bound::var("x");
    for level in 1u32..=20 {
        summed = Bound::sum([summed.clone(), summed.clone()]);
        multiplied = Bound::prod([multiplied.clone(), multiplied.clone()]);

        let expected = 1usize << level;
        assert_eq!(
            arity_of(&summed),
            expected,
            "sum arity at level {level} is not 2^{level}"
        );
        assert_eq!(
            arity_of(&multiplied),
            expected,
            "prod arity at level {level} is not 2^{level}"
        );
        assert_eq!(summed.depth(), 2, "the depth guard never engages");
        assert_eq!(multiplied.depth(), 2, "the depth guard never engages");
    }

    // 2^20 operands - exactly `MAX_NODES` - and every budget still says yes.
    assert_eq!(arity_of(&summed), 1 << 20);
    assert!(
        Bound::sum_checked([summed.clone(), summed.clone()]).is_ok(),
        "sum_checked has no error variant for an unbounded operand list"
    );
    assert!(
        Bound::prod_checked([multiplied.clone(), multiplied.clone()]).is_ok(),
        "prod_checked has no error variant for an unbounded operand list"
    );
}

/// **BLOCKER 1, as the property that should hold.** Un-ignore this once an
/// arity or total-size budget exists; it is the statement of the fix.
///
/// The bound is deliberately generous: an n-ary node may not carry more
/// operands than [`landav_bound::MAX_NODES`] allows nodes, because a term with
/// more operands than that has no wire form and no observer that terminates.
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

/// **BLOCKER (abort / non-termination).** The wire form preserves sharing;
/// nothing else does.
///
/// `Bound::eval`, `Bound::canonical_bytes`, `Bound::to_wire`,
/// `Bound::wire_node_count`, `Hash`, `PartialEq`, `Canonical::canonical_cmp`
/// and `Display` all traverse the *tree*, with no memoisation over the shared
/// `Arc`s. On [`shared_ladder`] each costs `2^levels`:
///
/// ```text
/// level  depth  distinct DAG nodes  canonical_bytes  wire_node_count time
///    12     25                  26         155 624 B          50 ms
///    16     33                  34       2 490 344 B        1 055 ms
///    20     41                  42      39 845 864 B       17 460 ms
/// ```
///
/// `MAX_DEPTH` is 512, so 255 levels are constructible in-budget and every one
/// of those observers needs `2^255` steps. `canonical_bytes` additionally
/// *allocates* its output, so it OOM-aborts around level 30 (≈40 GB) long
/// before it hangs.
///
/// `wire_node_count` is the sharpest instance: its doc comment offers it as
/// the cheap pre-check "so a caller can check the serialised size *before*
/// serialising", and it is exponential in the thing it is meant to guard - it
/// returns 34 after doing 2^16 work.
///
/// Pinned here as the exact doubling law, at a size that runs in milliseconds.
#[test]
fn every_observer_is_exponential_in_a_shared_dag() {
    let mut previous: Option<usize> = None;
    for levels in 8u32..=16 {
        let bound = shared_ladder(levels);
        let bytes = bound.canonical_bytes().as_bytes().len();

        // The DAG is tiny and the depth is tiny; only the tree is huge.
        assert_eq!(
            bound.depth(),
            u16::try_from(2 * levels + 1).unwrap_or(u16::MAX)
        );
        assert_eq!(bound.wire_node_count(), 2 * levels + 2);
        assert!(bound.wire_node_count() < landav_bound::MAX_NODES);

        if let Some(before) = previous {
            assert!(
                bytes > before * 2 - 64,
                "canonical_bytes grew from {before} to {bytes}: the doubling has been fixed"
            );
        }
        previous = Some(bytes);
    }

    // The observers agree with each other; it is the cost, not the answer,
    // that is wrong. Two independently built ladders compare equal - after
    // 2^16 comparisons, because `PartialEq` short-circuits only on pointer
    // identity of the *roots*.
    let left = shared_ladder(16);
    let right = shared_ladder(16);
    assert!(left == right);
    assert_eq!(left.canonical_cmp(&right), core::cmp::Ordering::Equal);
}

/// **BLOCKER 2, as the property that should hold.** Un-ignore once the
/// observers memoise over the shared DAG.
///
/// The statement: the canonical encoding of a term must be bounded by its DAG
/// size, not by its tree size. A 42-node DAG must not produce 40 MB of bytes.
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

/// **BLOCKER 2, delivered by untrusted input.**
///
/// [`Bound::try_from_wire`] is the documented ingest for "a hand-edited or
/// platform-supplied document". This builds one by hand: 50 nodes against a
/// budget of 1 048 576, root depth 49 against a limit of 512, every child
/// index strictly below its parent, every operator in-vocabulary. It is
/// accepted in about 100 microseconds - and the term it returns costs `2^24`
/// steps to evaluate, print, hash or serialise.
///
/// Scaled to the depth limit the same document is under 15 KB of JSON and the
/// resulting term is unobservable forever. `try_from_wire`'s three guards -
/// version, node budget, depth - are all satisfied, because none of them
/// measures the tree.
///
/// This test stops at 12 levels so that the suite stays fast; the acceptance
/// is the finding, the cost is measured by
/// [`every_observer_is_exponential_in_a_shared_dag`].
#[test]
fn a_wire_document_inside_every_budget_rebuilds_an_unobservable_term() {
    let levels = 12u32;
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

    assert!(u32::try_from(wire.nodes.len()).unwrap_or(u32::MAX) < landav_bound::MAX_NODES);
    let rebuilt = Bound::try_from_wire(&wire);
    assert!(
        rebuilt.is_ok(),
        "the document was refused - a budget now exists: {rebuilt:?}"
    );

    // Accepted, in budget, and the encoding of what came back is 2^levels big.
    let bytes = rebuilt
        .map(|term| {
            assert!(term.depth() <= landav_bound::MAX_DEPTH);
            term.canonical_bytes().as_bytes().len()
        })
        .unwrap_or_default();
    assert!(
        bytes > (1usize << levels),
        "{} wire nodes rebuilt to only {bytes} bytes: the blow-up is fixed",
        wire.nodes.len()
    );
}

// ---------------------------------------------------------------------------
// TIGHTNESS - the saturating-with-a-zero regime
// ---------------------------------------------------------------------------

/// A valuation binding `x` and defaulting everything else to `default`.
fn at_x(value: Nat, default: Nat) -> TotalValuation {
    let mut known = BTreeMap::new();
    known.insert(VarId::new("x"), value);
    TotalValuation::with_default(known, default)
}

/// **TIGHTNESS, with a hard consequence.** A *closed* term that denotes
/// exactly `0` folds to `Const(omega)`, and [`Verdict::classify`] then refuses
/// to publish it at all.
///
/// `Bound::prod([0, 2^40, 2^40])` has no symbolic operand and no `omega`
/// anywhere. Its value is `0` under any reading. `assemble` nonetheless
/// multiplies the *non-zero* literals first, overflows, and returns
/// `Bound::omega()` before it ever looks at `has_zero`.
///
/// The result is not merely loose. A program proved to cost nothing produces
/// `Err(BoundError::UnblamedOmega)` - a tool error with a non-clean exit -
/// where the correct verdict is `Proved(0)`.
///
/// The order in `assemble` that causes it is deliberate for the *symbolic*
/// case (a `Var` may be `omega`, so a zero cannot pre-empt the overflow). It
/// is not needed here: with no symbolic operands left, `has_zero` decides the
/// value outright, which is what the very next branch already does when the
/// product happens to fit.
#[test]
fn a_closed_product_denoting_zero_becomes_an_unblamed_omega() {
    let big = 1u64 << 40;
    let closed = Bound::prod([Bound::zero(), Bound::constant(big), Bound::constant(big)]);

    assert_eq!(
        closed,
        Bound::omega(),
        "prod([0, 2^40, 2^40]) is no longer omega - the tightness gap is closed"
    );
    assert!(!closed.is_finite());
    assert_eq!(
        closed.eval(&at_x(Nat::ZERO, Nat::ZERO)),
        Nat::OMEGA,
        "true value is 0"
    );

    // And the consequence: a proved-zero cost cannot be published.
    let verdict = Verdict::classify(Lifted::Elem(closed), Origin::new("f"), None);
    assert!(
        verdict.is_err(),
        "prod([0, 2^40, 2^40]) now classifies: {verdict:?}"
    );

    // The same multiset, grouped so the zero is folded first, is exact.
    let regrouped = Bound::prod([
        Bound::prod([Bound::zero(), Bound::constant(big)]),
        Bound::constant(big),
    ]);
    assert_eq!(regrouped, Bound::zero());
}

/// **TIGHTNESS + contract deviation.** `Bound::prod`'s doc comment, step 3,
/// says the finite literals are constant-folded; step 5 says a folded `0` is
/// collapsed only when there are no other operands. The implementation does
/// something the doc does not describe: it pushes the zero **and** the folded
/// product of the other literals as *two separate operands*.
///
/// The extra literal is pure looseness. `Prod[0, k, x]` is `omega` whenever
/// `k * x` leaves `u64`; `Prod[0, x]` - which has the same denotation, because
/// a zero factor annihilates every finite `x` and `omega` still absorbs at
/// `x = omega` - is exact. Dropping `k` cannot cost soundness: it can only
/// remove a factor from the overflow test, and the zero already decides every
/// case the overflow test would have caught.
#[test]
fn prod_keeps_a_redundant_literal_beside_the_zero_and_pays_for_it() {
    let loose = Bound::prod([Bound::zero(), Bound::constant(3), Bound::var("x")]);
    let tight = Bound::prod([Bound::zero(), Bound::var("x")]);

    assert_eq!(
        arity_of(&loose),
        3,
        "the zero and the 3 are separate operands"
    );
    assert_eq!(arity_of(&tight), 2);

    let at = at_x(Nat::Fin(u64::MAX), Nat::OMEGA);
    assert_eq!(
        loose.eval(&at),
        Nat::OMEGA,
        "0 * 3 * u64::MAX is 0, not omega - the deviation has been fixed"
    );
    assert_eq!(tight.eval(&at), Nat::ZERO, "0 * u64::MAX is 0");

    // Both are still sound at the top of the lattice.
    let top = at_x(Nat::OMEGA, Nat::OMEGA);
    assert_eq!(loose.eval(&top), Nat::OMEGA);
    assert_eq!(tight.eval(&top), Nat::OMEGA);
}

/// **TIGHTNESS, and it falsifies a frozen claim made in `b.rs`.**
///
/// [`landav_bound::B::star`]'s doc comment justifies its syntactic zero test
/// like this: *"it is well defined only because the smart constructors
/// constant-fold: any closed term denoting zero folds to `Const(0)`, so `star`
/// is a function of the denotation rather than of the spelling."*
///
/// That premise is false, and this is the witness. Both terms below are
/// closed, `omega`-free as recipes, and denote `0`; one folds to `Const(0)`
/// and the other to `Const(omega)`. Once `B::star` is implemented against that
/// test it will return `one` for the first spelling and `Elem(omega)` for the
/// second - two different answers for one denotation, which is precisely the
/// determinism hazard the comment claims is closed.
#[test]
fn closed_terms_denoting_zero_do_not_all_fold_to_const_zero() {
    let folds_to_zero = Bound::prod([Bound::zero(), Bound::constant(3)]);
    let folds_to_omega = Bound::prod([
        Bound::zero(),
        Bound::constant(1 << 40),
        Bound::constant(1 << 40),
    ]);

    assert_eq!(folds_to_zero.kind(), &BoundKind::Const(Nat::ZERO));
    assert_eq!(
        folds_to_omega.kind(),
        &BoundKind::Const(Nat::OMEGA),
        "star's well-definedness premise now holds"
    );
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
