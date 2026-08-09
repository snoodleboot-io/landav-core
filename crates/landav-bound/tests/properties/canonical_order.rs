//! The canonical order: **total, deterministic, and content derived.**
//!
//! `Bound` implements neither `Ord` nor `PartialOrd`, because `b1 < b2` on a
//! symbolic cost expression reads as "b1 is tighter", which is semantic
//! domination and which this crate does not decide. The total order that
//! canonicalisation needs lives on `Canonical` instead, under a name that
//! cannot be misread.
//!
//! That split only pays for itself if the order on `Canonical` is genuinely a
//! total order and genuinely content derived. If it can tie two distinct
//! values, e-graph extraction has no unique winner. If it depends on
//! construction order, an address, an allocation, an interner index or a hash
//! seed, then one program has two cache keys and the F-008 incremental cache
//! is silently wrong across processes.

use core::cmp::Ordering;

use landav_bound::{Base, Bound, BoundKind, Canonical, Lifted, Nat, Symbol, TransKind, VarId};
use proptest::prelude::*;

use crate::support::{BoundSpec, arb_perturbation, arb_spec, build, canonical_violation, perturb};

/// Two independently built copies of one recipe: structurally identical, but
/// separately allocated, so nothing about the comparison can come from the
/// address or from `Arc` sharing.
fn built_twice(spec: &BoundSpec) -> (Bound, Bound) {
    (build(spec), build(spec))
}

/// The canonical encoding of any [`Canonical`] value, standalone.
fn canonical_bytes_of<T: Canonical>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    value.write_canonical(&mut out);
    out
}

/// The `Canonical` contract, checked over a list of **pairwise distinct**
/// values of one type.
///
/// The trait states it in four clauses, and this asserts all four:
///
/// * `canonical_cmp` returns `Equal` **exactly** when the values are equal, so
///   no two distinct values tie and extraction has a unique winner;
/// * the order is antisymmetric and transitive, so it is a total order;
/// * equal values produce equal bytes;
/// * **distinct values produce distinct bytes**, so the encoding is a faithful
///   key rather than a summary.
///
/// # Why a generic harness rather than a test per type
///
/// Six of the eleven `Canonical` implementations in this crate are never
/// called by `landav-bound` itself. `Bound::write_canonical` emits a DAG table
/// through `write_node_record`, which reaches `Nat`, `VarId`, `TransKind` and
/// `Base` directly and never touches the `Terms`, `MaxTerms`, `BoundKind` or
/// `Lifted` implementations at all. They are public API, LAN-58 and the
/// hosted platform are typed against them, and the crate's own tests cannot
/// reach them by accident - so the contract has to be asserted on the trait,
/// for every implementor, or not at all.
fn assert_canonical_contract<T: Canonical + core::fmt::Debug>(what: &str, values: &[T]) {
    for (i, left) in values.iter().enumerate() {
        assert_eq!(
            left.canonical_cmp(left),
            Ordering::Equal,
            "{what}: canonical_cmp is not reflexive at {left:?}"
        );
        for (j, right) in values.iter().enumerate() {
            let ordering = left.canonical_cmp(right);
            assert_eq!(
                ordering == Ordering::Equal,
                i == j,
                "{what}: canonical_cmp ties {left:?} and {right:?}, which are distinct values"
            );
            assert_eq!(
                ordering,
                right.canonical_cmp(left).reverse(),
                "{what}: canonical_cmp is not antisymmetric on {left:?} and {right:?}"
            );
            assert_eq!(
                canonical_bytes_of(left) == canonical_bytes_of(right),
                i == j,
                "{what}: {left:?} and {right:?} share an encoding"
            );
            for (k, third) in values.iter().enumerate() {
                let (ij, jk) = (ordering, right.canonical_cmp(third));
                if ij == Ordering::Less && jk == Ordering::Less {
                    assert_eq!(
                        left.canonical_cmp(third),
                        Ordering::Less,
                        "{what}: order is not transitive at ({i}, {j}, {k})"
                    );
                }
            }
        }
    }
}

