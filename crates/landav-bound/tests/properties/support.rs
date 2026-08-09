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
    Some(ceil_log_u64(base, argument?))
}

/// The least `i` with `base^i >= max(1, argument)`.
#[must_use]
pub fn ceil_log_u64(base: u32, argument: u64) -> u64 {
    let n = argument.max(1);
    let k = u64::from(base);
    let mut acc: u64 = 1;
    let mut i: u64 = 0;
    while acc < n {
        i += 1;
        match acc.checked_mul(k) {
            Some(next) => acc = next,
            // `k^i` exceeded `u64::MAX`, so it certainly reached `n`.
            None => return i,
        }
    }
    i
}

/// The least `i` with `base^i >= 2^64`, computed in `u128`.
///
/// Every [`Ideal::Beyond`] magnitude is at least `2^64`, so this is a valid
/// **lower** bound on its ceiling log - the direction that keeps the reference
/// below the implementation, which is the direction soundness is checked in.
#[must_use]
pub fn ceil_log_beyond(base: u32) -> u64 {
    let target: u128 = 1 << 64;
    let k = u128::from(base);
    let mut acc: u128 = 1;
    let mut i: u64 = 0;
    while acc < target {
        i += 1;
        acc *= k;
    }
    i
}

// ---------------------------------------------------------------------------
// the ideal denotation
// ---------------------------------------------------------------------------

/// A magnitude in the **ideal** semantics - the function the algebra
/// over-approximates - computed without saturating at intermediate steps.
///
/// # Why three cases and not two
///
/// `Beyond` is a magnitude that is *finite but larger than `u64::MAX`*. It
/// observes as `omega`, but it is **not** `omega`, and the distinction is the
/// whole reason this type exists.
///
/// Saturating multiplication on `N u {omega}` is **not associative**:
/// `(2 * u64::MAX) * 0` saturates to `omega` and then absorbs to `omega`,
/// while `2 * (u64::MAX * 0)` is `0`. A left fold with `checked_mul` therefore
/// gives an n-ary product two different answers for the same multiset of
/// factors, which is not a semantics at all - `Prod` is commutative, its
/// operands are held in canonical order, and a reference that depends on the
/// order they were supplied in cannot be a denotation.
///
/// Keeping `Beyond` distinct from `Omega` restores associativity, because a
/// zero factor annihilates a merely-large one (`Beyond * 0 = 0`) while
/// `omega` still absorbs unconditionally (`Omega * 0 = Omega`, the LAN-73
/// rule). All three folds below are then over genuinely associative and
/// commutative operations, so the reference is a function of the operand
/// *multiset*, as `Prod` requires.
///
/// Saturation happens exactly once, at the observation point
/// ([`ideal_ref`]) - which is where `Bound::eval` returns a `Nat` and where
/// the loss of precision is a deliberate, sound, upward rounding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ideal {
    /// A natural number that fits in `u64`.
    Fin(u64),
    /// Finite, but larger than `u64::MAX`.
    Beyond,
    /// Unbounded.
    Omega,
}

/// Lifts an observed magnitude into the ideal domain.
#[must_use]
pub fn ideal_of(r: Ref) -> Ideal {
    match r {
        Some(v) => Ideal::Fin(v),
        None => Ideal::Omega,
    }
}

/// Observes an ideal magnitude, saturating `Beyond` to `omega`.
#[must_use]
pub fn ideal_ref(i: Ideal) -> Ref {
    match i {
        Ideal::Fin(v) => Some(v),
        Ideal::Beyond | Ideal::Omega => REF_OMEGA,
    }
}

/// The ideal magnitude order: `Fin(_) < Beyond < Omega`.
///
/// `Beyond` is strictly below `Omega`, which is the distinction that makes a
/// domination antecedent mean what it says: a replacement whose true value is
/// merely *too large for `u64`* does not dominate a variable that is genuinely
/// unbounded.
#[must_use]
pub fn ideal_le(a: Ideal, b: Ideal) -> bool {
    match (a, b) {
        (_, Ideal::Omega) => true,
        (Ideal::Omega, _) => false,
        (_, Ideal::Beyond) => true,
        (Ideal::Beyond, _) => false,
        (Ideal::Fin(x), Ideal::Fin(y)) => x <= y,
    }
}

/// Ideal addition. Associative and commutative.
#[must_use]
pub fn ideal_plus(a: Ideal, b: Ideal) -> Ideal {
    match (a, b) {
        (Ideal::Omega, _) | (_, Ideal::Omega) => Ideal::Omega,
        (Ideal::Beyond, _) | (_, Ideal::Beyond) => Ideal::Beyond,
        (Ideal::Fin(x), Ideal::Fin(y)) => match x.checked_add(y) {
            Some(sum) => Ideal::Fin(sum),
            None => Ideal::Beyond,
        },
    }
}

