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
    ///
    /// # A refusal is not the end of what can be said
    ///
    /// That is the *lowering's* answer, and it is the only sound one on that
    /// path. A consumer reading this program directly rather than through a
    /// transition system has a second option: charge the node as a named region
    /// of unknown cost and derive everything around it. `landav-engine` does
    /// exactly that, which is why [`crate::SourceProgram::unsupported_nodes`]
    /// is published - such a consumer has to be able to check that it accounted
    /// for every one of these, and a second walk of its own would be a second
    /// thing to keep correct.
    ///
    /// # Position matters to that consumer and not to this one
    ///
    /// The lowering scans, so where a node hangs is irrelevant to it. A cost is
    /// charged where the node *is*: a region inside a loop body is paid once per
    /// iteration, and the same region attached to nothing at all cannot be
    /// placed and cannot be charged soundly. A frontend that translates an
    /// expression it will not keep should therefore surface the refusal at a
    /// position in the statement tree rather than leaving the node orphaned.
    ///
    /// # A refusal that still knows how big the value is
    ///
    /// `bounded_by` is the one thing a refusal may carry beyond its name. It
    /// names an expression whose **magnitude dominates this node's**, which is
    /// strictly less than knowing the value: `n // 2` has no representation in
    /// the polynomial fragment and never will, but `|n // 2| <= |n|` holds for
    /// every divisor, so `n` is a legitimate over-approximation of it.
    ///
    /// The two consumers read that differently, and both stay right.
    /// [`crate::lower`] refuses exactly as before - a transition system built
    /// from an over-approximation admits executions the program does not have,
    /// so there is no system to emit. `landav-engine`, which derives a *bound*
    /// rather than a system, may read the named expression and mark what it
    /// read **approximate**, so a loop counted by one is reported `O` and never
    /// `Theta`.
    ///
    /// It is `None` for every refusal that has no such expression, which is
    /// almost all of them: a call, an attribute, a comprehension have no
    /// operand whose size dominates their value.
    ///
    /// # The operand must be *referenced*, not merely translated
    ///
    /// Whatever this names is part of the program: a walk reaches it, charges
    /// the regions inside it, and accounts for them. A frontend that wants to
    /// bound a node by an operand it does not otherwise keep must therefore
    /// point at that operand here rather than translating and discarding it -
    /// an `Unsupported` node nothing points at is the orphan
    /// `SourceProgram::unsupported_nodes` exists to catch.
    Unsupported {
        /// What was refused.
        construct: Construct,
        /// Frontend-supplied specifics, if any.
        detail: Option<Symbol>,
        /// An expression whose magnitude dominates this one's, if the frontend
        /// knows one.
        bounded_by: Option<ExprId>,
    },
}
