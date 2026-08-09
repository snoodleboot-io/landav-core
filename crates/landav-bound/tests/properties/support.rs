//! Generators plus an **implementation-independent reference semantics**.
//!
//! Nothing in this module calls a `Nat` arithmetic method, `Nat::magnitude_cmp`
//! or `Canonical::canonical_cmp` to decide what an answer *should* be. The
//! reference arithmetic below is written out from the specification in
//! `nat.rs`'s doc comments using plain `u64` checked operations, so a property
//! that compares `Bound::eval` against it is comparing the implementation
//! against the spec rather than against itself.
//!
//! `omega` is `None`. That choice is deliberate: `Option` has no `+`, no `*`
//! and no `Ord` that could quietly do the wrong thing, so every case must be
//! written out here exactly as it is written out in the crate.

use std::collections::BTreeMap;

use landav_bound::{Base, Bound, BoundKind, BoundShape, Canonical, Nat, TotalValuation, VarId};
use proptest::prelude::*;

/// The variables every generated term draws from.
pub const VAR_NAMES: [&str; 3] = ["x0", "x1", "x2"];

/// A cost magnitude in the reference semantics. `None` is `omega`.
pub type Ref = Option<u64>;

/// The top of the reference lattice.
pub const REF_OMEGA: Ref = None;

/// `Nat::MAX_FINITE_EXPONENT`, restated so the tests do not read the value they
/// are checking out of the type under test.
pub const REFERENCE_MAX_FINITE_EXPONENT: u64 = 64;

// ---------------------------------------------------------------------------
// reference arithmetic on `N u {omega}`
// ---------------------------------------------------------------------------

/// Addition. Overflow goes to `omega`, never to `u64::MAX`.
#[must_use]
pub fn ref_plus(a: Ref, b: Ref) -> Ref {
    match (a, b) {
        (Some(x), Some(y)) => x.checked_add(y),
        _ => REF_OMEGA,
    }
}

/// Multiplication. `omega` absorbs **unconditionally**, so `0 * omega` is
/// `omega`. Overflow goes to `omega`, never to `u64::MAX`.
#[must_use]
pub fn ref_times(a: Ref, b: Ref) -> Ref {
    match (a, b) {
        (Some(x), Some(y)) => x.checked_mul(y),
        _ => REF_OMEGA,
    }
}

/// Lattice join.
#[must_use]
pub fn ref_join(a: Ref, b: Ref) -> Ref {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        _ => REF_OMEGA,
    }
}

/// Lattice meet. Only used to build ordered valuation pairs.
#[must_use]
pub fn ref_meet(a: Ref, b: Ref) -> Ref {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => REF_OMEGA,
    }
}

/// The magnitude order, with `omega` on top.
#[must_use]
pub fn ref_le(a: Ref, b: Ref) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x <= y,
        (Some(_), None) | (None, None) => true,
        (None, Some(_)) => false,
    }
}

/// `base ^ exponent`, saturating to `omega`.
///
/// The exponent is tested against the finite-exponent ceiling **before** any
/// narrowing, which is the step whose absence made `pow(2, 2^32)` report `1`.
#[must_use]
pub fn ref_pow(base: u32, exponent: Ref) -> Ref {
    let e = exponent?;
    if e >= REFERENCE_MAX_FINITE_EXPONENT {
        return REF_OMEGA;
    }
    let narrowed = u32::try_from(e).ok()?;
    u64::from(base).checked_pow(narrowed)
}

/// `ceil(log_base(max(1, argument)))`, by integer multiplication only.
///
/// Written as "the least `i` with `base^i >= max(1, argument)`", which is the
/// *definition* of the ceiling, rather than as any adjustment of a floor.
#[must_use]
pub fn ref_ceil_log(base: u32, argument: Ref) -> Ref {
    let n = argument?.max(1);
    let k = u64::from(base);
    let mut acc: u64 = 1;
    let mut i: u64 = 0;
    while acc < n {
        i += 1;
        match acc.checked_mul(k) {
            Some(next) => acc = next,
            // `k^i` exceeded `u64::MAX`, so it certainly reached `n`.
            None => return Some(i),
        }
    }
    Some(i)
}

/// Reads a `Nat` without going through `Ord` or `magnitude_cmp`.
#[must_use]
pub fn nat_ref(n: Nat) -> Ref {
    match n {
        Nat::Fin(v) => Some(v),
        Nat::Omega => REF_OMEGA,
    }
}

/// Builds a `Nat` from a reference magnitude.
#[must_use]
pub fn ref_nat(r: Ref) -> Nat {
    match r {
        Some(v) => Nat::Fin(v),
        None => Nat::OMEGA,
    }
}

