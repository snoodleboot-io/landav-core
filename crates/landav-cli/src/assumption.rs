//! LAN-14: the assumption a partial bound could not discharge.
//!
//! # Why this exists, and why the seam was already here
//!
//! `CONTRIBUTING.md`'s third non-negotiable asks a partial result for **two**
//! things: name the unaccounted **term**, and name the **assumption that could
//! not be discharged**. The first has been done since LAN-87 - every hole
//! carries its construct and its position, and the bound mentions its variable.
//! The second was built and left unwired: [`landav_bound::Assumption`] and
//! [`landav_bound::Blame`] have existed since F-015, and the word "assumption"
//! appeared nowhere in either renderer.
//!
//! This module is the join. It turns one [`landav_engine::Hole`] into the
//! obligation that hole represents, so both renderers say the same thing about
//! the same region.
//!
//! # Why a hole cannot simply call [`landav_its::Unsupported::blame`]
//!
//! It usually can, and where it can the answer would be
//! [`Assumption::ResourceNotModelled`] - `blame` says so and argues for it: a
//! refused construct is precisely one the cost model has no rule for.
//!
//! The `while` loop is the case that does not fit, and it is the most common
//! hole on the corpus: 186 occurrences and **zero** refusal records. A `while`
//! *lowers*. It becomes transitions, `landav-its` never builds an `Unsupported`
//! node for it, and there is consequently nothing to call `blame()` on. The
//! hole is raised by the **engine**, which read the loop and found it had no
//! ranking argument for it.
//!
//! That difference is exactly the distinction `Unsupported::blame` draws and
//! then declines, correctly, to make on its own behalf: it says the assumption
//! is deliberately *not* [`Assumption::TerminationNotProved`], "which is a
//! stronger and different claim - the loop may well terminate, we simply
//! declined to look". For a refusal that is right; the lowering did decline to
//! look. For an engine-side `while` hole it is not: the engine *did* look, the
//! walk reached the loop, and what it lacks is a ranking function. Naming that
//! `ResourceNotModelled` would blame a missing cost rule for a missing
//! termination proof and send a reader to the wrong file.
//!
//! So the mapping is keyed on the hole's construct rather than on a refusal
//! record, which also means every hole gets an assumption whether or not a
//! refusal exists beside it.
//!
//! # What is not distinguishable here, said plainly
//!
//! **Self-recursion is**: the callee the frontend recorded is compared against
//! the enclosing function's own name, and a match is
//! [`Assumption::RecursionNotRanked`] rather than a callee whose cost happens to
//! be unknown. **Mutual recursion is not.** Deciding that `f` calls `g` calls
//! `f` needs a call graph across functions, and this crate sees one function at
//! a time; such a cycle is reported as [`Assumption::CalleeCostUnknown`], which
//! is true but weaker than the truth. Nothing here pretends otherwise.

use landav_bound::{Assumption, Symbol};
use landav_engine::Hole;
use landav_its::Unsupported;

/// The name rendered for a variant this build does not know about.
///
/// [`Assumption`] is `#[non_exhaustive]`, so a `match` over it needs a wildcard
/// arm and a variant added upstream would land here. Deliberately ugly: a
/// nameless assumption in a report is a defect somebody should see, and it must
/// not read like a real category.
const UNNAMED: &str = "assumption-not-named-by-this-build";