proptest! {
    /// Agrees with `Eq`: `Equal` exactly when the values are equal. No two
    /// distinct values tie, so an unstable sort is safe and extraction has a
    /// unique winner.
    #[test]
    fn canonical_cmp_agrees_with_eq(left in arb_spec(), right in arb_spec()) {
        let (a, b) = (build(&left), build(&right));
        prop_assert_eq!(
            a.canonical_cmp(&b) == Ordering::Equal,
            a == b,
            "canonical_cmp and Eq disagree on {} vs {}",
            a,
            b
        );
    }

    /// Antisymmetric.
    #[test]
    fn canonical_cmp_is_antisymmetric(left in arb_spec(), right in arb_spec()) {
        let (a, b) = (build(&left), build(&right));
        prop_assert_eq!(a.canonical_cmp(&b), b.canonical_cmp(&a).reverse());
    }

    /// Reflexive, and stable across repeated calls within a process.
    #[test]
    fn canonical_cmp_is_reflexive_and_stable(spec in arb_spec()) {
        let (a, b) = built_twice(&spec);
        prop_assert_eq!(a.canonical_cmp(&a), Ordering::Equal);
        for _ in 0..4 {
            prop_assert_eq!(a.canonical_cmp(&b), Ordering::Equal);
        }
    }

    /// Transitive.
    #[test]
    fn canonical_cmp_is_transitive(
        first in arb_spec(),
        second in arb_spec(),
        third in arb_spec(),
    ) {
        // Sorting a triple and then asserting it is sorted tests `sort_by`,
        // not the comparator. Transitivity is checked directly, over every
        // ordered triple drawn from the three terms - which includes the
        // repeats, so reflexivity and antisymmetry are exercised too.
        let terms = [build(&first), build(&second), build(&third)];
        for x in &terms {
            for y in &terms {
                for z in &terms {
                    let (xy, yz, xz) =
                        (x.canonical_cmp(y), y.canonical_cmp(z), x.canonical_cmp(z));
                    if xy != Ordering::Greater && yz != Ordering::Greater {
                        prop_assert!(
                            xz != Ordering::Greater,
                            "{x} <= {y} <= {z} but {x} > {z}"
                        );
                    }
                    if xy == Ordering::Less && yz == Ordering::Less {
                        prop_assert!(xz == Ordering::Less, "{x} < {y} < {z} but not {x} < {z}");
                    }
                }
            }
        }
    }

    /// **Content derived, not address derived.** A clone shares the `Arc`; a
    /// rebuild does not. All three must compare `Equal`, hash equal, and
    /// encode to identical bytes.
    #[test]
    fn canonical_order_is_content_derived(spec in arb_spec()) {
        let (first, second) = built_twice(&spec);
        let cloned = first.clone();

        prop_assert_eq!(first.canonical_cmp(&second), Ordering::Equal);
        prop_assert_eq!(first.canonical_cmp(&cloned), Ordering::Equal);
        prop_assert_eq!(&first, &second);
        let (first_bytes, second_bytes) = (first.canonical_bytes(), second.canonical_bytes());
        prop_assert_eq!(first_bytes.as_bytes(), second_bytes.as_bytes());
    }

    /// Distinct values encode to distinct bytes, and equal values to equal
    /// bytes. The byte form is what the cache key is built from.
    #[test]
    fn canonical_bytes_separate_exactly_the_distinct_values(
        left in arb_spec(),
        right in arb_spec(),
    ) {
        let (a, b) = (build(&left), build(&right));
        let (a_bytes, b_bytes) = (a.canonical_bytes(), b.canonical_bytes());
        prop_assert_eq!(
            a_bytes.as_bytes() == b_bytes.as_bytes(),
            a == b,
            "canonical_bytes disagrees with Eq on {} vs {}",
            a,
            b
        );
        prop_assert!(!a_bytes.is_empty());
        prop_assert_eq!(a_bytes.len(), a_bytes.as_bytes().len());
    }

    /// The same separation, over **near-miss** pairs: two terms of identical
    /// shape everywhere that differ in exactly one kind of payload.
    ///
    /// The property above draws its two terms independently, and two
    /// independent draws essentially never agree in shape while disagreeing in
    /// a leaf. So every payload `write_node_record` writes - the variable
    /// name, the base, which of `Pow`/`Log`, the literal - could be deleted
    /// from the encoding and that property would still pass. `Bound`'s
    /// canonical bytes are the F-008 cache key material, and two programs
    /// sharing a key is the unsound direction, so this is checked on the pairs
    /// that can actually collide.
    ///
    /// Stated as an `iff`, not as "these differ": a perturbation can be
    /// absorbed (`omega + x` is `omega` whatever `x` is called), and demanding
    /// that the terms differ would be demanding that absorption stop working.
    #[test]
    fn near_miss_pairs_are_separated_by_the_canonical_bytes(
        spec in arb_spec(),
        how in arb_perturbation(),
    ) {
        let original = build(&spec);
        let nearby = build(&perturb(&spec, how));
        let (a_bytes, b_bytes) = (original.canonical_bytes(), nearby.canonical_bytes());

        prop_assert_eq!(
            a_bytes.as_bytes() == b_bytes.as_bytes(),
            original == nearby,
            "{:?} produced {} from {}: canonical_bytes and Eq disagree",
            how,
            nearby,
            original
        );
        prop_assert_eq!(
            original.canonical_cmp(&nearby) == Ordering::Equal,
            original == nearby,
            "{:?} produced {} from {}: canonical_cmp and Eq disagree",
            how,
            nearby,
            original
        );
    }

    /// The **kind** order and the **term** order are two separately written
    /// public comparators over the same data, and they must agree.
    ///
    /// `Bound::canonical_cmp` matches on `BoundKind` directly rather than
    /// delegating to `<BoundKind as Canonical>::canonical_cmp`, so nothing in
    /// the crate calls the latter at all - it is public API with no in-crate
    /// caller, and every one of its arms could be deleted without a test
    /// noticing. This is the property that notices.
    #[test]
    fn the_kind_order_agrees_with_the_term_order(left in arb_spec(), right in arb_spec()) {
        let (a, b) = (build(&left), build(&right));
        prop_assert_eq!(
            a.kind().canonical_cmp(b.kind()),
            a.canonical_cmp(&b),
            "BoundKind and Bound order {} and {} differently",
            a,
            b
        );
        prop_assert_eq!(
            canonical_bytes_of(a.kind()) == canonical_bytes_of(b.kind()),
            a == b,
            "BoundKind's encoding does not separate {} from {}",
            a,
            b
        );
    }

    /// **Construction order is not observable.** Commutative operands supplied
    /// in any permutation produce one term with one byte encoding.
    #[test]
    fn operand_order_does_not_reach_the_term(
        first in arb_spec(),
        second in arb_spec(),
        third in arb_spec(),
    ) {
        let (a, b, c) = (build(&first), build(&second), build(&third));

        let forwards = Bound::sum([a.clone(), b.clone(), c.clone()]);
        let backwards = Bound::sum([c.clone(), b.clone(), a.clone()]);
        let shuffled = Bound::sum([b.clone(), a.clone(), c.clone()]);
        prop_assert_eq!(&forwards, &backwards);
        prop_assert_eq!(&forwards, &shuffled);
        let (forwards_bytes, backwards_bytes) =
            (forwards.canonical_bytes(), backwards.canonical_bytes());
        prop_assert_eq!(forwards_bytes.as_bytes(), backwards_bytes.as_bytes());

        prop_assert_eq!(
            Bound::prod([a.clone(), b.clone(), c.clone()]),
            Bound::prod([c.clone(), a.clone(), b.clone()])
        );
        prop_assert_eq!(
            Bound::max_of([a.clone(), b.clone(), c.clone()]),
            Bound::max_of([c, b, a])
        );
    }

    /// Sorting a collection by `canonical_cmp` yields the same sequence
    /// whatever order it arrived in.
    #[test]
    fn sorting_is_permutation_independent(
        specs in proptest::collection::vec(arb_spec(), 2..6),
    ) {
        let mut forwards: Vec<Bound> = specs.iter().map(build).collect();
        let mut backwards: Vec<Bound> = specs.iter().rev().map(build).collect();
        forwards.sort_by(Canonical::canonical_cmp);
        backwards.sort_by(Canonical::canonical_cmp);
        prop_assert_eq!(forwards, backwards);
    }

    /// The invariants the order exists to serve: n-ary operands are held
    /// sorted, `Max` operands are additionally distinct, arity is `>= 2`, and
    /// depth stays inside `MAX_DEPTH`.
    #[test]
    fn built_terms_satisfy_the_canonical_invariants(spec in arb_spec()) {
        let bound = build(&spec);
        prop_assert_eq!(canonical_violation(&bound), None, "in {}", bound);
    }

    /// `Hash` matches `PartialEq`, so the derived-field cache can never make
    /// two structurally identical bounds land in different buckets - which
    /// would break `MaxTerms` deduplication.
    #[test]
    fn hash_agrees_with_equality(spec in arb_spec()) {
        use std::hash::{DefaultHasher, Hash, Hasher};

        let (a, b) = built_twice(&spec);
        let mut ha = DefaultHasher::new();
        let mut hb = DefaultHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        prop_assert_eq!(ha.finish(), hb.finish(), "{} hashed two ways", a);
    }

    /// Lifting preserves the order, and `Bottom` is the least element of the
    /// lifted order - the same shape the law suite compares denotations with.
    #[test]
    fn lifting_preserves_the_canonical_order(left in arb_spec(), right in arb_spec()) {
        let (a, b) = (build(&left), build(&right));
        let (la, lb) = (Lifted::Elem(a.clone()), Lifted::Elem(b.clone()));
        prop_assert_eq!(la.canonical_cmp(&lb), a.canonical_cmp(&b));

        let bottom: Lifted<Bound> = Lifted::Bottom;
        prop_assert_eq!(bottom.canonical_cmp(&la), Ordering::Less);
        prop_assert_eq!(la.canonical_cmp(&bottom), Ordering::Greater);
        prop_assert!(bottom.is_bottom());
        prop_assert!(!la.is_bottom());
        prop_assert_eq!(la.as_elem(), Some(&a));
        prop_assert_eq!(bottom.as_elem(), None);
    }
}

