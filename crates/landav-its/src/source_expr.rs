//! [`SourceExpr`] - one node of the fragment's integer expression language.

use landav_bound::Symbol;

use crate::{arith_op::ArithOp, construct::Construct, expr_id::ExprId, var_name::VarName};

/// An integer-valued expression, as one arena node.
///
/// Children are [`ExprId`]s rather than boxes; see [`ExprId`] for why.
///
/// # Every variant denotes a polynomial - except one
///
/// [`SourceExpr::Int`], [`SourceExpr::Var`], [`SourceExpr::Arith`],
/// [`SourceExpr::Neg`] and [`SourceExpr::Pow`] are closed under the
/// polynomial semiring, so any expression built from them alone lowers to a
/// [`crate::Polynomial`] **exactly**: no rounding, no widening, no
/// approximation. That is a deliberate design property and not a coincidence -
/// the expression language was chosen to be exactly the fragment that lowers
/// without loss.
///
/// [`SourceExpr::Unsupported`] is the escape hatch, and it is the whole
/// mechanism behind `LAN-67` criterion 4.
///
/// # Why "unsupported" is a node rather than an absence
///
/// Criterion 4 asks that truncation be impossible *by construction*, and the
/// only way to get that is to make the failure a thing the frontend must
/// build rather than a step it may forget. A frontend that meets a construct
/// it cannot translate has exactly two options here: build an `Unsupported`
/// node, or fail to produce a program at all. It cannot produce a program that
/// is quietly missing the construct, because there is no expression-shaped
/// hole to leave - every operand position is an `ExprId` that must name a real
/// node.
///
/// This inverts the usual default. In a translator whose fallback arm skips
/// what it does not recognise, silence is what you get for free and a
/// diagnostic is what you have to remember; here the catch-all arm of a
/// frontend's `match` produces a refusal, so *not thinking about a construct*
/// yields a loud refusal rather than a quiet unsound bound.
/// # Deliberately **not** `#[non_exhaustive]`
///
/// The rest of this crate's public vocabulary — [`Construct`],
/// [`crate::LoweringError`] — is `#[non_exhaustive]`, and this is not. The
/// difference is which way the information flows. Those are *produced* here
/// and matched elsewhere, so a new variant must not break a consumer. This is
/// the input language: it is matched by every consumer that interprets a
/// program, and a new construct arriving in it is precisely the event that
/// must break them all loudly.
///
/// A wildcard arm in a consumer is how a construct gets silently mishandled,
/// which is the failure `LAN-67` criterion 4 exists to prevent. Making the
/// enum exhaustive turns that into a compile error at every site at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceExpr {
    /// An integer literal.
    Int {
        /// The value.
        value: i64,
    },
    /// A read of an integer variable.
    Var {
        /// The variable read.
        name: VarName,
    },
    /// A binary arithmetic operation.
    Arith {
        /// Which operator.
        op: ArithOp,
        /// The left operand.
        left: ExprId,
        /// The right operand.
        right: ExprId,
    },
    /// Arithmetic negation.
    Neg {
        /// The operand.
        operand: ExprId,
    },
    /// A power with a literal non-negative exponent.
    ///
    /// The exponent is a `u32` in the *type* rather than an [`ExprId`],
    /// because `x ** y` for a variable `y` is not a polynomial and there is no
    /// sound polynomial to lower it to. A frontend meeting one emits
    /// [`Construct::NonPolynomialPower`]. The lowering additionally refuses an
    /// exponent that would push the result past [`crate::MAX_DEGREE`].
    Pow {
        /// The base.
        base: ExprId,
        /// The literal exponent.
        exponent: u32,
    },
    /// A construct the frontend could not translate.
    ///
    /// Lowering any program containing one refuses, naming this construct and
    /// this node's origin. It never yields a transition, and it never yields
    /// *no* transition either - the whole lowering fails, because a partial
    /// integer transition system admits fewer executions than the program has
    /// and a bound derived from it can be exceeded.
    Unsupported {
        /// What was refused.
        construct: Construct,
        /// Frontend-supplied specifics, if any.
        detail: Option<Symbol>,
    },
}
