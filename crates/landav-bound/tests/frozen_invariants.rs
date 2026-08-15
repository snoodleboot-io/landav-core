//! The invariants that stand between this crate and a **hang**.
//!
//! # Why this is a separate, fast test target
//!
//! Three of `landav-bound`'s loops have no independent iteration bound. They
//! terminate because an invariant is held *somewhere else*:
//!
//! | loop | terminates because |
//! |---|---|
//! | [`Nat::ceil_log`]'s accumulator | [`Base`] is `>= 2`, so `base^i` strictly grows |
//! | `Bound::assemble`'s flattening | `Terms::len` reports the real arity, so the `MAX_NODES` guard sees the doubling |
//! | `Bound::canonical_dag`'s worklist | `Bound: PartialEq` deduplicates, so a shared DAG is not unfolded into its tree |
//!
//! Weaken any one of them and the property suite in `tests/properties/` does
//! not fail - it **runs forever**. Mutation testing measured exactly that:
//! `Base::get -> 0`, `Base::get -> 1`, `Terms::len -> 0`, `Terms::len -> 1`
//! and `<Bound as PartialEq>::eq -> false` were all killed by a 120-second
//! clock rather than by an assertion, and a mutant killed by the clock is
//! indistinguishable from CI being slow.
//!
//! Cargo runs integration-test targets in name order and stops at the first
//! failing target, so this one - no property test, no generator, no term
//! larger than a few dozen nodes - completes before `properties` starts.
//! Weakening any of the three invariants above now produces a named assertion
//! failure in well under a second.
//!
//! # This file must never run the loops it protects
//!
//! A test that *calls* `ceil_log` with a broken base hangs, and one hanging
//! test blocks the whole binary from reporting however many of its siblings
//! already failed. So nothing here calls `ceil_log`, `exp_of`, `Bound::log` or
//! `Bound::pow`, and nothing builds a term whose operand list can double. The
//! loops themselves are exercised at length in `tests/properties/`; this
//! target asserts only the invariants that keep them finite, and its whole
//! value is that it **finishes**.

use core::cmp::Ordering;

use landav_bound::{Base, Bound, BoundError, BoundKind, Canonical, Nat, VarId};

// ---------------------------------------------------------------------------
// Base >= 2: the only bound on `Nat::ceil_log`'s accumulate loop
// ---------------------------------------------------------------------------

/// `ceil_log` accumulates `base^i` until it reaches its argument and has no
/// iteration cap of its own. At `base == 1` the accumulator never grows; at
/// `base == 0` it drops to zero and stays there. Both are non-termination, and
/// the *only* thing making them unreachable is that [`Base`] refuses to hold
/// either value.
#[test]
fn base_refuses_every_value_that_would_make_ceil_log_diverge() {
    for bad in [0u32, 1] {
        let refused = Base::new(bad);
        assert!(
            matches!(refused, Err(BoundError::BaseTooSmall { got }) if got == bad),
            "Base::new({bad}) must be BaseTooSmall; ceil_log does not terminate at base {bad}"
        );
        assert!(
            Base::try_from(bad).is_err(),
            "Base::try_from({bad}) must refuse too - it is the path serde takes, and \
             serde must not be an escape hatch around the invariant"
        );
    }
}

/// Validation must not *alter* the base it accepts. `Base::get` is what
/// `ceil_log` and `exp_of` read as the multiplier, so a `get` that disagrees
/// with the value `new` validated re-opens the hole `new` exists to close.
#[test]
fn a_validated_base_reports_the_value_it_was_validated_from() {
    for good in [2u32, 3, 10, 16, 1024, u32::MAX] {
        assert_eq!(
            Base::new(good).ok().map(Base::get),
            Some(good),
            "Base::new({good}) must be accepted and must report {good} back"
        );
    }
}

/// The two constants bypass [`Base::new`] entirely, so they are a second,
/// unchecked way to inhabit the type. They must agree with the checked path.
#[test]
fn the_base_constants_agree_with_the_checked_path() {
    assert_eq!(Base::TWO.get(), 2);
    assert_eq!(Base::TEN.get(), 10);
    assert_eq!(Base::new(2).ok(), Some(Base::TWO));
    assert_eq!(Base::new(10).ok(), Some(Base::TEN));
    assert!(
        Base::TWO.get() >= 2 && Base::TEN.get() >= 2,
        "a constant below 2 makes ceil_log diverge without ever calling Base::new"
    );
}

// ---------------------------------------------------------------------------
// arity: the only bound on `assemble`'s flattening
// ---------------------------------------------------------------------------

/// The operands of an n-ary node, whatever payload type carries them.
fn operands_of(bound: &Bound) -> &[Bound] {
    match bound.kind() {
        BoundKind::Sum(terms) | BoundKind::Prod(terms) => terms.as_slice(),
        BoundKind::Max(terms) => terms.as_slice(),
        BoundKind::Const(_) | BoundKind::Var(_) | BoundKind::Trans { .. } => &[],
    }
}

