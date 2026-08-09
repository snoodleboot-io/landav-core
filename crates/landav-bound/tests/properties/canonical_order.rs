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

use landav_bound::{Bound, Canonical, Lifted, Nat};
use proptest::prelude::*;

use crate::support::{BoundSpec, arb_spec, build, canonical_violation};

/// Two independently built copies of one recipe: structurally identical, but
/// separately allocated, so nothing about the comparison can come from the
/// address or from `Arc` sharing.
fn built_twice(spec: &BoundSpec) -> (Bound, Bound) {
    (build(spec), build(spec))
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
