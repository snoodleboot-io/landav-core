//! `LAN-75` AC: the refusals this engine's exactness argument rests on are
//! still refusals.
//!
//! # Why this file exists separately
//!
//! Every exact answer `landav-engine` gives depends on facts about the
//! *accepted* fragment, not on anything the engine checks. A counted loop's
//! trip count is arithmetic rather than inference **only because** nothing can
//! leave the loop early, and that is true only because the lowering refuses the
//! constructs that would let it.
//!
//! So the engine's soundness has a dependency it cannot see. If one of these
//! refusals is relaxed - a perfectly reasonable thing to want - every `Exact`
//! this crate reports silently becomes a guess. There is no type that catches
//! that and no test inside the engine that would notice, because the engine's
//! own inputs would still look well-formed.
//!
//! This file is the tripwire. It fails by **name**, saying which refusal was
//! relaxed and what stops being true, so the person relaxing it finds out from
//! a test rather than from a wrong bound in production.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::num::NonZeroI64;

use landav_bound::Origin;
use landav_its::{Construct, RangeSpec, SourceProgramBuilder, VarName, lower};

fn here() -> Origin {
    Origin::new("precondition.py:1")
}

/// The refusals the exactness argument depends on, each with the sentence that
/// stops being true if it is relaxed.
///
/// Kept as data rather than as one test per construct so that adding a refusal
/// to the argument means adding a row, and so the failure message can carry the
/// consequence rather than just the name.
fn load_bearing_refusals() -> Vec<(Construct, &'static str)> {
    vec![
        (
            Construct::LoopJump,
            "`break` and `continue` would let a loop stop before its counter is \
             exhausted, so a trip count derived from the range would be an \
             over-estimate rather than an equality - every `Exact` becomes `AtMost`",
        ),
        (
            Construct::ExceptionalControlFlow,
            "an exception can leave a loop mid-iteration, so neither the trip \
             count nor the body's cost is attained in full",
        ),
        (
            Construct::IntegerDivision,
            "division introduces loops whose counter is scaled rather than \
             stepped, whose trip count is logarithmic and not a polynomial the \
             summation can close",
        ),
        (
            Construct::Call,
            "a call has a cost this engine cannot see, so a body cost of `1` per \
             statement stops being the whole cost of the statement",
        ),
    ]
}

/// Each load-bearing construct still stops a program from lowering.
///
/// A program that does not lower never reaches a solver **or** this engine, so
/// a refusal here is what keeps the accepted fragment equal to the fragment the
/// exactness argument was made about.
#[test]
fn every_refusal_the_exactness_argument_rests_on_is_still_a_refusal() {
    for (construct, consequence) in load_bearing_refusals() {
        let mut build = SourceProgramBuilder::new("guarded", here(), vec![VarName::new("n")]);
        let refused = build.unsupported_stmt(construct, here());
        let program = build.build(vec![refused]);

        assert!(
            lower(&program).is_err(),
            "`{}` ({}) no longer stops a program from lowering.\n\n\
             landav-engine's exactness argument depends on it: {consequence}.\n\n\
             If this refusal was relaxed deliberately, the engine must be \
             revisited before this test is - loosening it here does not make \
             the bounds correct, it only stops anyone finding out.",
            construct.tag(),
            construct.describe(),
        );
    }
}

/// The construct vocabulary still contains every refusal named above.
///
/// Separate from the test that they refuse, because the two fail differently.
/// Deleting a variant is a compile error here and a silent gap there: if
/// `LoopJump` were removed because `break` became supported, the loop above
/// would simply have one fewer row to check and would still pass.
#[test]
fn the_refusal_vocabulary_still_names_each_one() {
    let named: Vec<&str> = load_bearing_refusals()
        .iter()
        .map(|(construct, _)| construct.tag())
        .collect();
    assert_eq!(
        named,
        vec![
            "loop-jump",
            "exceptional-control-flow",
            "integer-division",
            "call"
        ],
        "a refusal the exactness argument depends on was renamed or removed. \
         Renaming is fine once this list follows it; removing means the \
         construct is now accepted, and the engine's `Exact` answers need \
         re-deriving before this list does."
    );
}

/// The positive control. Without it the tests above could pass because
/// *everything* fails to lower, which would prove nothing about refusals.
///
/// This session already shipped one test that could not fail; the cheapest
/// defence is to assert that the thing being detected is actually a difference.
#[test]
fn a_program_without_any_of_them_still_lowers() {
    let mut build = SourceProgramBuilder::new("clean", here(), vec![VarName::new("n")]);
    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let value = build.int(0, here());
    let assign = build.assign(VarName::new("x"), value, here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, NonZeroI64::new(1).unwrap()),
        vec![assign],
        here(),
    );
    let program = build.build(vec![loop_stmt]);

    assert!(
        lower(&program).is_ok(),
        "a counted loop over a parameter must lower, or the refusal tests above \
         are passing for the wrong reason"
    );
}
