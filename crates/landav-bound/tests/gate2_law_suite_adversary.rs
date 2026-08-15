//! Gate 2 adversary against the **law suite itself** (LAN-59).
//!
//! `dioid_laws` lives behind `cfg(any(test, feature = "laws"))`, and the
//! feature exists precisely so that a `Dioid` instance defined *outside* this
//! crate inherits the identical conformance suite. This file is that outside
//! instance, so it is gated the same way:
//!
//! ```text
//! cargo test -p landav-bound --features laws --test gate2_law_suite_adversary
//! ```
//!
//! Note that `.github/workflows/ci.yml` runs `cargo test --workspace` with
//! default features only, so nothing in CI compiles the `laws` feature at all
//! today.
#![cfg(feature = "laws")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use landav_bound::{
    DioidLaws, b::B, bound::Bound, canonical::Canonical, dioid::Dioid,
    dioid_laws::check_dioid_laws, law::Law, lifted::Lifted, nat::Nat, semiring_id::SemiringId,
    total_valuation::TotalValuation,
};

fn trivial_valuations() -> Vec<TotalValuation> {
    vec![
        TotalValuation::with_default(BTreeMap::new(), Nat::ZERO),
        TotalValuation::with_default(BTreeMap::new(), Nat::OMEGA),
    ]
}

// ---------------------------------------------------------------------------
// 1. L4 and L6 are absorbing-only. Does L2 really cover what they cannot?
// ---------------------------------------------------------------------------

/// `B`'s algebra with `times` replaced by "always unreachable".
///
/// `dioid_laws::apparatus::l4_and_l6_can_only_ever_compare_the_bottom` records
/// that neither L4 nor L6 can falsify this mutant, and asserts that "L2's
/// identity law is what does that". That is a claim about a mutant nobody
/// built. This is the mutant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimesIsAlwaysUnreachable {}

impl Dioid for TimesIsAlwaysUnreachable {
    type Carrier = Lifted<Bound>;

    const SEMIRING: SemiringId = SemiringId::new("times-always-bottom");
    const PLUS_IDEMPOTENT: bool = false;

    fn zero() -> Self::Carrier {
        B::zero()
    }
    fn one() -> Self::Carrier {
        B::one()
    }
    fn plus(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
        B::plus(a, b)
    }
    /// The mutant: `zero` for every pair, which satisfies L4 by construction.
    fn times(_a: &Self::Carrier, _b: &Self::Carrier) -> Self::Carrier {
        Lifted::Bottom
    }
    fn star(a: &Self::Carrier) -> Self::Carrier {
        B::star(a)
    }
}

impl DioidLaws for TimesIsAlwaysUnreachable {
    fn grid() -> Vec<Self::Carrier> {
        <B as DioidLaws>::grid()
    }
    fn valuations() -> Vec<TotalValuation> {
        <B as DioidLaws>::valuations()
    }
    fn denote(value: &Self::Carrier, at: &TotalValuation) -> Lifted<Nat> {
        <B as DioidLaws>::denote(value, at)
    }
}