/// The order must be **content derived**, so it cannot come from declaration
/// order. `BoundShape::canonical_tag` pins that, and the canonical order over
/// terms of different shapes must follow those tags rather than anything else.
#[test]
fn canonical_order_across_shapes_follows_the_pinned_tags() {
    use landav_bound::BoundShape;

    let representatives: Vec<Bound> = BoundShape::ALL
        .iter()
        .map(|shape| build(&crate::support::irreducible_spec_of_shape(*shape)))
        .collect();

    for (i, left) in representatives.iter().enumerate() {
        assert_eq!(left.shape(), BoundShape::ALL[i]);
        for (j, right) in representatives.iter().enumerate() {
            let want = BoundShape::ALL[i]
                .canonical_tag()
                .cmp(&BoundShape::ALL[j].canonical_tag());
            if want != Ordering::Equal {
                assert_eq!(
                    left.canonical_cmp(right),
                    want,
                    "{left} vs {right}: shapes must order by canonical_tag"
                );
            }
        }
    }
}

/// `Nat`'s order is the semantic magnitude order, with `Omega` on top and
/// `Fin(u64::MAX)` strictly below it. `Nat` keeps `Ord` precisely because
/// there is no symbolic content to misread; `Bound` does not.
#[test]
fn nat_magnitude_order_is_written_out_and_puts_omega_on_top() {
    assert_eq!(Nat::Fin(0).magnitude_cmp(Nat::Fin(1)), Ordering::Less);
    assert_eq!(Nat::Fin(1).magnitude_cmp(Nat::Fin(1)), Ordering::Equal);
    assert_eq!(Nat::Fin(2).magnitude_cmp(Nat::Fin(1)), Ordering::Greater);
    assert_eq!(
        Nat::Fin(u64::MAX).magnitude_cmp(Nat::OMEGA),
        Ordering::Less,
        "Omega is a value above every finite count, not a spelling of u64::MAX"
    );
    assert_eq!(Nat::OMEGA.magnitude_cmp(Nat::OMEGA), Ordering::Equal);
    assert_eq!(Nat::OMEGA.magnitude_cmp(Nat::ZERO), Ordering::Greater);

    // The canonical order on `Nat` agrees with the magnitude order.
    assert_eq!(
        Nat::Fin(3).canonical_cmp(&Nat::OMEGA),
        Nat::Fin(3).magnitude_cmp(Nat::OMEGA)
    );
}