/// The arity of an n-ary node, as `Bound::assemble`'s budget reads it, and
/// `None` for a node that has no operands.
fn reported_arity(bound: &Bound) -> Option<usize> {
    match bound.kind() {
        BoundKind::Sum(terms) | BoundKind::Prod(terms) => Some(terms.len()),
        BoundKind::Max(terms) => Some(terms.len()),
        BoundKind::Const(_) | BoundKind::Var(_) | BoundKind::Trans { .. } => None,
    }
}

/// `Terms::len` and `MaxTerms::len` are what `Bound::assemble` budgets a
/// flattened operand list against. Flattening a same-operator child *doubles*
/// that list while leaving the depth and the DAG size constant, so neither
/// `MAX_DEPTH` nor the node budget can see it - `len` is the only measure that
/// does. A `len` that under-reports removes the guard altogether, and forty
/// doublings then ask for a `Vec` of `2^40` handles.
#[test]
fn operand_count_is_reported_honestly() {
    let leaves: Vec<Bound> = (0..5).map(|i| Bound::var(format!("v{i}"))).collect();

    for arity in 2..=5usize {
        let supplied = &leaves[..arity];
        for node in [
            Bound::sum(supplied.to_vec()),
            Bound::prod(supplied.to_vec()),
            Bound::max_of(supplied.to_vec()),
        ] {
            assert_eq!(
                reported_arity(&node),
                Some(arity),
                "{node} carries {arity} distinct operands and reports another count"
            );
            assert_eq!(
                reported_arity(&node),
                Some(operands_of(&node).len()),
                "len() and as_slice() disagree on {node}"
            );
        }
    }
}

/// The arity invariant `Terms` and `MaxTerms` document: two or more, never
/// empty. `is_empty` is constantly `false` by construction, which is exactly
/// why the *positive* half has to be asserted somewhere.
#[test]
fn an_n_ary_node_is_never_empty() {
    for node in [
        Bound::sum([Bound::var("a"), Bound::var("b")]),
        Bound::prod([Bound::var("a"), Bound::var("b")]),
        Bound::max_of([Bound::var("a"), Bound::var("b")]),
    ] {
        assert_eq!(reported_arity(&node), Some(2), "{node} is not binary");
        assert!(
            !operands_of(&node).is_empty(),
            "{node} reported no operands"
        );
    }
}

// ---------------------------------------------------------------------------
// structural equality: the only bound on `canonical_dag`'s worklist
// ---------------------------------------------------------------------------

/// `b_{i+1} = (b_i * b_i) + 1`: two new DAG nodes per level and a tree that
/// doubles - the shape every observer must walk as a DAG rather than a tree.
/// The `Prod` child is a `Sum` and the `Sum` child is a `Prod`, so no operand
/// list ever flattens and the sharing survives.
fn shared_ladder(levels: u32) -> Bound {
    let mut bound = Bound::var("x");
    for _ in 0..levels {
        let squared = Bound::prod([bound.clone(), bound.clone()]);
        bound = Bound::sum([squared, Bound::constant(1)]);
    }
    bound
}

/// Every observer on [`Bound`] deduplicates its worklist through a
/// `HashMap<Bound, _>`, so an equality that never holds turns a `k`-node DAG
/// back into its `2^k`-node tree. That is not a wrong answer, it is no answer:
/// before the DAG traversal landed, `canonical_bytes` on a twenty-level ladder
/// allocated 39 MB.
///
/// The property is the ordinary one - equality is reflexive, and equal terms
/// built by different routes compare equal - stated on the term where losing
/// it costs termination rather than correctness.
#[test]
fn structural_equality_holds_on_the_shape_that_needs_it() {
    let ladder = shared_ladder(8);

    assert_eq!(ladder, ladder.clone(), "equality is not reflexive");
    assert_eq!(
        ladder,
        shared_ladder(8),
        "two independently built copies of one term compare unequal"
    );
    assert_ne!(
        ladder,
        shared_ladder(7),
        "two ladders of different height compare equal"
    );

    // The consequence: the DAG stays a DAG. Eight levels is 2^8 tree nodes and
    // eighteen distinct ones.
    let dag = ladder.wire_node_count();
    assert!(
        dag <= 32,
        "an 8-level ladder reported {dag} distinct nodes; the sharing was lost"
    );
}

/// The leaf equality every recursive comparison bottoms out in, over the
/// meaning-critical constants that must never be conflated.
#[test]
fn leaf_equality_separates_the_meaning_critical_values() {
    assert_eq!(Bound::zero(), Bound::constant(0));
    assert_eq!(Bound::omega(), Bound::magnitude(Nat::OMEGA));
    assert_ne!(Bound::zero(), Bound::one());
    assert_ne!(Bound::zero(), Bound::omega());
    assert_eq!(Bound::var("x"), Bound::var("x"));
    assert_ne!(Bound::var("x"), Bound::var("y"));
    assert_eq!(
        Bound::var("x").canonical_cmp(&Bound::var("x")),
        Ordering::Equal
    );
    assert_eq!(Bound::var("x").vars(), vec![VarId::new("x")]);
}
