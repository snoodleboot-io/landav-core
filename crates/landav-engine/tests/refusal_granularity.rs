//! `LAN-100`: **a refusal forgets what it can change, not the whole frame.**
//!
//! # What changed, and the one rule behind it
//!
//! `Walk::region` used to end everything the engine knew: an unanalysable
//! region may assign to anything, so no earlier value survived it. That is the
//! right default and it was far too strong in the common case. Measured on the
//! typed corpus, 75 of the 91 loops that iterate a sized parameter lost their
//! trip count to a refusal *earlier in the same function*, and most of those
//! refusals were a binding of one known name - `total = 0` condemned by a
//! later `total = total + a`. A statement that rebinds `total` cannot change
//! how many entries a mapping three lines below has.
//!
//! A refused node now carries [`Writes`]: which locals it may rebind, and
//! whether it may mutate an object. The engine forgets exactly that. A node
//! that says nothing inherits its construct's answer, which is what keeps the
//! narrowing an opt-in per node rather than a relaxation of the default.
//!
//! # Why these tests build programs rather than translate Python
//!
//! The claim under test is the engine's: what survives a region is decided in
//! `Walk::region` and `writes_of`, from the write set alone. A Python source
//! would test the frontend's scan on top of that, and the two failure modes
//! would be indistinguishable. `landav-cli/tests/refusal_granularity.rs` tests
//! the scan; this file tests what the engine does with its answer.
//!
//! The direction that matters is **no bound moves down**: every narrowing here
//! must be one a program cannot exceed. The property suite in
//! `landav-its/tests/properties/engine_cost.rs` checks that against a reference
//! interpreter over generated programs; these are the named cases.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::num::NonZeroI64;

use landav_bound::Origin;
use landav_engine::{Hole, TripCount, cost};
use landav_its::{
    Construct, Extent, RangeSpec, SourceProgram, SourceProgramBuilder, StmtId, VarName, Writes,
};

fn here() -> Origin {
    Origin::new("granularity.py:1")
}

fn one() -> NonZeroI64 {
    NonZeroI64::new(1).expect("1 is non-zero")
}

fn describe(result: &TripCount) -> String {
    let kind = match result {
        TripCount::Exact(_) => "Theta",
        TripCount::AtMost(_) => "O",
        TripCount::Partial { .. } => "Partial",
        TripCount::Unknown => "Unknown",
    };
    let bound = result
        .bound()
        .map_or_else(|| "-".to_owned(), ToString::to_string);
    let holes: Vec<&str> = result.holes().iter().map(Hole::construct).collect();
    format!("{kind}({bound}) holes={holes:?}")
}

fn mentions(result: &TripCount, name: &str) -> bool {
    result
        .bound()
        .is_some_and(|bound| bound.vars().iter().any(|var| var.symbol().as_str() == name))
}

fn holes_on(result: &TripCount, construct: &str) -> bool {
    result
        .holes()
        .iter()
        .any(|hole| hole.construct() == construct)
}

/// `for i in range(0, stop): a = 0`, the loop whose endpoint the refusal before
/// it must not erase.
fn counted_loop(builder: &mut SourceProgramBuilder, stop: &str, body: Vec<StmtId>) -> StmtId {
    let start = builder.int(0, here());
    let stop = builder.var(VarName::new(stop), here());
    let mut statements = body;
    if statements.is_empty() {
        let zero = builder.int(0, here());
        statements.push(builder.assign(VarName::new("a"), zero, here()));
    }
    builder.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        statements,
        here(),
    )
}

/// A refused statement carrying `writes`, then a counted loop over `n`.
fn refusal_then_loop(construct: Construct, writes: Writes) -> SourceProgram {
    let mut builder = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let refused =
        builder.unsupported_stmt_writing(construct, None, Extent::Statement, writes, here());
    let looped = counted_loop(&mut builder, "n", Vec::new());
    builder.build(vec![refused, looped])
}

// ---------------------------------------------------------------------------
// 1 · a refused binding forgets the name it binds, and nothing else
// ---------------------------------------------------------------------------

/// **`x = <unreadable>; for i in range(0, n)` is a function of `n`.**
///
/// The whole of the ticket in one program. The refusal is still a hole - the
/// value of `x` is unknown and so is the cost of finding it - and the loop is
/// still counted, because `x` is the only thing the statement could change.
#[test]
fn a_refused_binding_of_a_local_keeps_the_endpoint() {
    let program = refusal_then_loop(
        Construct::NonIntegerValue,
        Writes::only([VarName::new("x")]),
    );
    let result = cost(&program);
    let shown = describe(&result);
    assert!(
        mentions(&result, "n"),
        "the loop must be counted by `n`: the refused binding of `x` cannot have \
         changed it. Got {shown}"
    );
    assert!(
        !holes_on(&result, "for"),
        "the loop lost its endpoint to a refusal that named a different variable. \
         Got {shown}"
    );
    assert!(
        holes_on(&result, "non-integer-value"),
        "the refusal must still be a hole: narrowing what it forgets does not \
         make it analysable. Got {shown}"
    );
    assert!(
        !result.is_complete(),
        "a function with a refused statement makes no complete claim. Got {shown}"
    );
}

/// **`n = <unreadable>; for i in range(0, n)` is not.**
///
/// The converse, and the fence: naming the variable the refusal binds is what
/// lets the engine forget it and keep the rest. Forgetting it is not optional.
#[test]
fn a_refused_binding_of_the_endpoint_forgets_it() {
    let program = refusal_then_loop(
        Construct::NonIntegerValue,
        Writes::only([VarName::new("n")]),
    );
    let result = cost(&program);
    let shown = describe(&result);
    assert!(
        !mentions(&result, "n"),
        "`n` was rebound to a value the engine cannot read, so no bound may \
         mention it. Got {shown}"
    );
    assert!(
        holes_on(&result, "for"),
        "with its endpoint gone the loop must be a hole. Got {shown}"
    );
}