/// **The claim holds.** A `times` that returns `zero` unconditionally
/// satisfies L4 and L6 outright and is caught by L2's identity law, exactly as
/// the suite's own comment says.
#[test]
fn a_times_that_returns_zero_unconditionally_is_caught_by_l2() {
    let Err(reported) = check_dioid_laws::<TimesIsAlwaysUnreachable>() else {
        unreachable!("a times that annihilates everything is not a semiring");
    };
    assert_eq!(
        reported.law,
        Law::TimesMonoid,
        "L2's identity law is what covers L4 and L6's blind spot, got {reported}"
    );
    assert!(
        reported.detail.contains("times(a, one) == a"),
        "the failure must name the identity law: {reported}"
    );

    // And it is genuinely invisible to the two absorbing-only laws: run them
    // in isolation by building a suite that stops before L2 would fire.
    // `annihilation` and `zero_sum_freedom` are private, so this is asserted
    // through the public behaviour instead: the mutant satisfies L4 and L6
    // *pointwise*, which is what "absorbing-only" means.
    let grid = <TimesIsAlwaysUnreachable as DioidLaws>::grid();
    let zero = TimesIsAlwaysUnreachable::zero();
    for a in &grid {
        assert_eq!(TimesIsAlwaysUnreachable::times(&zero, a), zero, "L4 holds");
        assert_eq!(TimesIsAlwaysUnreachable::times(a, &zero), zero, "L4 holds");
    }
    for a in &grid {
        for b in &grid {
            if TimesIsAlwaysUnreachable::plus(a, b) == zero {
                assert_eq!(*a, zero, "L6 holds");
                assert_eq!(*b, zero, "L6 holds");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 2. A fifth kind of wrong instance: the denotation is instance-supplied, and
//    nothing requires it to be faithful.
// ---------------------------------------------------------------------------

/// The parity quotient `N / (n ~ n+2 for n >= 1)` with a top adjoined - the
/// carrier `dioid_laws`'s own `NotAntisymmetric` uses to prove that L7 does not
/// follow from L6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Parity {
    /// The class of `0`. The additive identity.
    Nil,
    /// The class of the odd naturals. The multiplicative identity.
    Odd,
    /// The class of the even naturals `>= 2`.
    Even,
    /// An adjoined top.
    Top,
}

impl Parity {
    const fn tag(self) -> u8 {
        match self {
            Self::Nil => 0,
            Self::Odd => 1,
            Self::Even => 2,
            Self::Top => 3,
        }
    }
}

impl Canonical for Parity {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.tag().cmp(&other.tag())
    }
    fn write_canonical(&self, out: &mut Vec<u8>) {
        out.push(self.tag());
    }
}

/// **The fifth wrong instance.**
///
/// The operations are *identical* to the suite's own `NotAntisymmetric`
/// counterexample - the same non-antisymmetric quotient, the same structure the
/// design panel called out as "parity of allocations". The only difference is
/// [`DioidLaws::denote`], which here collapses `Odd`, `Even` and `Top` onto one
/// value.
///
/// Every law in the suite is stated up to `denote`, and nothing anywhere
/// requires `denote` to be injective, to be a homomorphism, or to be the
/// carrier's real semantics. So the eleven laws all pass, and the instance is
/// still not a dioid: `Odd <= Even` and `Even <= Odd` in the canonical
/// preorder with `Odd != Even`.
///
/// This is not a defect in any of L1-L11. It is the boundary of what
/// `check_dioid_laws` certifies: it certifies the *quotient* of the carrier by
/// the instance's own denotation, never the carrier. The four shipped
/// counterexamples all supply a faithful `denote`, so none of them can find it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoarselyDenoted {}

impl Dioid for CoarselyDenoted {
    type Carrier = Parity;

    const SEMIRING: SemiringId = SemiringId::new("parity-coarse");
    // Not idempotent on the carrier - `Odd (+) Odd` is `Even` - but idempotent
    // up to the coarse denotation, which is the only thing L11 can see.
    const PLUS_IDEMPOTENT: bool = true;

    fn zero() -> Self::Carrier {
        Parity::Nil
    }

    fn one() -> Self::Carrier {
        Parity::Odd
    }

    fn plus(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
        match (a, b) {
            (Parity::Top, _) | (_, Parity::Top) => Parity::Top,
            (Parity::Nil, other) | (other, Parity::Nil) => *other,
            (Parity::Odd, Parity::Odd) | (Parity::Even, Parity::Even) => Parity::Even,
            (Parity::Odd, Parity::Even) | (Parity::Even, Parity::Odd) => Parity::Odd,
        }
    }

    fn times(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
        match (a, b) {
            (Parity::Nil, _) | (_, Parity::Nil) => Parity::Nil,
            (Parity::Top, _) | (_, Parity::Top) => Parity::Top,
            (Parity::Odd, other) | (other, Parity::Odd) => *other,
            (Parity::Even, Parity::Even) => Parity::Even,
        }
    }

    fn star(a: &Self::Carrier) -> Self::Carrier {
        match a {
            Parity::Nil => Parity::Odd,
            Parity::Odd | Parity::Even | Parity::Top => Parity::Top,
        }
    }
}

impl DioidLaws for CoarselyDenoted {
    fn grid() -> Vec<Self::Carrier> {
        vec![Parity::Nil, Parity::Odd, Parity::Even, Parity::Top]
    }

    fn valuations() -> Vec<TotalValuation> {
        trivial_valuations()
    }

    /// The whole counterexample, in four lines: `zero` is told apart from
    /// everything else - which is all L6, L8 and L9 need - and the three
    /// non-zero classes are not told apart from one another, which is what
    /// makes L7's conclusion vacuous.
    fn denote(value: &Self::Carrier, _at: &TotalValuation) -> Lifted<Nat> {
        match value {
            Parity::Nil => Lifted::Bottom,
            Parity::Odd | Parity::Even | Parity::Top => Lifted::Elem(Nat::ONE),
        }
    }
}

/// **A non-dioid that passes all eleven laws.**
///
/// The carrier's canonical preorder is not antisymmetric, which is the single
/// property [`landav_bound::dioid::Dioid`] says "defines the trait". The suite
/// passes anyway, because every law is stated up to an instance-supplied
/// `denote` that no law constrains.
#[test]
fn a_coarse_denotation_lets_a_non_dioid_pass_every_law() {
    // First: it really is not a dioid. `Odd <= Even` and `Even <= Odd` in the
    // canonical preorder `exists c. plus(a, c) == b`, and they are distinct.
    assert_eq!(
        CoarselyDenoted::plus(&Parity::Odd, &Parity::Odd),
        Parity::Even,
        "Odd <= Even, witnessed by c = Odd"
    );
    assert_eq!(
        CoarselyDenoted::plus(&Parity::Even, &Parity::Odd),
        Parity::Odd,
        "Even <= Odd, witnessed by c = Odd"
    );
    assert_ne!(Parity::Odd, Parity::Even);

    // Second: the suite certifies it anyway.
    assert!(
        check_dioid_laws::<CoarselyDenoted>().is_ok(),
        "the coarse instance was expected to pass every law: {:?}",
        check_dioid_laws::<CoarselyDenoted>()
    );

    // Third: it is the *same algebra* the suite's own L7 counterexample
    // rejects. Only `denote` differs, so `denote` is the whole of the
    // difference between "caught" and "certified".
    assert_eq!(
        <CoarselyDenoted as DioidLaws>::denote(&Parity::Odd, &trivial_valuations()[0]),
        <CoarselyDenoted as DioidLaws>::denote(&Parity::Even, &trivial_valuations()[0]),
        "the two mutually-preceding classes are indistinguishable to every law"
    );
}
