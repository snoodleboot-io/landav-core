//! LAN-56 AC2 - `log` is `ceil(log_k(max(1, b)))`, natural-valued, monotone,
//! and computed by integer arithmetic only.
//!
//! # Why the ceiling, and why this file is long
//!
//! **Too low is a soundness bug; too high is only looseness.** The floor
//! variant - `u64::ilog` - under-reports every argument that is not a power of
//! the base. `log2(3)` is `2`, not `1`; with the floor, KoAT's
//! `x1 * (log2(x1) + 2)` reports `9` at `x1 = 3` against a true `12`, and a
//! reported bound the code exceeds is the one class of bug that invalidates
//! the product.
//!
//! The other tempting spelling, `(n - 1).ilog(k) + 1`, panics on receiver `0`,
//! which is reachable from both `b = 0` and `b = 1`.
//!
//! So the ceiling is checked three ways: against the defining inequality,
//! against hand-computed values at the edges, and against a reference
//! implementation that multiplies rather than divides.

use landav_bound::{Base, Bound, BoundKind, BoundShape, Nat};
use proptest::prelude::*;

use crate::support::{
    REF_OMEGA, REFERENCE_MAX_FINITE_EXPONENT, arb_base_u32, arb_ordered_refs, arb_ref, base_of,
    nat_ref, ref_ceil_log, ref_le, ref_nat,
};

/// `Some(k^e)`, or `None` when it exceeds `u64::MAX`.
fn checked_pow_u64(k: u64, e: u64) -> Option<u64> {
    if e > 128 {
        return None;
    }
    let mut acc: u64 = 1;
    for _ in 0..e {
        acc = acc.checked_mul(k)?;
    }
    Some(acc)
}

/// The **defining** property of `ceil(log_k(n))`: `r` is the least exponent
/// whose power reaches `n`.
///
/// Independent of any implementation, including the reference one: it only
/// multiplies and compares.
fn is_ceiling_log(k: u64, n: u64, r: u64) -> bool {
    let reaches = match checked_pow_u64(k, r) {
        // `k^r` overflowed `u64`, so it certainly reached `n`.
        None => true,
        Some(v) => v >= n,
    };
    let previous_falls_short = match r.checked_sub(1) {
        None => true,
        Some(prev) => match checked_pow_u64(k, prev) {
            None => false,
            Some(v) => v < n,
        },
    };
    reaches && previous_falls_short
}

proptest! {
    /// The result satisfies the defining inequality for every finite argument
    /// and every base.
    #[test]
    fn ceil_log_is_the_least_exponent_that_reaches_the_argument(
        b in any::<u64>(),
        base in arb_base_u32(),
    ) {
        let k = base_of(base);
        let n = b.max(1);
        match Nat::Fin(b).ceil_log(k) {
            Nat::Fin(r) => prop_assert!(
                is_ceiling_log(u64::from(base), n, r),
                "log_{base}({b}) = {r} is not the least exponent with {base}^i >= {n}"
            ),
            Nat::Omega => prop_assert!(
                false,
                "log_{base}({b}) returned omega for a finite argument"
            ),
        }
    }

    /// Never under-reports. This is the soundness direction: `k^result` must
    /// reach `max(1, b)`.
    #[test]
    fn ceil_log_never_under_reports(b in any::<u64>(), base in arb_base_u32()) {
        let k = base_of(base);
        let n = b.max(1);
        match Nat::Fin(b).ceil_log(k) {
            Nat::Fin(r) => {
                let reached = checked_pow_u64(u64::from(base), r);
                prop_assert!(
                    reached.is_none_or(|v| v >= n),
                    "log_{base}({b}) = {r}, but {base}^{r} = {reached:?} < {n}"
                );
            }
            // Looseness is permitted; under-reporting is not.
            Nat::Omega => {}
        }
    }

    /// A finite argument never yields `omega`, and the result never exceeds
    /// `MAX_FINITE_EXPONENT`.
    #[test]
    fn ceil_log_of_a_finite_argument_is_finite_and_small(
        b in any::<u64>(),
        base in arb_base_u32(),
    ) {
        let got = Nat::Fin(b).ceil_log(base_of(base));
        prop_assert!(got.is_finite(), "log_{base}({b}) escaped to omega");
        prop_assert_eq!(nat_ref(got), ref_ceil_log(base, Some(b)));
        match got {
            Nat::Fin(r) => prop_assert!(
                r <= REFERENCE_MAX_FINITE_EXPONENT,
                "log_{base}({b}) = {r} exceeds the finite-exponent ceiling"
            ),
            Nat::Omega => {}
        }
    }

    /// Monotone in the argument.
    #[test]
    fn ceil_log_is_monotone_in_the_argument(
        (small, large) in arb_ordered_refs(),
        base in arb_base_u32(),
    ) {
        let k = base_of(base);
        prop_assert!(ref_le(
            nat_ref(ref_nat(small).ceil_log(k)),
            nat_ref(ref_nat(large).ceil_log(k)),
        ));
    }

    /// **Anti-monotone in the base**, which is fine and is not an argument-wise
    /// violation - but it is what makes `log_2(x) -> log_4(x)` unsound as a
    /// LAN-58 rewrite while `log_4(x) -> log_2(x)` is merely loosening.
    #[test]
    fn ceil_log_is_anti_monotone_in_the_base(
        b in arb_ref(),
        k1 in arb_base_u32(),
        k2 in arb_base_u32(),
    ) {
        let (smaller, larger) = (k1.min(k2), k1.max(k2));
        let n = ref_nat(b);
        prop_assert!(
            ref_le(
                nat_ref(n.ceil_log(base_of(larger))),
                nat_ref(n.ceil_log(base_of(smaller))),
            ),
            "log_{larger}({b:?}) must not exceed log_{smaller}({b:?})"
        );
    }

    /// `Bound::log` constant-folds a `Const` argument, which is what keeps the
    /// denotation and the syntax in step - `star` tests syntactically for a
    /// zero, so `log_2(Const(1))` and `Const(0)` must be the same term.
    #[test]
    fn bound_log_constant_folds(b in arb_ref(), base in arb_base_u32()) {
        let folded = Bound::log(base_of(base), Bound::magnitude(ref_nat(b)));
        prop_assert_eq!(folded.shape(), BoundShape::Const);
        prop_assert_eq!(
            folded.kind(),
            &BoundKind::Const(ref_nat(ref_ceil_log(base, b)))
        );
    }
}

