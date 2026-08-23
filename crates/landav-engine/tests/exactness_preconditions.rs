//! `LAN-75` AC, re-aimed by `LAN-87`: the facts this engine's exactness
//! argument rests on are still facts.
//!
//! # Why this file exists separately
//!
//! Every exact answer `landav-engine` gives depends on facts about the
//! *accepted* fragment, not on anything the engine checks. A counted loop's
//! trip count is arithmetic rather than inference **only because** nothing can
//! leave the loop early, and a statement costs one step only because nothing
//! inside it can cost more.
//!
//! So the engine's soundness has a dependency it cannot see. If one of these
//! facts stops holding — a perfectly reasonable thing to want — every `Exact`
//! this crate reports silently becomes a guess. There is no type that catches
//! that, because the engine's own inputs would still look well-formed.
//!
//! This file is the tripwire. It fails by **name**, saying which fact was
//! relaxed and what stops being true.
//!
//! # What `LAN-87` changed about it, and why the change was necessary
//!
//! The original version asked one question of each construct: *does
//! `landav_its::lower` still refuse it?* The premise was written into the
//! module docs — "a program that does not lower never reaches a solver **or**
//! this engine" — and `LAN-87` removes exactly that premise. The engine is now
//! total over `SourceProgram` and is consulted for programs that will never
//! become a transition system.
//!
//! A tripwire whose premise has been removed does not fail; it keeps passing
//! while the thing it was watching for happens. The refusal rows below still
//! ask the lowering question, because option (A) keeps the ITS refusing and a
//! change there is still worth hearing about. But the row that matters now asks
//! the **engine** question: given a program containing the construct, does the
//! engine charge it, or does it report a cost that leaves the construct out?

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::num::NonZeroI64;

use landav_bound::Origin;
use landav_engine::{TripCount, cost};
use landav_its::{Construct, RangeSpec, SourceProgram, SourceProgramBuilder, VarName, lower};

fn here() -> Origin {
    Origin::new("precondition.py:1")
}

fn one() -> NonZeroI64 {
    NonZeroI64::new(1).expect("1 is non-zero")
}

fn describe(result: &TripCount) -> String {
    let kind = match result {
        TripCount::Exact(_) => "Exact",
        TripCount::AtMost(_) => "AtMost",
        TripCount::Partial { .. } => "Partial",
        TripCount::Unknown => "Unknown",
    };
    let bound = result
        .bound()
        .map_or_else(|| "-".to_owned(), ToString::to_string);
    let holes: Vec<String> = result.holes().iter().map(ToString::to_string).collect();
    format!("{kind}({bound}) holes={holes:?}")
}