/// The canonical order is reached only through `Canonical::canonical_cmp`.
/// `Lifted<Nat>` is `Ord` because the law suite compares denotations with it;
/// the dual fact - `Lifted<Bound>` is **not** `Ord`, so `MaxPlus::plus` cannot
/// be written `a.max(b)` - is pinned by the `compile_fail` doctest on `Bound`.
#[test]
fn lifted_nat_is_ord_and_bottom_is_least() {
    const fn requires_ord<T: Ord>() {}
    const _: () = requires_ord::<Lifted<Nat>>();

    assert!(Lifted::Bottom < Lifted::Elem(Nat::ZERO));
    assert!(Lifted::Elem(Nat::Fin(1)) < Lifted::Elem(Nat::OMEGA));
}

// ---------------------------------------------------------------------------
// the `Canonical` contract, for every implementor
// ---------------------------------------------------------------------------

/// A corpus of pairwise-distinct terms covering all six shapes, with two or
/// more representatives of each so that a comparator arm which collapses to
/// "equal" has somewhere to be caught.
fn distinct_terms() -> Vec<Bound> {
    let var = Bound::var;
    vec![
        Bound::constant(0),
        Bound::constant(1),
        Bound::omega(),
        var("x0"),
        var("x1"),
        Bound::sum([var("x0"), var("x1")]),
        Bound::sum([var("x0"), var("x2")]),
        Bound::sum([var("x0"), var("x1"), var("x2")]),
        Bound::prod([var("x0"), var("x1")]),
        Bound::prod([var("x0"), var("x2")]),
        Bound::max_of([var("x0"), var("x1")]),
        Bound::max_of([var("x0"), var("x2")]),
        Bound::log(Base::TWO, var("x0")),
        Bound::pow(Base::TWO, var("x0")),
        Bound::log(Base::TEN, var("x0")),
        Bound::log(Base::TWO, var("x1")),
    ]
}