/// The obligation `hole` stands for.
///
/// `refusals` is this function's refusal ledger, consulted only for the
/// specifics a hole does not carry - which callee, which construct description.
/// The *choice* of assumption is made from the hole, so a region the lowering
/// never refused still gets one; see the module documentation for why the
/// `while` loop makes that mandatory rather than tidy.
///
/// `enclosing` is the name of the function being analysed, which is what makes
/// self-recursion distinguishable from any other callee.
#[must_use]
pub fn undischarged(hole: &Hole, refusals: &[Unsupported], enclosing: &str) -> Assumption {
    // Matched on position and construct, which is the only join available: a
    // hole carries where it is and what it was, and the refusal ledger carries
    // the specifics the hole does not - which callee, which operator. Where
    // there is no matching record the assumption is still decided; it is the
    // *specifics* that are optional, never the obligation.
    let specifics = refusals.iter().find(|refusal| {
        refusal.origin().as_str() == hole.origin().as_str()
            && refusal.construct().tag() == hole.construct()
    });
    match hole.construct() {
        // The engine read this loop and has no ranking argument for it. See the
        // module documentation: this is the one place the refusal vocabulary's
        // answer would be wrong.
        "while" => Assumption::TerminationNotProved,
        "call" => {
            let callee = specifics
                .and_then(|refusal| refusal.detail().map(|detail| detail.as_str().to_owned()))
                // The frontend records the callee for every call it refuses, so
                // this is defensive rather than expected. Falling back to the
                // construct keeps the record honest - it says a call, and does
                // not invent a name for it.
                .unwrap_or_else(|| hole.construct().to_owned());
            if callee == enclosing {
                // `f` calls `f`. The obstacle is not that some other function's
                // cost is unknown; it is that this one recurses and nothing
                // ranked it.
                Assumption::RecursionNotRanked
            } else {
                Assumption::CalleeCostUnknown {
                    callee: Symbol::from(callee),
                }
            }
        }
        // Everything else is a construct the cost model has no rule for, which
        // is what `Unsupported::blame` says and for the reason it gives.
        construct => Assumption::ResourceNotModelled {
            detail: Symbol::from(
                specifics.map_or(construct, |refusal| refusal.construct().describe()),
            ),
        },
    }
}

/// The stable name of an assumption, for machine output.
///
/// A name, never a code: the same rule `machine.rs` applies to constructs, and
/// for the same reason - an agent's transcript is read by a person, and
/// `termination-not-proved` survives that reading where `A-02` does not.
#[must_use]
pub fn name(assumption: &Assumption) -> &'static str {
    match assumption {
        Assumption::TerminationNotProved => "termination-not-proved",
        Assumption::SizeNotBounded { .. } => "size-not-bounded",
        Assumption::CalleeCostUnknown { .. } => "callee-cost-unknown",
        Assumption::RecursionNotRanked => "recursion-not-ranked",
        Assumption::ExpressionDepthExceeded => "expression-depth-exceeded",
        Assumption::ResourceNotModelled { .. } => "resource-not-modelled",
        _ => UNNAMED,
    }
}

/// The subject of an assumption - the callee, the variable, the construct.
///
/// Separate from [`name`] so that a consumer can group by obligation without
/// parsing a sentence, which is the same argument `landav_its::Unsupported`
/// makes for keeping the construct a value and the free text beside it.
#[must_use]
pub fn subject(assumption: &Assumption) -> Option<String> {
    match assumption {
        Assumption::CalleeCostUnknown { callee } => Some(callee.as_str().to_owned()),
        Assumption::SizeNotBounded { var } => Some(var.symbol().as_str().to_owned()),
        Assumption::ResourceNotModelled { detail } => Some(detail.as_str().to_owned()),
        _ => None,
    }
}

/// The assumption as a clause a person reads, for the text report.
///
/// Phrased as what was *not* established, because that is the actionable half:
/// "the cost of `fetch` is not known here" tells a reader what to supply, where
/// "unbounded" tells them only that something went wrong.
#[must_use]
pub fn describe(assumption: &Assumption) -> String {
    match assumption {
        Assumption::TerminationNotProved => {
            "termination is not proved, so no trip count follows".to_owned()
        }
        Assumption::SizeNotBounded { var } => {
            format!("no size bound was derived for `{}`", var.symbol())
        }
        Assumption::CalleeCostUnknown { callee } => {
            format!("the cost of `{callee}` is not known here")
        }
        Assumption::RecursionNotRanked => {
            "the function recurses and no ranking function was found".to_owned()
        }
        Assumption::ExpressionDepthExceeded => {
            "the expression grew past the depth limit and was widened to omega".to_owned()
        }
        Assumption::ResourceNotModelled { detail } => {
            format!("the cost model has no rule for it ({detail})")
        }
        _ => format!("this build does not name the obligation ({UNNAMED})"),
    }
}