/// The facts the exactness argument depends on, each with the sentence that
/// stops being true if it is relaxed.
///
/// Kept as data rather than as one test per construct so that adding a fact to
/// the argument means adding a row, and so the failure message can carry the
/// consequence rather than just the name.
fn load_bearing_constructs() -> Vec<(Construct, &'static str)> {
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

/// `def f(n): x = <construct>` - the construct in the position where the
/// engine's "one step per statement" rule is applied.
fn statement_containing(construct: Construct) -> SourceProgram {
    let mut build = SourceProgramBuilder::new("guarded", here(), vec![VarName::new("n")]);
    let value = build.unsupported_expr(construct, here());
    let assign = build.assign(VarName::new("x"), value, here());
    build.build(vec![assign])
}

// ---------------------------------------------------------------------------
// the engine's side of the argument
// ---------------------------------------------------------------------------

/// **Each load-bearing construct is charged by the engine, wherever it sits.**
///
/// The row `LAN-87` makes load-bearing. `stmt_cost` gives `Assign` a cost of
/// exactly one step, and that is right only while an assignment's right-hand
/// side is arithmetic. An assignment whose value is a call costs one step *plus
/// the call*, and reporting `Exact(1)` for it is a two-sided claim about a cost
/// the engine never saw.
///
/// Asserted here rather than only in `hole_totality.rs` because this is the
/// file someone reads when they relax one of these constructs, and the engine's
/// dependency on it must be visible from the same place as the lowering's.
#[test]
fn every_construct_the_exactness_argument_rests_on_is_charged_by_the_engine() {
    let mut wrong = Vec::new();

    for (construct, consequence) in load_bearing_constructs() {
        let result = cost(&statement_containing(construct));
        let charged = matches!(result, TripCount::Partial { .. })
            && result
                .holes()
                .iter()
                .any(|hole| hole.construct() == construct.tag());
        if !charged {
            wrong.push(format!(
                "  {:<26} -> {}\n      {consequence}",
                construct.tag(),
                describe(&result)
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "the engine reported a cost for a statement without charging the \
         construct inside it:\n{}\n\n\
         Each of these programs is one assignment whose value the engine cannot \
         read. `Exact(1)` for it is the whole-statement claim being made on the \
         strength of the statement *form*, and it is exceeded by any callee \
         that does anything at all.",
        wrong.join("\n")
    );
}

/// **The engine keeps no assignment environment, and holing a region depends on
/// it.**
///
/// The invariant that is currently accidental, written down here because the
/// day it stops being true is the day holing a call becomes unsound.
///
/// The engine reads a variable's value from the *entry* state. It gets away
/// with that only for variables nothing assigns — which is what `LAN-87a`
/// enforces. The moment someone adds constant propagation to tighten
/// `m = 5; for i in range(m)`, this program becomes unsound: the region between
/// the assignment and the loop may set `m` to anything, so the propagated `5`
/// is a trip count with no evidence behind it.
///
/// So: after an unanalysable region, no value assigned before it may be used
/// for a trip count. The loop is a region of its own.
#[test]
fn a_value_assigned_before_an_unanalysable_region_is_not_used_after_it() {
    let mut build = SourceProgramBuilder::new("propagated", here(), vec![VarName::new("n")]);
    let five = build.int(5, here());
    let define_m = build.assign(VarName::new("m"), five, here());
    let region = build.unsupported_stmt(Construct::Call, here());
    let start = build.int(0, here());
    let stop = build.var(VarName::new("m"), here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        Vec::new(),
        here(),
    );
    let program = build.build(vec![define_m, region, loop_stmt]);

    let result = cost(&program);
    let shown = describe(&result);

    assert!(
        result.holes().iter().any(|hole| hole.construct() == "for"),
        "`m = 5; <call>; for i in range(m)` must treat the loop as a region: \
         the call may assign anything to `m`, so neither `m`'s entry value nor \
         the literal 5 is its value at the loop. Got {shown}.\n\n\
         A bound mentioning `m` is unusable (the caller has no `m`); a bound \
         saying the loop runs 5 times is unsound (the call may have changed it). \
         The engine keeps no environment, and that absence is load-bearing."
    );
}

/// **The same invariant, for a region that is not a whole statement.**
///
/// The statement case above passed while this one failed, which is how the rule
/// came to be written at one of three call sites. Python has exactly one
/// expression that rebinds a local - the walrus - and it is refused as
/// `Construct::BindingForm` in *condition* position, so
/// `if (n := 100) > 0: pass` followed by `for i in range(n)` reported
/// `Theta(2 + #hole0 + n)`: a two-sided claim, in the caller's `n`, for a loop
/// that runs a hundred times whatever the caller passed. Filling `#hole0` with
/// what the walrus genuinely costs - one assignment - then yields a *complete*
/// bound the program exceeds without limit, which breaks the composition
/// guarantee holes exist to provide.
///
/// The two spellings of one source-level fact must get the same treatment, so
/// the rule lives at `Walk::region`, where a region is recognised, rather than
/// at whichever caller happens to have been written first.
///
/// # Why this reads a **parameter** rather than a local
///
/// The statement-position test above assigns `m = 5` and then reads `m`, and
/// that shape cannot distinguish the two rules: `stmt_cost`'s `Assign` arm takes
/// `m` out of the readable set the moment it is assigned, so the loop is a
/// region whether or not the region in between cleared anything. A parameter is
/// the only kind of name that is readable *before* the region and would still be
/// readable after it, which is exactly the walrus case - `n := 100` rebinds the
/// caller's parameter.
#[test]
fn a_region_in_condition_position_also_forgets_what_came_before_it() {
    let mut build = SourceProgramBuilder::new("rebound", here(), vec![VarName::new("n")]);
    let region = build.unsupported_cond(Construct::BindingForm, here());
    let branch = build.if_else(region, Vec::new(), Vec::new(), here());
    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        Vec::new(),
        here(),
    );
    let program = build.build(vec![branch, loop_stmt]);

    let result = cost(&program);
    let shown = describe(&result);

    assert!(
        result.holes().iter().any(|hole| hole.construct() == "for"),
        "`if <region>: pass; for i in range(n)` must treat the loop as a region \
         for exactly the reason the statement-position version does: the region \
         may assign anything to `n`. Got {shown}.\n\n\
         A region in expression or condition position is still a region. \
         Charging its hole and leaving the readable set alone is the same \
         unsoundness `LAN-87a` removed, wearing a `Theta` label - and the \
         resulting bound is not merely loose, it is exceeded without limit by a \
         walrus that raises `n`."
    );
}

/// The engine's positive control. Without it the test above could pass because
/// the engine has stopped deriving anything, which would prove nothing.
#[test]
fn a_program_without_any_of_them_is_still_derived_exactly() {
    let result = cost(&clean_program());
    assert!(
        result.is_exact(),
        "a counted loop over a parameter must still be exact, or the charging \
         tests above are passing for the wrong reason: {}",
        describe(&result)
    );
}

// ---------------------------------------------------------------------------
// the lowering's side of the argument
// ---------------------------------------------------------------------------

/// Each load-bearing construct still stops a program from **lowering**.
///
/// No longer the tripwire for the engine — `LAN-87` disconnected those — but
/// still the statement of what option (A) chose: the ITS gains no
/// representation for unknown cost, so `Coverage::lowered()` keeps meaning
/// "the whole toolchain can handle this function". A construct that starts
/// lowering has had a sound transition written for it, and that is a decision
/// worth being told about.
#[test]
fn every_construct_the_exactness_argument_rests_on_is_still_a_refusal() {
    for (construct, consequence) in load_bearing_constructs() {
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

/// The construct vocabulary still contains every construct named above.
///
/// Separate from the tests that use them, because the two fail differently.
/// Deleting a variant is a compile error here and a silent gap there: if
/// `LoopJump` were removed because `break` became supported, the loops above
/// would simply have one fewer row to check and would still pass.
#[test]
fn the_refusal_vocabulary_still_names_each_one() {
    let named: Vec<&str> = load_bearing_constructs()
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
        "a fact the exactness argument depends on was renamed or removed. \
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
    assert!(
        lower(&clean_program()).is_ok(),
        "a counted loop over a parameter must lower, or the refusal tests above \
         are passing for the wrong reason"
    );
}

/// `def clean(n): for i in range(0, n, 1): x = 0`
fn clean_program() -> SourceProgram {
    let mut build = SourceProgramBuilder::new("clean", here(), vec![VarName::new("n")]);
    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let value = build.int(0, here());
    let assign = build.assign(VarName::new("x"), value, here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        vec![assign],
        here(),
    );
    build.build(vec![loop_stmt])
}
