//! What a run is entitled to say about one function's cost.
//!
//! # Why this is a decision and not a call
//!
//! [`landav_engine::cost`] is total over the structured fragment, so it has an
//! answer for every function the frontend can translate - including the ones
//! [`landav_its::lower`] refuses. That is the whole of `LAN-87`: a call is the
//! sole construct blocking most of the corpus, and "analysed apart from the
//! call at line 12" is worth reporting where "no bound" was not.
//!
//! But it opens a gap between two claims, and the gap has to be closed
//! deliberately in one place rather than twice, differently, in the text
//! renderer and the JSON collector.

use landav_engine::TripCount;
use landav_python::LoweredFunction;

/// The cost this run may report for `function`, given whether it lowered.
///
/// # The divergence invariant
///
/// A function that did **not** lower may never be reported with a complete
/// bound - `Theta` or `O`, `"exact"` or `"upper"`. Those are two-sided and
/// one-sided *claims*, and a consumer is entitled to compare either against a
/// budget. A function analysed apart from a named region has made neither: its
/// bound carries an unfilled hole, an unfilled hole denotes `omega`, and the
/// honest label is `"partial"`.
///
/// Usually that falls out on its own - a program that did not lower contains an
/// `Unsupported` node, and the engine charges every one of them as a hole. It
/// does not fall out in one case, and that case is the reason this function
/// exists: [`landav_its::Construct::PolynomialDegree`],
/// [`landav_its::Construct::PolynomialSize`] and
/// [`landav_its::Construct::ArithmeticOverflow`] are raised by the **lowering**,
/// from limits on the representation it emits into, and leave no node in the
/// arena for the engine to see. `x = (a + b + c) ** 8` costs exactly one step
/// and does not lower.
///
/// The engine is not wrong there. But the run holds two answers that disagree
/// about whether this function was analysed, and resolving that in favour of
/// the stronger one is how an over-claim ships. So the complete bound is
/// withheld, and the refusal - which the run does report, with its construct
/// and position - stands on its own.
#[must_use]
pub fn cost_of(function: &LoweredFunction, lowered: bool) -> Option<TripCount> {
    let derived = landav_engine::cost(function.program());
    if lowered {
        return Some(derived);
    }
    // See above: a complete claim about a function the toolchain refused is the
    // one shape this run will not make.
    (!derived.is_complete()).then_some(derived)
}