/// Ideal multiplication. Associative and commutative.
///
/// `omega` absorbs unconditionally, including against zero - that is the
/// frozen LAN-73 rule and it is checked here first. A zero factor then
/// annihilates everything else, including a `Beyond`, because the product
/// really is zero and only saturation could have invented an `omega`.
#[must_use]
pub fn ideal_times(a: Ideal, b: Ideal) -> Ideal {
    match (a, b) {
        (Ideal::Omega, _) | (_, Ideal::Omega) => Ideal::Omega,
        (Ideal::Fin(0), _) | (_, Ideal::Fin(0)) => Ideal::Fin(0),
        (Ideal::Beyond, _) | (_, Ideal::Beyond) => Ideal::Beyond,
        (Ideal::Fin(x), Ideal::Fin(y)) => match x.checked_mul(y) {
            Some(product) => Ideal::Fin(product),
            None => Ideal::Beyond,
        },
    }
}

/// Ideal join. Associative and commutative.
#[must_use]
pub fn ideal_join(a: Ideal, b: Ideal) -> Ideal {
    match (a, b) {
        (Ideal::Omega, _) | (_, Ideal::Omega) => Ideal::Omega,
        (Ideal::Beyond, _) | (_, Ideal::Beyond) => Ideal::Beyond,
        (Ideal::Fin(x), Ideal::Fin(y)) => Ideal::Fin(x.max(y)),
    }
}

/// `base ^ exponent` in the ideal domain.
#[must_use]
pub fn ideal_pow(base: u32, exponent: Ideal) -> Ideal {
    match exponent {
        Ideal::Omega => Ideal::Omega,
        Ideal::Beyond => Ideal::Beyond,
        Ideal::Fin(e) => {
            // `base >= 2`, so `base^64 > u64::MAX` - finite, but Beyond.
            if e >= REFERENCE_MAX_FINITE_EXPONENT {
                return Ideal::Beyond;
            }
            match u32::try_from(e)
                .ok()
                .and_then(|narrowed| u64::from(base).checked_pow(narrowed))
            {
                Some(value) => Ideal::Fin(value),
                None => Ideal::Beyond,
            }
        }
    }
}

/// `ceil(log_base(max(1, argument)))` in the ideal domain.
#[must_use]
pub fn ideal_ceil_log(base: u32, argument: Ideal) -> Ideal {
    match argument {
        Ideal::Omega => Ideal::Omega,
        Ideal::Beyond => Ideal::Fin(ceil_log_beyond(base)),
        Ideal::Fin(v) => Ideal::Fin(ceil_log_u64(base, v)),
    }
}

// ---------------------------------------------------------------------------
// the upper bound - the cap on how loose the constructor may be
// ---------------------------------------------------------------------------

/// Substitutes `replacement` for every occurrence of the `index`th variable.
///
/// Recipe-level substitution, so that the *composed* denotation can be
/// computed in the ideal domain without routing an intermediate value through
/// a saturating `Ref`. That routing is what makes a rebinding floor wrong: at
/// `x := Beyond` a `Prod` carrying a zero would read `omega` and inflate.
#[must_use]
pub fn subst_spec(spec: &BoundSpec, index: usize, replacement: &BoundSpec) -> BoundSpec {
    let slot = index % VAR_NAMES.len();
    match spec {
        BoundSpec::Const(_) => spec.clone(),
        BoundSpec::Var(i) => {
            if i % VAR_NAMES.len() == slot {
                replacement.clone()
            } else {
                spec.clone()
            }
        }
        BoundSpec::Sum(xs) => BoundSpec::Sum(
            xs.iter()
                .map(|x| subst_spec(x, index, replacement))
                .collect(),
        ),
        BoundSpec::Max(xs) => BoundSpec::Max(
            xs.iter()
                .map(|x| subst_spec(x, index, replacement))
                .collect(),
        ),
        BoundSpec::Prod(xs) => BoundSpec::Prod(
            xs.iter()
                .map(|x| subst_spec(x, index, replacement))
                .collect(),
        ),
        BoundSpec::Trans { log, base, arg } => BoundSpec::Trans {
            log: *log,
            base: *base,
            arg: Box::new(subst_spec(arg, index, replacement)),
        },
    }
}

