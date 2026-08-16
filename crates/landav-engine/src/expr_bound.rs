//! Reading a source expression as a [`Bound`], where that is possible at all.

use landav_bound::Bound;
use landav_its::{ArithOp, ExprId, SourceExpr, SourceProgram};

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
pub fn read(program: &SourceProgram, id: ExprId) -> Option<Bound> {
    match program.expr(id)? {
        SourceExpr::Int { value } => u64::try_from(*value).ok().map(Bound::constant),
        SourceExpr::Var { name } => Some(Bound::var(name.symbol().clone())),
        SourceExpr::Arith { op, left, right } => {
            let left = read(program, *left)?;
            let right = read(program, *right)?;
            match op {
                ArithOp::Add => Some(Bound::sum([left, right])),
                ArithOp::Mul => Some(Bound::prod([left, right])),
                // Non-monotone in its right operand. See the note above: there
                // is no sound bound to return, not merely no convenient one.
                ArithOp::Sub => None,
            }
        }
        // `-e` is `0 - e`, and decreasing.
        SourceExpr::Neg { .. } => None,
        SourceExpr::Pow { base, exponent } => {
            let base = read(program, *base)?;
            // `Bound::pow` raises a *constant* base to a bound exponent, which
            // is the opposite shape: this is `x^k` for literal `k`, so it is a
            // product of `k` copies. `k` is capped by the lowering's degree
            // limit, so the expansion is bounded.
            match exponent {
                0 => Some(Bound::one()),
                // `try_from` rather than `as`: the workspace denies truncating
                // casts because one of them once turned an enormous bound into
                // a small one, and a silently-shortened product would be
                // exactly that failure again.
                k => Some(Bound::prod(std::iter::repeat_n(
                    base,
                    usize::try_from(*k).ok()?,
                ))),
            }
        }
        SourceExpr::Unsupported { .. } => None,
    }
}