/// A validated base. Every generator produces `k >= 2`, so the fallback is
/// unreachable for generated values; it exists so the tests never call
/// `unwrap`.
#[must_use]
pub fn base_of(k: u32) -> Base {
    Base::new(k).unwrap_or(Base::TWO)
}

// ---------------------------------------------------------------------------
// terms as inert data
// ---------------------------------------------------------------------------

/// A term recipe.
///
/// Generation produces one of these rather than a `Bound` directly, for two
/// reasons. It keeps every `todo!()` inside the *test body* where proptest can
/// attribute the failure, and it gives the denotation property a naive term to
/// interpret independently of the smart constructors that folded it.
#[derive(Debug, Clone)]
pub enum BoundSpec {
    /// A literal magnitude, possibly `omega`.
    Const(Ref),
    /// An index into [`VAR_NAMES`].
    Var(usize),
    /// `t0 + t1 + ...`.
    Sum(Vec<BoundSpec>),
    /// `max(t0, t1, ...)`.
    Max(Vec<BoundSpec>),
    /// `t0 * t1 * ...`.
    Prod(Vec<BoundSpec>),
    /// `base ^ arg`, or `ceil(log_base(max(1, arg)))` when `log`.
    Trans {
        /// `true` for `Log`, `false` for `Pow`.
        log: bool,
        /// Always `>= 2` as generated.
        base: u32,
        /// The single operand.
        arg: Box<BoundSpec>,
    },
}

impl BoundSpec {
    /// `true` iff `omega` occurs syntactically anywhere in this recipe.
    #[must_use]
    pub fn mentions_omega(&self) -> bool {
        match self {
            Self::Const(r) => r.is_none(),
            Self::Var(_) => false,
            Self::Sum(xs) | Self::Max(xs) | Self::Prod(xs) => xs.iter().any(Self::mentions_omega),
            Self::Trans { arg, .. } => arg.mentions_omega(),
        }
    }

    /// Every variable index occurring in this recipe.
    pub fn var_indices(&self, out: &mut Vec<usize>) {
        match self {
            Self::Const(_) => {}
            Self::Var(i) => out.push(*i),
            Self::Sum(xs) | Self::Max(xs) | Self::Prod(xs) => {
                for x in xs {
                    x.var_indices(out);
                }
            }
            Self::Trans { arg, .. } => arg.var_indices(out),
        }
    }
}

/// Builds the recipe through the smart constructors.
#[must_use]
pub fn build(spec: &BoundSpec) -> Bound {
    match spec {
        BoundSpec::Const(r) => Bound::magnitude(ref_nat(*r)),
        BoundSpec::Var(i) => Bound::var(VAR_NAMES[*i % VAR_NAMES.len()]),
        BoundSpec::Sum(xs) => Bound::sum(xs.iter().map(build)),
        BoundSpec::Max(xs) => Bound::max_of(xs.iter().map(build)),
        BoundSpec::Prod(xs) => Bound::prod(xs.iter().map(build)),
        BoundSpec::Trans { log, base, arg } => {
            let b = base_of(*base);
            if *log {
                Bound::log(b, build(arg))
            } else {
                Bound::pow(b, build(arg))
            }
        }
    }
}

/// The naive interpretation: no flattening, no folding, no absorption
/// shortcuts, no sorting. Just the denotation of the recipe as written.
///
/// This is the "naive interpretation" that [`build`] must agree with for the
/// smart constructors to be denotation preserving.
#[must_use]
pub fn naive_eval(spec: &BoundSpec, env: &Env) -> Ref {
    match spec {
        BoundSpec::Const(r) => *r,
        BoundSpec::Var(i) => env.value_of(*i),
        BoundSpec::Sum(xs) => xs
            .iter()
            .map(|x| naive_eval(x, env))
            .fold(Some(0), ref_plus),
        BoundSpec::Max(xs) => xs
            .iter()
            .map(|x| naive_eval(x, env))
            .fold(Some(0), ref_join),
        BoundSpec::Prod(xs) => xs
            .iter()
            .map(|x| naive_eval(x, env))
            .fold(Some(1), ref_times),
        BoundSpec::Trans { log, base, arg } => {
            let inner = naive_eval(arg, env);
            if *log {
                ref_ceil_log(*base, inner)
            } else {
                ref_pow(*base, inner)
            }
        }
    }
}

