//! Reading a source expression as a [`Bound`], where that is possible at all.

use std::collections::BTreeSet;

use landav_bound::Bound;
use landav_its::{ArithOp, ExprId, SourceExpr, SourceProgram, VarName};

/// A bound read off an expression, and whether it is that expression's value
/// or merely dominates it.
///
/// # Why the two cannot be collapsed
///
/// [`crate::TripCount::Exact`] is a two-sided claim: the loop runs that many
/// times, and the program achieves it. [`crate::TripCount::AtMost`] claims only
/// the upper half. `Walk::count_of` used to wrap whatever this module returned
/// in `Exact`, which was true only while every reading here was the value
/// itself.
///
/// It no longer is. `n // 2` has no representation in the bound algebra - there
/// is no division and no subtraction - so the only thing that can be returned
/// for it is `n`, which dominates it and is not equal to it. A loop counted by
/// that reading runs half as often as the number says, and labelling it `Theta`
/// is the `LAN-88` failure exactly: a complete two-sided claim the program does
/// not meet.
///
/// So the reading carries its own quality, and the caller has to look at it to
/// choose a variant.
#[derive(Debug, Clone)]
pub struct Reading {
    bound: Bound,
    exact: bool,
}

impl Reading {
    /// A reading that is the expression's value.
    #[must_use]
    pub const fn exact(bound: Bound) -> Self {
        Self { bound, exact: true }
    }

    /// A reading that only dominates the expression's value.
    #[must_use]
    pub const fn approximate(bound: Bound) -> Self {
        Self {
            bound,
            exact: false,
        }
    }

    /// Whether the bound *is* the value rather than an over-approximation.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        self.exact
    }

    /// The bound itself.
    #[must_use]
    pub const fn bound(&self) -> &Bound {
        &self.bound
    }

    /// The bound, consuming the reading.
    #[must_use]
    pub fn into_bound(self) -> Bound {
        self.bound
    }

    /// A reading built from two, exact only if both were.
    ///
    /// Sound because every [`Bound`] constructor is weakly monotone: if
    /// `a <= A` and `b <= B` then `a + b <= A + B` and `a * b <= A * B`, so
    /// combining over-approximations over-approximates.
    fn combined(left: Self, right: Self, join: impl FnOnce(Bound, Bound) -> Bound) -> Self {
        Self {
            bound: join(left.bound, right.bound),
            exact: left.exact && right.exact,
        }
    }

    /// The same bound, no longer claimed to be the value.
    fn relaxed(self) -> Self {
        Self {
            bound: self.bound,
            exact: false,
        }
    }
}

/// The expression as a bound over the function's parameters, or `None`.
///
/// # Why this is partial, and why the partiality is the interesting part
///
/// [`Bound`] is **weakly monotone by construction** - that guarantee is what
/// makes composition-by-substitution sound, and it is enforced by there being
/// no non-monotone constructor to reach for. Subtraction and negation are
/// exactly the operations that would break it, so they have no representation
/// and an expression using them has no bound.
///
/// That is not a limitation to be worked around. An expression like `n - m`
/// genuinely is not monotone in `m`, so any "bound" for it would be a lie in
/// one direction or the other. Returning `None` and letting the caller fall
/// back to an external solver is the honest outcome.
///
/// A negative literal is refused for the same reason from the other side:
/// bounds live in `N u {omega}` and there is no natural number to map it to.
/// Callers that want "a count, floored at zero" must say so themselves, which
/// is what [`crate::TripCount`] does.
///
/// # `readable` is a soundness argument, not a convenience
///
/// A [`Bound`] is written in the variables the **caller supplies on entry**.
/// A variable read denotes the variable's value *at the point of the read*.
/// Those two are the same number only when nothing has assigned to the name in
/// between, and this function has no way to know that on its own - so the
/// caller passes the set of names for which it holds, and every other name has
/// no bound.
///
/// Without it, `n = n * n` followed by `for i in range(n)` reports `1 + 2n` for
/// a loop that runs `n^2` times, marked exact and carrying no holes: a complete
/// claim a budget gate may act on, which the program exceeds. And
/// `m = n * n; for i in range(m)` reports a bound over `m`, which is a local -
/// the caller has nothing to supply for it, so it evaluates to zero and a
/// quadratic cost reads as a constant.
///
/// Returning `None` costs precision only. The caller turns it into a named
/// region, so the loop is reported as unanalysed rather than as analysed wrong.
pub fn read(program: &SourceProgram, id: ExprId, readable: &BTreeSet<VarName>) -> Option<Reading> {
    match program.expr(id)? {
        SourceExpr::Int { value } => u64::try_from(*value)
            .ok()
            .map(|value| Reading::exact(Bound::constant(value))),
        // A read denotes the variable's value **at this point**, and
        // `Bound::var` denotes the value the caller supplied on entry. Those
        // are the same number only for a name `readable` vouches for; see the
        // doc comment above for what happens when they are not.
        SourceExpr::Var { name } => readable
            .contains(name)
            .then(|| Reading::exact(Bound::var(name.symbol().clone()))),
        SourceExpr::Arith { op, left, right } => {
            let left = read(program, *left, readable)?;
            let right = read(program, *right, readable)?;
            match op {
                ArithOp::Add => Some(Reading::combined(left, right, |left, right| {
                    Bound::sum([left, right])
                })),
                ArithOp::Mul => Some(Reading::combined(left, right, |left, right| {
                    Bound::prod([left, right])
                })),
                // Non-monotone in its right operand. See the note above: there
                // is no sound bound to return, not merely no convenient one.
                ArithOp::Sub => None,
            }
        }
        // `-e` is `0 - e`, and decreasing.
        SourceExpr::Neg { .. } => None,
        SourceExpr::Pow { base, exponent } => {
            let base = read(program, *base, readable)?;
            // `Bound::pow` raises a *constant* base to a bound exponent, which
            // is the opposite shape: this is `x^k` for literal `k`, so it is a
            // product of `k` copies. `k` is capped by the lowering's degree
            // limit, so the expansion is bounded.
            match exponent {
                0 => Some(Reading::exact(Bound::one())),
                // `try_from` rather than `as`: the workspace denies truncating
                // casts because one of them once turned an enormous bound into
                // a small one, and a silently-shortened product would be
                // exactly that failure again.
                k => {
                    let copies = usize::try_from(*k).ok()?;
                    let exact = base.is_exact();
                    let product = Bound::prod(std::iter::repeat_n(base.into_bound(), copies));
                    Some(if exact {
                        Reading::exact(product)
                    } else {
                        Reading::approximate(product)
                    })
                }
            }
        }
        // A refusal that named an expression dominating its own magnitude: the
        // value is not known, its size is. Whatever the named expression reads
        // as, this reads as **approximately**, because the inequality it rests
        // on is an inequality. See [`SourceExpr::Unsupported`].
        SourceExpr::Unsupported { bounded_by, .. } => {
            let bounded_by = (*bounded_by)?;
            Some(read(program, bounded_by, readable)?.relaxed())
        }
    }
}