/// The edge arguments LAN-56 calls out, for every base in a wide sweep:
/// `b = 0, 1, 2, k - 1, k, k + 1`.
#[test]
fn ceil_log_at_the_named_edges() {
    for base in [2u32, 3, 4, 5, 7, 10, 16, 63, 64, 255, 256, 1024, u32::MAX] {
        let k = base_of(base);
        let k64 = u64::from(base);
        for b in [0u64, 1, 2, k64 - 1, k64, k64 + 1] {
            let got = Nat::Fin(b).ceil_log(k);
            let n = b.max(1);
            assert!(
                got.is_finite(),
                "log_{base}({b}) returned omega for a finite argument"
            );
            if let Nat::Fin(r) = got {
                assert!(
                    is_ceiling_log(k64, n, r),
                    "log_{base}({b}) = {r} is not ceil(log_{base}(max(1, {b})))"
                );
            }
        }
        // `max(1, .)` removes the `log(0)` pole; both 0 and 1 land on 0.
        let above = k64 + 1;
        assert_eq!(Nat::ZERO.ceil_log(k), Nat::ZERO, "log_{base}(0) must be 0");
        assert_eq!(Nat::ONE.ceil_log(k), Nat::ZERO, "log_{base}(1) must be 0");
        assert_eq!(Nat::Fin(2).ceil_log(k), Nat::ONE, "log_{base}(2) must be 1");
        assert_eq!(
            Nat::Fin(k64).ceil_log(k),
            Nat::ONE,
            "log_{base}({base}) must be 1"
        );
        assert_eq!(
            Nat::Fin(above).ceil_log(k),
            Nat::Fin(2),
            "log_{base}({above}) must be 2"
        );
    }
}

/// The hand-computed values. `log2(3) == 2` is the single value that separates
/// the ceiling from the floor, and the floor variant is unsound.
#[test]
fn ceil_log_base_two_is_the_ceiling_not_the_floor() {
    let expected: [(u64, u64); 12] = [
        (0, 0),
        (1, 0),
        (2, 1),
        (3, 2),
        (4, 2),
        (5, 3),
        (7, 3),
        (8, 3),
        (9, 4),
        (15, 4),
        (16, 4),
        (17, 5),
    ];
    for (argument, want) in expected {
        assert_eq!(
            Nat::Fin(argument).ceil_log(Base::TWO),
            Nat::Fin(want),
            "ceil(log2({argument}))"
        );
    }
}