/// Builds a recipe of the requested shape.
///
/// The `match` is exhaustive over [`BoundShape::ALL`] on purpose: a seventh
/// constructor is then a **compile error in this test suite**, not a review
/// checklist item. That is LAN-56 AC1 mechanised.
#[must_use]
pub fn spec_of_shape(
    shape: BoundShape,
    a: BoundSpec,
    b: BoundSpec,
    base: u32,
    log: bool,
) -> BoundSpec {
    match shape {
        // `omega` inside `Const`, not beside it: there is no seventh variant
        // to reach for.
        BoundShape::Const => BoundSpec::Const(REF_OMEGA),
        BoundShape::Var => BoundSpec::Var(0),
        BoundShape::Sum => BoundSpec::Sum(vec![a, b]),
        BoundShape::Max => BoundSpec::Max(vec![a, b]),
        BoundShape::Prod => BoundSpec::Prod(vec![a, b]),
        BoundShape::Trans => BoundSpec::Trans {
            log,
            base,
            arg: Box::new(a),
        },
    }
}

/// A recipe of the requested shape whose smart constructor cannot collapse it
/// to a different shape, so `shape()` observably round-trips.
#[must_use]
pub fn irreducible_spec_of_shape(shape: BoundShape) -> BoundSpec {
    match shape {
        BoundShape::Const => BoundSpec::Const(Some(7)),
        BoundShape::Var => BoundSpec::Var(0),
        BoundShape::Sum => BoundSpec::Sum(vec![BoundSpec::Var(0), BoundSpec::Var(1)]),
        BoundShape::Max => BoundSpec::Max(vec![BoundSpec::Var(0), BoundSpec::Var(1)]),
        BoundShape::Prod => BoundSpec::Prod(vec![BoundSpec::Var(0), BoundSpec::Var(1)]),
        BoundShape::Trans => BoundSpec::Trans {
            log: false,
            base: 2,
            arg: Box::new(BoundSpec::Var(0)),
        },
    }
}

// ---------------------------------------------------------------------------
// valuations
// ---------------------------------------------------------------------------

/// A valuation, held as reference magnitudes so a test can order two of them
/// pointwise without consulting the type under test.
#[derive(Debug, Clone)]
pub struct Env {
    /// The value of each name in [`VAR_NAMES`].
    pub vals: [Ref; 3],
    /// The value of every other variable.
    pub default: Ref,
}

impl Env {
    /// Every variable maps to `omega` - the only sound policy for analysis.
    #[must_use]
    pub fn all_omega() -> Self {
        Self {
            vals: [REF_OMEGA; 3],
            default: REF_OMEGA,
        }
    }

    /// The magnitude of the `i`th generated variable.
    #[must_use]
    pub fn value_of(&self, i: usize) -> Ref {
        self.vals[i % VAR_NAMES.len()]
    }

    /// This env with the `i`th variable rebound.
    #[must_use]
    pub fn with(&self, i: usize, v: Ref) -> Self {
        let mut next = self.clone();
        next.vals[i % VAR_NAMES.len()] = v;
        next
    }

    /// Pointwise order, over the generated variables **and** the default that
    /// stands for every other variable.
    #[must_use]
    pub fn le(&self, other: &Self) -> bool {
        ref_le(self.default, other.default)
            && self
                .vals
                .iter()
                .zip(other.vals.iter())
                .all(|(a, b)| ref_le(*a, *b))
    }

    /// The corresponding [`TotalValuation`].
    #[must_use]
    pub fn valuation(&self) -> TotalValuation {
        let mut known = BTreeMap::new();
        for (i, name) in VAR_NAMES.iter().enumerate() {
            known.insert(VarId::new(*name), ref_nat(self.vals[i]));
        }
        TotalValuation::with_default(known, ref_nat(self.default))
    }
}

// ---------------------------------------------------------------------------
// structural invariants
// ---------------------------------------------------------------------------