/// **A refusal that says nothing still forgets the frame.**
///
/// The default is unchanged, and this pins it: a `non-integer-value` refusal
/// with no write set is a frame-wide region exactly as it was before
/// `LAN-100`. The narrowing is opt-in per node, never a relaxation of the
/// construct's answer.
#[test]
fn an_unstated_refusal_still_forgets_the_frame() {
    let program = refusal_then_loop(Construct::NonIntegerValue, Writes::unstated());
    let result = cost(&program);
    let shown = describe(&result);
    assert!(
        !mentions(&result, "n") && holes_on(&result, "for"),
        "a refusal with nothing declared must forget every readable value, as it \
         always did. Got {shown}"
    );
}

/// **A refusal that says "any local" forgets the frame whatever its kind says.**
///
/// An attribute access rebinds nothing *by kind* - foreign code runs in its own
/// frame - but `x[(n := 5)]` hides a walrus in an operand the frontend did not
/// translate. The frontend that saw it says `Writes::frame`, and that must
/// outrank the construct.
#[test]
fn a_frame_answer_outranks_a_construct_that_rebinds_nothing() {
    let by_kind = refusal_then_loop(Construct::Attribute, Writes::unstated());
    let widened = refusal_then_loop(Construct::Attribute, Writes::frame());
    let (by_kind, widened) = (cost(&by_kind), cost(&widened));
    assert!(
        mentions(&by_kind, "n"),
        "an attribute access rebinds no local by kind, so the loop below it is \
         counted. Got {}",
        describe(&by_kind)
    );
    assert!(
        !mentions(&widened, "n") && holes_on(&widened, "for"),
        "the frontend saw something that may rebind a local, and its answer \
         must win over the kind's. Got {}",
        describe(&widened)
    );
}

// ---------------------------------------------------------------------------
// 2 · objects are a separate question, answered separately
// ---------------------------------------------------------------------------

/// **A refusal that touches no object keeps a length; one that may does not.**
///
/// `len(items)` is volatile: a `property` getter may call `items.append(...)`,
/// so any region that may mutate an object loses it. `total = 0` is not such a
/// region, and saying so is what lets `for x in items` below it be counted -
/// which is the corpus case `LAN-100` was measured on. `x.y` *is* such a
/// region even though it rebinds no local, and the length must go.
#[test]
fn a_pure_refusal_keeps_a_volatile_length_and_an_impure_one_loses_it() {
    fn subject(writes: Writes) -> TripCount {
        let mut builder = SourceProgramBuilder::new("f", here(), vec![VarName::new("len(items)")]);
        builder.mark_volatile(VarName::new("len(items)"));
        let refused = builder.unsupported_stmt_writing(
            Construct::NonIntegerValue,
            Some("total".into()),
            Extent::Statement,
            writes,
            here(),
        );
        let looped = counted_loop(&mut builder, "len(items)", Vec::new());
        cost(&builder.build(vec![refused, looped]))
    }
    let pure = subject(Writes::only([VarName::new("total")]));
    let impure = subject(Writes::at_most([VarName::new("total")]));
    assert!(
        mentions(&pure, "len(items)") && !holes_on(&pure, "for"),
        "a refused binding that touches no object cannot change how long `items` \
         is, so the loop over it is counted. Got {}",
        describe(&pure)
    );
    assert!(
        !mentions(&impure, "len(items)") && holes_on(&impure, "for"),
        "a refusal that may mutate an object may have changed the length read on \
         entry, and the loop must lose it. Got {}",
        describe(&impure)
    );
}

// ---------------------------------------------------------------------------
// 3 · inside a loop body, the same rule decides what the body writes
// ---------------------------------------------------------------------------

/// **A refused binding in a loop body forgets its name, and a nested loop
/// keeps its endpoint.**
///
/// `for_range_cost` asks `writes_of` what the body assigns before walking it,
/// and an answer of "anything" clears every readable value, which costs a
/// nested counted loop its endpoint. A refused binding in the body answers
/// with its one name, exactly as an ordinary assignment does.
#[test]
fn a_refused_binding_in_a_body_forgets_only_its_name() {
    fn subject(writes: Writes) -> TripCount {
        let mut builder = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
        let refused = builder.unsupported_stmt_writing(
            Construct::NonIntegerValue,
            Some("x".into()),
            Extent::Statement,
            writes,
            here(),
        );
        let inner = counted_loop(&mut builder, "n", Vec::new());
        let outer = {
            let start = builder.int(0, here());
            let stop = builder.var(VarName::new("n"), here());
            builder.for_range(
                VarName::new("j"),
                RangeSpec::new(start, stop, one()),
                vec![refused, inner],
                here(),
            )
        };
        cost(&builder.build(vec![outer]))
    }
    let narrowed = subject(Writes::only([VarName::new("x")]));
    let unstated = subject(Writes::unstated());
    assert!(
        mentions(&narrowed, "n") && !holes_on(&narrowed, "for"),
        "both loops count on `n`, which the refused binding of `x` in the outer \
         body cannot change. Got {}",
        describe(&narrowed)
    );
    assert!(
        holes_on(&unstated, "for"),
        "a refusal with nothing declared in the outer body may have rebound `n` \
         before the inner loop read it, so the inner loop is a hole. Got {}",
        describe(&unstated)
    );
}