/// Base 10, so the ceiling is not accidentally satisfied by a base-2-only
/// implementation.
#[test]
fn ceil_log_base_ten_edges() {
    let expected: [(u64, u64); 8] = [
        (0, 0),
        (1, 0),
        (2, 1),
        (9, 1),
        (10, 1),
        (11, 2),
        (100, 2),
        (101, 3),
    ];
    for (argument, want) in expected {
        assert_eq!(
            Nat::Fin(argument).ceil_log(Base::TEN),
            Nat::Fin(want),
            "ceil(log10({argument}))"
        );
    }
}

/// Large arguments, near `u64::MAX`, where a float-backed implementation loses
/// the last bits and the ceiling is decided by them.
#[test]
fn ceil_log_near_u64_max() {
    let two_to_63: u64 = 1 << 63;
    assert_eq!(Nat::Fin(two_to_63 - 1).ceil_log(Base::TWO), Nat::Fin(63));
    assert_eq!(Nat::Fin(two_to_63).ceil_log(Base::TWO), Nat::Fin(63));
    assert_eq!(Nat::Fin(two_to_63 + 1).ceil_log(Base::TWO), Nat::Fin(64));
    assert_eq!(Nat::Fin(u64::MAX).ceil_log(Base::TWO), Nat::Fin(64));
    assert_eq!(Nat::Fin(u64::MAX - 1).ceil_log(Base::TWO), Nat::Fin(64));

    // 10^19 < u64::MAX < 10^20.
    assert_eq!(
        Nat::Fin(10_000_000_000_000_000_000).ceil_log(Base::TEN),
        Nat::Fin(19)
    );
    assert_eq!(
        Nat::Fin(10_000_000_000_000_000_001).ceil_log(Base::TEN),
        Nat::Fin(20)
    );
    assert_eq!(Nat::Fin(u64::MAX).ceil_log(Base::TEN), Nat::Fin(20));

    // And the result of any finite argument stays within the finite-exponent
    // ceiling, so `pow` can undo it without overflowing.
    assert_eq!(
        Nat::Fin(u64::MAX).ceil_log(Base::TWO),
        Nat::Fin(REFERENCE_MAX_FINITE_EXPONENT)
    );
}

/// `log_k(omega) == omega`, for every base.
#[test]
fn ceil_log_of_omega_is_omega() {
    for base in [2u32, 3, 10, 1024, u32::MAX] {
        assert_eq!(Nat::OMEGA.ceil_log(base_of(base)), Nat::OMEGA);
    }
    assert_eq!(
        Bound::log(Base::TWO, Bound::omega()).kind(),
        &BoundKind::Const(Nat::OMEGA)
    );
    assert_eq!(ref_ceil_log(2, REF_OMEGA), REF_OMEGA);
}

/// **The KoAT case.** With the floor, `x1 * (log2(x1) + 2)` reports `9` at
/// `x1 = 3` against a true `12`. That is a reported bound the code exceeds.
#[test]
fn koat_case_x_times_log2_x_plus_two() {
    use std::collections::BTreeMap;

    use landav_bound::{TotalValuation, VarId};

    let x1 = Bound::var("x1");
    let bound = Bound::prod([
        x1.clone(),
        Bound::sum([Bound::log(Base::TWO, x1), Bound::constant(2)]),
    ]);

    let mut known = BTreeMap::new();
    known.insert(VarId::new("x1"), Nat::Fin(3));
    let at_three = TotalValuation::with_default(known, Nat::OMEGA);

    assert_eq!(
        bound.eval(&at_three),
        Nat::Fin(12),
        "3 * (ceil(log2(3)) + 2) = 3 * (2 + 2) = 12; the floor variant reports 9"
    );

    for (x, want) in [(1u64, 2u64), (2, 6), (3, 12), (4, 16), (5, 25), (8, 40)] {
        let mut known = BTreeMap::new();
        known.insert(VarId::new("x1"), Nat::Fin(x));
        let at = TotalValuation::with_default(known, Nat::OMEGA);
        assert_eq!(
            bound.eval(&at),
            Nat::Fin(want),
            "x1 * (log2(x1) + 2) at {x}"
        );
    }
}

/// `log` and `pow` are adjoints on the naturals, and the ceiling is what makes
/// the round trip land on the safe side: `k^(ceil(log_k(n))) >= n`.
#[test]
fn pow_undoes_log_upwards() {
    for base in [2u32, 3, 10] {
        let k = base_of(base);
        for n in [1u64, 2, 3, 7, 100, 1_000_000, u64::MAX / 3] {
            let r = Nat::Fin(n).ceil_log(k);
            let back = r.exp_of(k);
            let ok = match back {
                Nat::Fin(v) => v >= n,
                Nat::Omega => true,
            };
            assert!(ok, "{base}^log_{base}({n}) = {back:?} fell below {n}");
        }
    }
}