/// Rewrites `spec` so that no `Prod` has a `Prod` child.
///
/// Flattening is the direction `Bound::prod` already moves in, and it only
/// ever *loses* tightness: merging two factor groups can make the non-zero
/// product overflow where neither group did, and it can never rescue a zero
/// that was already there. So the flattened form of a recipe is an upper
/// bound on the recipe itself - which is the one comparison between two
/// *terms* that does not need to model the constructor's grouping.
#[must_use]
pub fn flatten_prods(spec: &BoundSpec) -> BoundSpec {
    match spec {
        BoundSpec::Const(_) | BoundSpec::Var(_) => spec.clone(),
        BoundSpec::Sum(xs) => BoundSpec::Sum(xs.iter().map(flatten_prods).collect()),
        BoundSpec::Max(xs) => BoundSpec::Max(xs.iter().map(flatten_prods).collect()),
        BoundSpec::Prod(_) => {
            let mut flat = Vec::new();
            gather_flat_prod(spec, &mut flat);
            BoundSpec::Prod(flat)
        }
        BoundSpec::Trans { log, base, arg } => BoundSpec::Trans {
            log: *log,
            base: *base,
            arg: Box::new(flatten_prods(arg)),
        },
    }
}

fn gather_flat_prod(spec: &BoundSpec, out: &mut Vec<BoundSpec>) {
    match spec {
        BoundSpec::Prod(xs) => {
            for x in xs {
                gather_flat_prod(x, out);
            }
        }
        other => out.push(flatten_prods(other)),
    }
}

/// Characterises the implementation at one point, with **no vacuous arm**.
///
/// An earlier revision checked only `exact <= observed`, whose `omega` case
/// (`observed_dominates` returns `true` for any `(None, _)`) discharged 71.6%
/// of generated cases without ever comparing two numbers - and on the 28.4%
/// that did compare, the implementation was exact every single time. A
/// headline soundness property that is three-quarters tautology is not a
/// soundness property, so both halves are now real assertions:
///
/// * a **finite** observation must equal the true denotation exactly;
/// * an **`omega`** observation must be arithmetically justified - some
///   grouping of the term has to be able to leave `u64`.
///
/// Measured over 30 000 draws from [`arb_spec`] and [`arb_env`]:
///
/// ```text
/// finite observation  -> strict equality asserted   28.5%
/// omega observation   -> justification asserted     71.5%
/// vacuous                                            0.0%
/// of which: envelope finite -> omega forbidden      27.0%
///           omega with a finite true value           3.0%   <- the open region
/// ```
///
/// # The 3% that is deliberately not pinned
///
/// That last row is `Bound::prod` reporting `omega` for a term whose true
/// value is finite, in the regime where saturation really was reachable -
/// a product that overflows while carrying a zero factor. **Both answers are
/// admissible there and they differ by `omega` versus `0`, the full width of
/// the lattice**, so no upper bound can permit the current implementation
/// without permitting anything at all. It is an accepted risk with a precise
/// trigger: the moment the tightness question is decided one way or the
/// other, this region becomes pinnable and should be pinned.
///
/// # The only valid floor is the exact denotation
///
/// **Never use one term's evaluated value as a lower bound for another
/// term's.** `Bound::eval` is not compositional: `Bound::prod` flattens and
/// regroups, saturating multiplication is not associative, so every term's
/// value carries whatever over-approximation its own grouping accumulated.
/// Using that as a floor pins the implementation's *looseness* as a contract
/// and forbids it from ever becoming more precise - the opposite of what a
/// soundness suite is for.
///
/// This suite made that mistake three times before it was named: a flattened
/// upper bound over recipes (wrong - arity-1 collapse), an
/// overflow-dominant floor under `Bound::prod` (blocked two correct tightness
/// fixes), and two substitution floors that read an intermediate `omega` as a
/// requirement. All three were *descriptions of the implementation* promoted
/// to *requirements on it*.
///
/// `exact_ideal <= observed` is necessary **and sufficient** for soundness.
/// Anything stronger over-constrains; anything weaker is unsound. Floors are
/// computed from [`naive_eval_ideal`] of a *recipe*, never from `eval` of a
/// term.
///
/// # Why there is no closed-form upper bound here
///
/// An earlier revision paired this with a cap: the value the recipe would have
/// if every `Prod` chain were flattened. **That cap is wrong**, and a 20 000
/// case run produced the witness:
///
/// ```text
/// Prod[ Sum[3, 0], Max[0, Prod[Var(x2), 6148914691236517206]] ]   at x2 = 0
/// ```
///
/// `Max[Const(0), P]` drops the zero, collapses to arity one, and *becomes*
/// `P` - so the parent `Prod` flattens `P`'s factors into its own and
/// `3 * 6148914691236517206` overflows to `omega`, while a model that
/// flattens only through `Prod` children predicts `0`.
///
/// Any arity-1 collapse can turn a `Sum` or `Max` into a `Prod`, so predicting
/// which factors end up multiplied together means reimplementing the
/// constructor - at which point the reference stops being independent and the
/// property stops being a test. The sound cap is therefore
/// `omega` in the saturating regime, and *exactness* outside it, which is
/// pinned by `denotation::denotation_is_exact_when_nothing_saturates` on a
/// generator where no grouping can overflow at all. That is a tighter
/// constraint than the flattened cap ever was, and unlike it, it is correct.
#[must_use]
pub fn precision_violation(spec: &BoundSpec, env: &Env, observed: Ref) -> Option<String> {
    let exact = naive_eval_ideal(spec, env);
    match observed {
        // A finite observation must be exactly right. Saturation is the only
        // imprecision the algebra sanctions, and saturation lands on `omega` -
        // so there is no route to a *loose but finite* answer, and anything
        // that produced one would be a truncation bug.
        Some(got) => match exact {
            Ideal::Fin(want) if want == got => None,
            Ideal::Fin(want) => Some(format!(
                "LOOSE BUT FINITE: evaluated to {got}, true denotation {want}"
            )),
            Ideal::Beyond | Ideal::Omega => Some(format!(
                "UNDER-APPROXIMATION: evaluated to a finite {got}, \
                 below the true denotation {exact:?}"
            )),
        },
        // `omega` must be *arithmetically justified*: some grouping of this
        // term has to be able to leave `u64`. If none can, `omega` is
        // gratuitous looseness rather than saturation.
        None => saturation_free_envelope(spec, env).map(|ceiling| {
            format!(
                "GRATUITOUS OMEGA: no grouping of this term can leave u64 - the largest \
                 magnitude any of them reaches is {ceiling} - so omega is not saturation; \
                 the true denotation is {exact:?}"
            )
        }),
    }
}

