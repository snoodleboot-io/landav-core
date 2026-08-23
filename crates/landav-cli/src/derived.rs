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

use std::collections::BTreeSet;

use landav_engine::TripCount;
use landav_its::Unsupported;
use landav_python::LoweredFunction;

/// The cost this run may report for `function`, given whether it lowered and
/// what the lowering refused.
///
/// # The divergence invariant
///
/// A function that did **not** lower may not be reported with a complete bound
/// (`Theta` or `O`, `"exact"` or `"upper"`) **unless the engine can be shown to
/// have seen everything the lowering refused**. Those labels are two-sided and
/// one-sided *claims*, and a consumer is entitled to compare either against a
/// budget. A function analysed apart from a named region has made neither: its
/// bound carries an unfilled hole, an unfilled hole denotes `omega`, and the
/// honest label is `"partial"`.
///
/// # Why `lowered` alone is the wrong test
///
/// It was the right test while every refusal was an `Unsupported` node the
/// engine charged as a hole, because then "did not lower" and "the engine is
/// missing something" coincided. `LAN-91` broke that coincidence in both
/// directions:
///
/// * [`landav_its::Construct::PolynomialDegree`],
///   [`landav_its::Construct::PolynomialSize`] and
///   [`landav_its::Construct::ArithmeticOverflow`] are raised by the **lowering**
///   from limits on the representation it emits into, and leave no node in the
///   arena. `x = (a + b + c) ** 8` costs exactly one step and does not lower.
///   The engine is not wrong there, but the run holds two answers that disagree
///   about whether the function was analysed, and resolving that in favour of
///   the stronger one is how an over-claim ships.
/// * A refusal carrying [`landav_its::SourceExpr::Unsupported`]'s `bounded_by` -
///   `n // 2`, a `set` display whose equal elements collapse - **is** in the
///   arena, and the engine reads the expression that dominates it and reports a
///   sound one-sided bound. Withholding that was pure loss: `range(n // 2)`
///   reported no bound at all where it had previously reported a partial one,
///   which is why the `integer-division` and `bitwise-operator` sole-blocker
///   counts did not move when those constructs were implemented.
///
/// So the test is not "did it lower" but "could the engine see what stopped it".
/// Every refusal the lowering recorded must correspond to a node in the
/// program's arenas; [`landav_engine`]'s own reconciliation then guarantees each
/// of those was charged.
///
/// # Why an empty refusal list still withholds
///
/// A lowering that failed without recording refusals failed for some other
/// reason - an arena overflow, a structural error - and there is nothing to
/// match against. Absence of evidence is not evidence here, so it withholds.
#[must_use]
pub fn cost_of(
    function: &LoweredFunction,
    lowered: bool,
    refusals: &[Unsupported],
) -> Option<TripCount> {
    let derived = landav_engine::cost(function.program());
    if lowered {
        return Some(derived);
    }
    // A partial result makes no finite claim, so there is nothing to over-claim
    // and it is always reported.
    if !derived.is_complete() {
        return Some(derived);
    }
    // Complete, but the toolchain refused the function. Report it only if every
    // refusal is something the engine had in front of it - either an
    // `Unsupported` node it charged as a hole, or a statement it models fully
    // and the lowering refuses anyway.
    let program = function.program();
    let visible: BTreeSet<Unsupported> = program
        .unsupported_nodes()
        .map(|node| node.refusal())
        .collect();
    let modelled: BTreeSet<_> = program.natively_modelled_refusals().collect();
    let accounted = !refusals.is_empty()
        && refusals
            .iter()
            .all(|refusal| visible.contains(refusal) || modelled.contains(refusal.origin()));
    accounted.then_some(derived)
}