/// The operand payload of an n-ary node, or `None` for a node without one.
fn operands_of(bound: &Bound) -> Option<&[Bound]> {
    match bound.kind() {
        BoundKind::Sum(terms) | BoundKind::Prod(terms) => Some(terms.as_slice()),
        BoundKind::Max(terms) => Some(terms.as_slice()),
        BoundKind::Const(_) | BoundKind::Var(_) | BoundKind::Trans { .. } => None,
    }
}

/// `Bound` itself satisfies the contract its own order is named after.
#[test]
fn bound_satisfies_the_canonical_contract() {
    assert_canonical_contract("Bound", &distinct_terms());
}

/// `BoundKind` is the observation type LAN-58 pattern-matches on, and its
/// `Canonical` implementation has no caller inside this crate: `Bound`'s own
/// comparator matches the variants directly. Every arm of it - and its whole
/// encoding - is therefore reachable only from outside.
#[test]
fn bound_kind_satisfies_the_canonical_contract() {
    let kinds: Vec<BoundKind> = distinct_terms()
        .iter()
        .map(|bound| bound.kind().clone())
        .collect();
    assert_canonical_contract("BoundKind", &kinds);
}

/// `Terms` and `MaxTerms` carry the operands of every n-ary node. Neither has
/// a public constructor, so the only corpus available is the payloads of terms
/// this crate built - which is also the only corpus a caller has.
#[test]
fn the_operand_payloads_satisfy_the_canonical_contract() {
    let var = Bound::var;
    let sums = [
        Bound::sum([var("x0"), var("x1")]),
        Bound::sum([var("x0"), var("x2")]),
        Bound::sum([var("x0"), var("x1"), var("x2")]),
        Bound::sum([var("x1"), var("x2")]),
    ];
    let maxes = [
        Bound::max_of([var("x0"), var("x1")]),
        Bound::max_of([var("x0"), var("x2")]),
        Bound::max_of([var("x0"), var("x1"), var("x2")]),
        Bound::max_of([var("x1"), var("x2")]),
    ];

    let terms: Vec<landav_bound::Terms> = sums
        .iter()
        .filter_map(|bound| match bound.kind() {
            BoundKind::Sum(payload) | BoundKind::Prod(payload) => Some(payload.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(terms.len(), sums.len(), "a Sum did not observe as a Sum");
    assert_canonical_contract("Terms", &terms);

    let max_terms: Vec<landav_bound::MaxTerms> = maxes
        .iter()
        .filter_map(|bound| match bound.kind() {
            BoundKind::Max(payload) => Some(payload.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        max_terms.len(),
        maxes.len(),
        "a Max did not observe as a Max"
    );
    assert_canonical_contract("MaxTerms", &max_terms);

    // The encoding is length prefixed, so a prefix of an operand list can
    // never encode as that list. This is the concatenation hazard the prefix
    // exists to close, and it needs operand lists of *different lengths* to
    // bite - which two independent draws from `arb_spec` do produce, but only
    // with matching shapes above them.
    for pair in terms.windows(2) {
        assert_eq!(
            pair[0].len() == pair[1].len() && pair[0].as_slice() == pair[1].as_slice(),
            pair[0] == pair[1]
        );
    }
}

/// The leaf payloads the DAG encoder writes directly. Each is the sole
/// discriminator of one node record: drop `VarId`'s encoding and every
/// variable becomes the same node; drop `Base`'s and `log_2` becomes `log_4`.
#[test]
fn the_leaf_payloads_satisfy_the_canonical_contract() {
    assert_canonical_contract("TransKind", &[TransKind::Pow, TransKind::Log]);
    assert_canonical_contract(
        "Nat",
        &[Nat::Fin(0), Nat::Fin(1), Nat::Fin(u64::MAX), Nat::OMEGA],
    );
    assert_canonical_contract(
        "Base",
        &[
            Base::TWO,
            crate::support::base_of(3),
            Base::TEN,
            crate::support::base_of(1024),
            crate::support::base_of(u32::MAX),
        ],
    );
    assert_canonical_contract(
        "VarId",
        &[
            VarId::new(""),
            VarId::new("x"),
            VarId::new("x0"),
            VarId::new("x1"),
            VarId::new("xx"),
        ],
    );
    assert_canonical_contract(
        "Symbol",
        &[
            Symbol::from(""),
            Symbol::from("x"),
            Symbol::from("x0"),
            Symbol::from("xx"),
        ],
    );
}

/// `Lifted<T>` is the carrier of **both** registered semirings, so its
/// encoding is what a persisted `MaxPlus` or `B` value is keyed by. `Bottom`
/// and `Elem(Const(0))` are the two meaning-critical zeros and may never share
/// an encoding.
#[test]
fn the_lifted_carrier_satisfies_the_canonical_contract() {
    assert_canonical_contract(
        "Lifted<Bound>",
        &[
            Lifted::Bottom,
            Lifted::Elem(Bound::zero()),
            Lifted::Elem(Bound::one()),
            Lifted::Elem(Bound::var("x0")),
        ],
    );
    assert_canonical_contract(
        "Lifted<Nat>",
        &[
            Lifted::Bottom,
            Lifted::Elem(Nat::ZERO),
            Lifted::Elem(Nat::ONE),
            Lifted::Elem(Nat::OMEGA),
        ],
    );
}

/// The DAG table refers to children **by index**, so two terms whose tables
/// hold the same records in the same order are separated by nothing but that
/// wiring.
///
/// `max(x0, x1) + max(x0, x2)` and `max(x0, x1) + max(x1, x2)` are such a
/// pair: both dedup to the table `[x0, x1, Max, x2, Max, Sum]` with the root
/// at index 5, so every record - shape tag and payload - is identical between
/// them, and only the child indices differ. Nothing in the generated corpus
/// reaches this shape, and without it the child-index run could be dropped
/// from the encoding entirely.
#[test]
fn the_dag_encoding_separates_terms_that_differ_only_in_their_wiring() {
    let var = Bound::var;
    let left = Bound::sum([
        Bound::max_of([var("x0"), var("x1")]),
        Bound::max_of([var("x0"), var("x2")]),
    ]);
    let right = Bound::sum([
        Bound::max_of([var("x0"), var("x1")]),
        Bound::max_of([var("x1"), var("x2")]),
    ]);

    assert_ne!(left, right, "the two terms must be distinct to begin with");
    assert_eq!(
        left.wire_node_count(),
        right.wire_node_count(),
        "the two terms must have the same number of distinct nodes"
    );
    assert_ne!(
        left.canonical_bytes().as_bytes(),
        right.canonical_bytes().as_bytes(),
        "{left} and {right} share a canonical encoding: the DAG table records \
         the same node payloads in the same order for both, so the child \
         indices are the only thing separating them"
    );
    assert_ne!(left.canonical_cmp(&right), Ordering::Equal);

    // And the operand payloads themselves separate, which the encoding above
    // relies on but does not by itself establish.
    match (operands_of(&left), operands_of(&right)) {
        (Some(a), Some(b)) => assert_ne!(a, b),
        _ => assert_eq!(left.shape(), right.shape(), "both must be n-ary"),
    }
}

// ---------------------------------------------------------------------------
// the fingerprint
// ---------------------------------------------------------------------------

/// Every node caches a content-derived `u64` fingerprint, and it is doing two
/// jobs that the `Hash`/`Eq` contract alone does not pin:
///
/// * `Hash for Bound` writes **only** the fingerprint, so it is O(1) rather
///   than a tree walk - and `canonical_dag`, which every observer is built on,
///   deduplicates through a `HashMap<Bound, _>`. A fingerprint that does not
///   separate terms turns that map into a linear scan with a full structural
///   comparison at every step;
/// * `PartialEq` short-circuits to `false` on a fingerprint mismatch, which is
///   what the doc comment calls turning a structural comparison into "a single
///   `u64` test in the overwhelming majority of cases".
///
/// A constant fingerprint keeps both *correct* and destroys both. Nothing in
/// the suite noticed: `Hash` with an empty body, a constant `fingerprint_of`,
/// and an FNV mixer with `|=` or `&=` in place of `^=` all survived, because
/// every property here asserts only that **equal** terms agree.
///
/// So this asserts separation, over a corpus that includes the small
/// consecutive constants a damaged mixer collapses first. With a real 64-bit
/// FNV-1a the collision probability over this corpus is below `2^-45`, and the
/// corpus is fixed, so the test is deterministic rather than merely unlikely
/// to flake.
#[test]
fn the_fingerprint_separates_distinct_terms() {
    use std::collections::HashMap;
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut corpus: Vec<Bound> = Vec::new();
    // Consecutive small literals: the first thing a mixer that loses bits
    // collapses, because their eight big-endian bytes differ only in the last.
    corpus.extend((0u64..48).map(Bound::constant));
    corpus.push(Bound::omega());
    // Names, which are mixed a byte at a time.
    corpus.extend((0u32..24).map(|i| Bound::var(format!("v{i}"))));
    // Structure, so that two terms over the same leaves still separate.
    for i in 0u32..8 {
        let (a, b) = (
            Bound::var(format!("v{i}")),
            Bound::var(format!("v{}", i + 1)),
        );
        corpus.push(Bound::sum([a.clone(), b.clone()]));
        corpus.push(Bound::prod([a.clone(), b.clone()]));
        corpus.push(Bound::max_of([a.clone(), b.clone()]));
        corpus.push(Bound::log(crate::support::base_of(i + 2), a.clone()));
        corpus.push(Bound::pow(crate::support::base_of(i + 2), a));
    }

    let hash_of = |bound: &Bound| -> u64 {
        let mut hasher = DefaultHasher::new();
        bound.hash(&mut hasher);
        hasher.finish()
    };

    let mut seen: HashMap<u64, Bound> = HashMap::new();
    let mut distinct = 0usize;
    for bound in &corpus {
        // The corpus must itself be free of duplicates, or "distinct terms
        // hash apart" would be asserting something about a repeated term.
        assert!(
            !seen.values().any(|other| other == bound),
            "the corpus lists {bound} twice"
        );
        distinct += 1;
        if let Some(clash) = seen.insert(hash_of(bound), bound.clone()) {
            assert_eq!(
                &clash, bound,
                "{clash} and {bound} are distinct terms with the same fingerprint: \
                 Hash no longer separates, so canonical_dag's deduplication and \
                 PartialEq's short circuit both degrade to a full structural walk"
            );
        }
    }
    assert_eq!(
        seen.len(),
        distinct,
        "{} distinct terms produced {} distinct fingerprints",
        distinct,
        seen.len()
    );
}

/// The names that reach a user, and the names that reach the wire.
///
/// [`landav_bound::BoundShape::as_str`] is documented as "the name used in
/// diagnostics and in the wire form", and it is what renders inside
/// `BoundError::ArityExceeded`. Its six strings are as much a frozen artefact
/// as `canonical_tag`'s six numbers - a diagnostic that names the wrong
/// operator, or names none, is a report the reader cannot act on - and
/// nothing was pinning them.
#[test]
fn the_shape_names_are_pinned_and_reach_the_diagnostics() {
    use landav_bound::{BoundError, BoundShape};

    let pinned = [
        (BoundShape::Const, "const"),
        (BoundShape::Var, "var"),
        (BoundShape::Sum, "sum"),
        (BoundShape::Max, "max"),
        (BoundShape::Prod, "prod"),
        (BoundShape::Trans, "trans"),
    ];
    assert_eq!(pinned.len(), BoundShape::ALL.len());

    for (index, (shape, name)) in pinned.into_iter().enumerate() {
        assert_eq!(
            shape,
            BoundShape::ALL[index],
            "ALL is in canonical-tag order"
        );
        assert_eq!(shape.as_str(), name);
        assert_eq!(
            shape.to_string(),
            name,
            "Display and as_str must agree, or the diagnostics and the wire form diverge"
        );

        // The name has to survive into the refusal a caller actually sees.
        let refusal = BoundError::ArityExceeded {
            op: shape,
            got: 9,
            limit: 4,
        };
        let rendered = refusal.to_string();
        assert!(
            rendered.contains(name),
            "an arity refusal for {shape:?} rendered as {rendered:?}, \
             which does not name the operator"
        );
    }

    // Pairwise distinct: two shapes sharing a name make the diagnostic
    // ambiguous and the wire tag non-injective.
    for (i, (left, _)) in pinned.into_iter().enumerate() {
        for (j, (right, _)) in pinned.into_iter().enumerate() {
            assert_eq!(
                left.as_str() == right.as_str(),
                i == j,
                "{left:?} and {right:?} share the name {:?}",
                left.as_str()
            );
        }
    }
}

/// A frontend-supplied name comes back out exactly as it went in, through
/// every route core offers: the accessor, `Display` on the [`Symbol`], and
/// `Display` on the [`VarId`] that wraps it.
///
/// This is the whole of core's contract with a frontend - it "attaches no
/// meaning to either" - and it is what a user reads in
/// `BoundError::UnboundVariable` and in every rendered bound.
#[test]
fn a_frontend_name_survives_every_route_back_out() {
    for name in ["x0", "", "x", "a really quite long variable name", "x_1'"] {
        let symbol = Symbol::from(name);
        assert_eq!(symbol.as_str(), name);
        assert_eq!(
            symbol.to_string(),
            name,
            "Symbol's Display altered the name"
        );

        let var = VarId::new(name);
        assert_eq!(var.symbol().as_str(), name);
        assert_eq!(var.to_string(), name, "VarId's Display altered the name");
        assert_eq!(
            Bound::var(name).to_string(),
            name,
            "a Var term must render as its name"
        );
    }

    // Distinct names stay distinct all the way out.
    assert_ne!(VarId::new("x0").to_string(), VarId::new("x1").to_string());
    assert_ne!(
        Symbol::from("x").to_string(),
        Symbol::from("xx").to_string()
    );
}