#[cfg(test)]
mod tests {
    use super::{describe, name, subject, undischarged};
    use landav_bound::{Assumption, Origin};
    use landav_engine::Hole;
    use landav_its::{Construct, Unsupported};

    fn hole(construct: &'static str) -> Hole {
        Hole::new(0, construct, Origin::new("probe.py:4:5"))
    }

    fn refusal(detail: &str) -> Unsupported {
        Unsupported::with_detail(Construct::Call, Origin::new("probe.py:4:5"), detail)
    }

    /// A `while` is a missing *termination* argument, not a missing cost rule.
    ///
    /// This is the assertion the module exists for. `Unsupported::blame` would
    /// answer `ResourceNotModelled` and would be right about a refusal; a
    /// `while` is not a refusal, it lowers, and the engine that holed it did
    /// read it. There is also no `Unsupported` node to ask, which is why the
    /// mapping is keyed on the hole.
    #[test]
    fn a_while_loop_is_a_termination_argument_that_was_not_made() {
        assert_eq!(
            undischarged(&hole("while"), &[], "loop_forever"),
            Assumption::TerminationNotProved
        );
    }

    /// A call names the callee whose cost is missing, taken from the frontend's
    /// own record rather than reconstructed.
    #[test]
    fn a_call_names_the_callee_whose_cost_is_unknown() {
        let assumption = undischarged(&hole("call"), &[refusal("fetch")], "caller");
        assert_eq!(name(&assumption), "callee-cost-unknown");
        assert_eq!(subject(&assumption).as_deref(), Some("fetch"));
        assert!(describe(&assumption).contains("fetch"));
    }

    /// A call to the enclosing function is recursion, and the obligation is a
    /// ranking function rather than another function's cost.
    #[test]
    fn a_self_call_is_recursion_that_was_not_ranked() {
        let assumption = undischarged(&hole("call"), &[refusal("walk")], "walk");
        assert_eq!(assumption, Assumption::RecursionNotRanked);
        assert!(describe(&assumption).contains("recurses"));
    }

    /// A call whose callee the frontend did not record still produces a
    /// record, and does not invent a name for the callee.
    #[test]
    fn a_call_with_no_recorded_callee_still_carries_an_assumption() {
        let assumption = undischarged(&hole("call"), &[], "caller");
        assert_eq!(name(&assumption), "callee-cost-unknown");
        assert_eq!(subject(&assumption).as_deref(), Some("call"));
    }

    /// Everything else is a construct the cost model has no rule for, carrying
    /// the frontend's description of it where there is one.
    #[test]
    fn any_other_construct_is_a_missing_cost_rule() {
        let assumption = undischarged(&hole("collection"), &[], "build");
        assert_eq!(name(&assumption), "resource-not-modelled");
        assert_eq!(subject(&assumption).as_deref(), Some("collection"));
    }

    /// Every assumption this build produces has a name that reads as words,
    /// and none of them is the placeholder.
    #[test]
    fn every_named_assumption_reads_as_words() {
        for construct in ["while", "call", "collection", "for", "subscript"] {
            let assumption = undischarged(&hole(construct), &[], "f");
            let named = name(&assumption);
            assert_ne!(
                named,
                super::UNNAMED,
                "`{construct}` produced an assumption this build cannot name"
            );
            assert!(
                named.contains('-') && named.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "an assumption must be named, never coded: {named}"
            );
            assert!(!describe(&assumption).is_empty());
        }
    }
}