/// Describes the first canonical-form invariant `root` violates, if any.
///
/// Checks, at every node: arity `>= 2` for the n-ary shapes, operands
/// non-decreasing under `canonical_cmp` for `Sum`/`Prod`, operands *strictly*
/// increasing for `Max` (which is sortedness and pairwise distinctness at
/// once), `is_empty()` consistent with `len()`, `shape()` consistent with
/// `kind()`, and `depth()` within [`landav_bound::MAX_DEPTH`].
#[must_use]
pub fn canonical_violation(root: &Bound) -> Option<String> {
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node.depth() > landav_bound::MAX_DEPTH {
            return Some(format!("depth {} exceeds MAX_DEPTH", node.depth()));
        }
        match node.kind() {
            BoundKind::Const(_) => {
                if node.shape() != BoundShape::Const {
                    return Some("shape() disagrees with kind() at Const".to_owned());
                }
            }
            BoundKind::Var(_) => {
                if node.shape() != BoundShape::Var {
                    return Some("shape() disagrees with kind() at Var".to_owned());
                }
            }
            BoundKind::Sum(terms) | BoundKind::Prod(terms) => {
                if terms.len() < 2 || terms.is_empty() {
                    return Some(format!("n-ary node has arity {}", terms.len()));
                }
                let operands = terms.as_slice();
                if operands.len() != terms.len() {
                    return Some("len() disagrees with as_slice()".to_owned());
                }
                for pair in operands.windows(2) {
                    if pair[0].canonical_cmp(&pair[1]) == core::cmp::Ordering::Greater {
                        return Some("Sum/Prod operands are not in canonical order".to_owned());
                    }
                }
                stack.extend(operands.iter().cloned());
            }
            BoundKind::Max(terms) => {
                if terms.len() < 2 || terms.is_empty() {
                    return Some(format!("Max node has arity {}", terms.len()));
                }
                let operands = terms.as_slice();
                if operands.len() != terms.len() {
                    return Some("len() disagrees with as_slice()".to_owned());
                }
                for pair in operands.windows(2) {
                    if pair[0].canonical_cmp(&pair[1]) != core::cmp::Ordering::Less {
                        return Some(
                            "Max operands are not strictly increasing (unsorted or duplicated)"
                                .to_owned(),
                        );
                    }
                }
                stack.extend(operands.iter().cloned());
            }
            BoundKind::Trans { base, arg, .. } => {
                if base.get() < 2 {
                    return Some(format!("base {} is below 2", base.get()));
                }
                stack.push(arg.clone());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// strategies
// ---------------------------------------------------------------------------

/// Magnitudes: small naturals, the meaning-critical `0` and `1`, values near
/// `u64::MAX` where saturation must engage, and `omega`.
pub fn arb_ref() -> impl Strategy<Value = Ref> {
    prop_oneof![
        6 => (0u64..8).prop_map(Some),
        3 => any::<u64>().prop_map(Some),
        3 => prop_oneof![
            Just(Some(0u64)),
            Just(Some(1u64)),
            Just(Some(2u64)),
            Just(Some(u64::MAX)),
            Just(Some(u64::MAX - 1)),
            Just(Some(1u64 << 63)),
            Just(Some((1u64 << 32) + 1)),
        ],
        2 => Just(REF_OMEGA),
    ]
}

/// Bases, always `>= 2`.
pub fn arb_base_u32() -> impl Strategy<Value = u32> {
    prop_oneof![
        4 => 2u32..=16,
        1 => Just(2u32),
        1 => Just(10u32),
        1 => 2u32..=1024,
    ]
}

/// Arbitrary terms of bounded depth, covering all six constructors.
pub fn arb_spec() -> impl Strategy<Value = BoundSpec> {
    let leaf = prop_oneof![
        3 => arb_ref().prop_map(BoundSpec::Const),
        3 => (0usize..VAR_NAMES.len()).prop_map(BoundSpec::Var),
    ];
    leaf.prop_recursive(4, 32, 3, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 2..4).prop_map(BoundSpec::Sum),
            proptest::collection::vec(inner.clone(), 2..4).prop_map(BoundSpec::Max),
            proptest::collection::vec(inner.clone(), 2..4).prop_map(BoundSpec::Prod),
            (any::<bool>(), arb_base_u32(), inner).prop_map(|(log, base, arg)| {
                BoundSpec::Trans {
                    log,
                    base,
                    arg: Box::new(arg),
                }
            }),
        ]
    })
}

/// An arbitrary valuation.
pub fn arb_env() -> impl Strategy<Value = Env> {
    (proptest::array::uniform3(arb_ref()), arb_ref())
        .prop_map(|(vals, default)| Env { vals, default })
}

/// Two valuations `(lo, hi)` with `lo <= hi` pointwise **by construction**,
/// so the antecedent of the monotonicity property is never merely assumed.
pub fn arb_env_pair() -> impl Strategy<Value = (Env, Env)> {
    (
        proptest::array::uniform3(arb_ref()),
        proptest::array::uniform3(arb_ref()),
        arb_ref(),
        arb_ref(),
    )
        .prop_map(|(left, right, d1, d2)| {
            let mut lo = [REF_OMEGA; 3];
            let mut hi = [REF_OMEGA; 3];
            for i in 0..3 {
                lo[i] = ref_meet(left[i], right[i]);
                hi[i] = ref_join(left[i], right[i]);
            }
            (
                Env {
                    vals: lo,
                    default: ref_meet(d1, d2),
                },
                Env {
                    vals: hi,
                    default: ref_join(d1, d2),
                },
            )
        })
}

/// Two magnitudes `(lo, hi)` with `lo <= hi`.
pub fn arb_ordered_refs() -> impl Strategy<Value = (Ref, Ref)> {
    (arb_ref(), arb_ref()).prop_map(|(a, b)| (ref_meet(a, b), ref_join(a, b)))
}