/// The largest magnitude **any** grouping of `spec` could reach at `env`, or
/// `None` if some subterm could leave `u64`.
///
/// A zero factor counts as one: a zero cannot rescue an overflow that a
/// different grouping would have hit first, which is exactly the asymmetry
/// that makes `Bound::prod` grouping dependent.
///
/// `None` therefore means "saturation is reachable here", and it is the only
/// circumstance under which an implementation may answer `omega` for a term
/// whose true denotation is finite.
#[must_use]
pub fn saturation_free_envelope(spec: &BoundSpec, env: &Env) -> Option<u64> {
    match spec {
        BoundSpec::Const(r) => *r,
        BoundSpec::Var(i) => env.value_of(*i),
        BoundSpec::Sum(xs) => xs.iter().try_fold(0u64, |acc, x| {
            acc.checked_add(saturation_free_envelope(x, env)?)
        }),
        BoundSpec::Max(xs) => xs.iter().try_fold(0u64, |acc, x| {
            Some(acc.max(saturation_free_envelope(x, env)?))
        }),
        BoundSpec::Prod(xs) => xs.iter().try_fold(1u64, |acc, x| {
            acc.checked_mul(saturation_free_envelope(x, env)?.max(1))
        }),
        BoundSpec::Trans { log, base, arg } => {
            let inner = saturation_free_envelope(arg, env)?;
            if *log {
                Some(ceil_log_u64(*base, inner))
            } else if inner >= REFERENCE_MAX_FINITE_EXPONENT {
                None
            } else {
                u64::from(*base).checked_pow(u32::try_from(inner).ok()?)
            }
        }
    }
}

/// `true` iff an observed magnitude soundly over-approximates an ideal one.
///
/// `omega` dominates everything. A finite observation must be at least the
/// ideal value, and may never stand in for a `Beyond` or an `Omega`.
#[must_use]
pub fn observed_dominates(observed: Ref, exact: Ideal) -> bool {
    match (observed, exact) {
        (None, _) => true,
        (Some(_), Ideal::Beyond | Ideal::Omega) => false,
        (Some(got), Ideal::Fin(want)) => got >= want,
    }
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
    ideal_ref(naive_eval_ideal(spec, env))
}

