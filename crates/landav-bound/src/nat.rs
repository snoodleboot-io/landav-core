//! [`Nat`] - the scalar cost magnitude.

use crate::{base::Base, canonical::Canonical};

/// A cost magnitude: a natural number, or `omega`.
///
/// `omega` is the top of the bound lattice - "no finite bound was
/// established". It is a value, not a sentinel: `Nat::Omega` is distinct from
/// `Nat::Fin(u64::MAX)`.
///
/// The converse is **not** guaranteed and must not be assumed: a genuinely
/// enormous finite count saturates *to* `Omega`, so `Omega` means "at least
/// unbounded as far as we could tell", never "definitely infinite". Do not
/// quote `Omega` as licence to treat every unbounded result as blameworthy in
/// the same way.
///
/// # Why this type keeps `Ord` when [`crate::Bound`] does not
///
/// `Nat` is a *concrete* magnitude on a total order, and `a < b` on it means
/// exactly "a is a smaller cost". There is no symbolic content to misread, and
/// `Fin(_) < Omega` is the semantic order. [`crate::Bound`] is symbolic, where
/// `<` would read as domination, which this crate does not decide.
///
/// Declaration order is *not* relied on: [`Nat::magnitude_cmp`] is written
/// out, so reordering the variants cannot silently change the order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Nat {
    /// A finite count.
    Fin(u64),
    /// Unbounded - no finite bound was established.
    Omega,
}

impl Nat {
    /// `0`. The additive identity of the scalar arithmetic.
    pub const ZERO: Self = Self::Fin(0);
    /// `1`. The multiplicative identity of the scalar arithmetic.
    pub const ONE: Self = Self::Fin(1);
    /// The top of the lattice.
    pub const OMEGA: Self = Self::Omega;

    /// The largest exponent that can possibly stay finite for any
    /// [`Base`] `>= 2`, because `2^64 > u64::MAX`.
    ///
    /// [`Nat::exp_of`] tests against this **before** narrowing the exponent to
    /// the `u32` that `u64::checked_pow` takes. Narrowing first is the
    /// determinism blocker that made `pow(2, 2^32)` report `1`.
    pub const MAX_FINITE_EXPONENT: u64 = 64;

    /// `true` iff this is not `omega`.
    #[must_use]
    pub const fn is_finite(self) -> bool {
        matches!(self, Self::Fin(_))
    }

    /// The semantic magnitude order: `Fin(a) < Fin(b)` iff `a < b`, and
    /// `Fin(_) < Omega` for every `u64`.
    ///
    /// Written out rather than derived so that reordering the variants is not
    /// a silent semantic change.
    #[must_use]
    pub fn magnitude_cmp(self, other: Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;

        match (self, other) {
            (Self::Fin(a), Self::Fin(b)) => a.cmp(&b),
            (Self::Fin(_), Self::Omega) => Ordering::Less,
            (Self::Omega, Self::Fin(_)) => Ordering::Greater,
            (Self::Omega, Self::Omega) => Ordering::Equal,
        }
    }

    /// Addition. `x + omega == omega`.
    ///
    /// Implemented with `checked_add`, falling back to [`Nat::OMEGA`] on
    /// overflow. **Never `saturating_add`**: that saturates to `u64::MAX`,
    /// which is finite, so it would be accepted by [`crate::FiniteBound`] and
    /// published as `Proved` while under-reporting the truth.
    #[must_use]
    pub fn plus(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Fin(a), Self::Fin(b)) => match a.checked_add(b) {
                Some(sum) => Self::Fin(sum),
                // Saturate to the top of the lattice, never to `u64::MAX`.
                None => Self::OMEGA,
            },
            _ => Self::OMEGA,
        }
    }

    /// Multiplication. **`omega` absorbs unconditionally, including against
    /// zero: `0 * omega == omega`.**
    ///
    /// This is the opposite of the measure-theoretic `0 * inf = 0` convention,
    /// and it is deliberate. `0 * omega = 0` was only ever forced by the
    /// annihilation law when the additive identity was `Const(0)`. The
    /// annihilator is now [`crate::Lifted::Bottom`], so the law no longer
    /// forces it, and the tight rule was actively harmful:
    ///
    /// * variables range over `N u {omega}`, so `0 * x` is `omega` at
    ///   `x = omega` and the tight rule made `Bound::prod` non
    ///   denotation-preserving;
    /// * it laundered an unblamed `omega` into a finite `0`, which
    ///   [`crate::Verdict`] then published as `Proved` with no blame;
    /// * it made `?a * omega -> omega` unsound as an e-graph congruence.
    ///
    /// The cost is tightness: a loop with a *proved* zero trip count and an
    /// unanalysed body reports `omega`. That knowledge belongs to the caller
    /// holding the zero-trip proof, which should elide the `times` node
    /// entirely rather than rely on a syntactic accident.
    ///
    /// Implemented with `checked_mul`, falling back to [`Nat::OMEGA`].
    /// **Never `saturating_mul`.**
    #[must_use]
    pub fn times(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Fin(a), Self::Fin(b)) => match a.checked_mul(b) {
                Some(product) => Self::Fin(product),
                None => Self::OMEGA,
            },
            // `omega` absorbs unconditionally, including `0 * omega`.
            _ => Self::OMEGA,
        }
    }

    /// Lattice join. `max(x, omega) == omega`.
    #[must_use]
    pub fn join(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Fin(a), Self::Fin(b)) => Self::Fin(a.max(b)),
            _ => Self::OMEGA,
        }
    }

    /// `base ^ self`. `base ^ omega == omega`; `base ^ 0 == 1`.
    ///
    /// Total and O(1)-bounded: the implementation **must** return
    /// [`Nat::OMEGA`] for any exponent `>= `[`Nat::MAX_FINITE_EXPONENT`]
    /// before narrowing to `u32`, then use `checked_pow`. Repeated
    /// multiplication without that short circuit does not terminate for large
    /// exponents, which satisfies "never panic" only vacuously.
    #[must_use]
    pub fn exp_of(self, base: Base) -> Self {
        let Self::Fin(exponent) = self else {
            return Self::OMEGA;
        };
        // Tested **before** narrowing: `4294967296u64 as u32` is `0`, which
        // made `pow(2, 2^32)` report `1`.
        if exponent >= Self::MAX_FINITE_EXPONENT {
            return Self::OMEGA;
        }
        let Ok(narrowed) = u32::try_from(exponent) else {
            return Self::OMEGA;
        };
        match u64::from(base.get()).checked_pow(narrowed) {
            Some(value) => Self::Fin(value),
            None => Self::OMEGA,
        }
    }

    /// `ceil(log_base(max(1, self)))`.
    ///
    /// Natural valued and total by construction: `max(1, .)` removes the
    /// `log(0)` pole and [`Base`] removes the `base < 2` pole. Computed by
    /// integer multiplication only - **no floating point anywhere in this
    /// crate** - so the result is bit-identical on every target.
    ///
    /// `log_base(omega) == omega`. For every finite argument the result is at
    /// most [`Nat::MAX_FINITE_EXPONENT`], so a finite input never yields
    /// `Omega`.
    ///
    /// The algorithm is: accumulate `base^i` until it reaches or exceeds
    /// `max(1, self)`, return `i`, saturating the accumulator at `omega`.
    /// `u64::ilog` is **not** used: the floor variant under-reports every
    /// non-power of the base (an unsound bound), and the `(n-1).ilog(k) + 1`
    /// variant panics on receiver `0`, which is reachable from both `b = 0`
    /// and `b = 1`.
    #[must_use]
    pub fn ceil_log(self, base: Base) -> Self {
        let Self::Fin(value) = self else {
            return Self::OMEGA;
        };
        // `max(1, .)` removes the `log(0)` pole; `Base >= 2` removes the
        // `base < 2` pole. Integer multiplication only - no floating point,
        // and no `ilog`, whose floor variant under-reports.
        let target = value.max(1);
        let k = u64::from(base.get());
        let mut reached: u64 = 1;
        let mut exponent: u64 = 0;
        while reached < target {
            exponent = exponent.saturating_add(1);
            match reached.checked_mul(k) {
                Some(next) => reached = next,
                // `base^exponent` exceeded `u64::MAX`, so it certainly
                // reached the target. Never under-report.
                None => return Self::Fin(exponent),
            }
        }
        Self::Fin(exponent)
    }
}

/// `Nat` keeps `Ord`, unlike [`crate::Bound`], because it is a concrete
/// magnitude on a total order where `<` means exactly "is a smaller cost".
///
/// Hand written rather than derived, delegating to [`Nat::magnitude_cmp`], so
/// that reordering the variants cannot silently change the order - the same
/// discipline [`crate::BoundShape::canonical_tag`] applies to the algebra.
impl Ord for Nat {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.magnitude_cmp(*other)
    }
}

impl PartialOrd for Nat {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Canonical for Nat {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.magnitude_cmp(*other)
    }

    fn write_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Self::Fin(value) => {
                out.push(0);
                out.extend_from_slice(&value.to_be_bytes());
            }
            Self::Omega => out.push(1),
        }
    }
}