/// The naive interpretation in the ideal domain, before the single saturation
/// at the observation point.
///
/// Every fold here is over an associative, commutative operation, so the value
/// of an n-ary node is a function of its operand **multiset** - which is what
/// `Sum`, `Max` and `Prod` require, since all three hold their operands in
/// canonical order and have therefore already forgotten the order they were
/// supplied in. See [`Ideal`] for why a left fold with `checked_mul` is not
/// such a function.
#[must_use]
pub fn naive_eval_ideal(spec: &BoundSpec, env: &Env) -> Ideal {
    match spec {
        BoundSpec::Const(r) => ideal_of(*r),
        BoundSpec::Var(i) => ideal_of(env.value_of(*i)),
        BoundSpec::Sum(xs) => xs
            .iter()
            .map(|x| naive_eval_ideal(x, env))
            .fold(Ideal::Fin(0), ideal_plus),
        BoundSpec::Max(xs) => xs
            .iter()
            .map(|x| naive_eval_ideal(x, env))
            .fold(Ideal::Fin(0), ideal_join),
        BoundSpec::Prod(xs) => xs
            .iter()
            .map(|x| naive_eval_ideal(x, env))
            .fold(Ideal::Fin(1), ideal_times),
        BoundSpec::Trans { log, base, arg } => {
            let inner = naive_eval_ideal(arg, env);
            if *log {
                ideal_ceil_log(*base, inner)
            } else {
                ideal_pow(*base, inner)
            }
        }
    }
}

/// `true` for the shapes that actually take sub-bounds as operands.
///
/// `Const` and `Sum`'s nullary siblings are not a stylistic distinction: a
/// property about behaviour "in its operands" is vacuous for a constructor
/// that has none, and iterating all six shapes to check it silently compares
/// two identical terms twice. Exhaustive, so a seventh constructor is a
/// compile error here as well.
#[must_use]
pub const fn shape_takes_operands(shape: BoundShape) -> bool {
    match shape {
        BoundShape::Const | BoundShape::Var => false,
        BoundShape::Sum | BoundShape::Max | BoundShape::Prod | BoundShape::Trans => true,
    }
}

/// Builds a recipe of the requested shape.
///
/// **`Const` and `Var` ignore `a` and `b`** - they are nullary constructors
/// and there is nowhere for an operand to go. Callers asserting anything
/// *about the operands* must filter with [`shape_takes_operands`] first.
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

/// Magnitudes small enough that products of a few of them stay inside `u64`.
///
/// [`arb_ref`] deliberately carries a heavy tail - `u64::MAX`, `1 << 63`,
/// uniform `u64` - because saturation is where the interesting bugs live. The
/// cost is that a `Prod` drawn from it almost always overflows, so its value
/// is grouping dependent and only the soundness direction can be asserted.
/// Measured, a `Prod`-bearing recipe from [`arb_spec`] avoided overflow only
/// 15% of the time.
///
/// This generator covers the other regime, where saturation is impossible and
/// the constructor must therefore be **exactly** denotation preserving.
///
/// Impossible by arithmetic, not by hope: magnitudes are at most `4`, the
/// recursion is at most 3 deep and at most 3 wide, so a term has at most
/// `3^3 = 27` leaves and the largest value any grouping can reach is
/// `4^27 = 2^54`, three orders of magnitude below `u64::MAX`. No regrouping
/// can overflow, so every grouping agrees and the answer is grouping
/// independent. `Pow` is excluded for the same reason - it reaches `2^64`
/// from a single small operand.
///
/// Both generators are needed; neither replaces the other.
pub fn arb_small_ref() -> impl Strategy<Value = Ref> {
    prop_oneof![
        8 => (0u64..5).prop_map(Some),
        3 => prop_oneof![Just(Some(0u64)), Just(Some(1u64)), Just(Some(2u64))],
        1 => Just(REF_OMEGA),
    ]
}

/// Terms whose literals and bases are small enough that nothing saturates.
pub fn arb_small_spec() -> impl Strategy<Value = BoundSpec> {
    let leaf = prop_oneof![
        3 => arb_small_ref().prop_map(BoundSpec::Const),
        3 => (0usize..VAR_NAMES.len()).prop_map(BoundSpec::Var),
    ];
    leaf.prop_recursive(3, 16, 3, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 2..4).prop_map(BoundSpec::Sum),
            proptest::collection::vec(inner.clone(), 2..4).prop_map(BoundSpec::Max),
            proptest::collection::vec(inner.clone(), 2..4).prop_map(BoundSpec::Prod),
            // `Log` only: `Pow` re-introduces saturation immediately, and a
            // saturating `Pow` is already covered by `arb_spec`.
            inner.prop_map(|arg| BoundSpec::Trans {
                log: true,
                base: 2,
                arg: Box::new(arg),
            }),
        ]
    })
}

/// A valuation whose magnitudes are small enough not to saturate.
pub fn arb_small_env() -> impl Strategy<Value = Env> {
    (proptest::array::uniform3(arb_small_ref()), arb_small_ref())
        .prop_map(|(vals, default)| Env { vals, default })
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
