//! `LAN-87` acceptance: the engine is **total** over `SourceProgram`, and every
//! node it cannot analyse is charged as a hole *at its position in the control
//! structure*.
//!
//! # What is being defended
//!
//! Before this ticket the engine was written against a fragment the lowering
//! had already filtered: `landav_its::lower` refused any program containing an
//! `Unsupported` node, so `landav-engine` never had to have an opinion about
//! one that was not a whole statement. `LAN-87` removes that filter for the
//! engine (the ITS keeps refusing — that is the point of option (A)), which
//! makes the engine's treatment of an unsupported node load-bearing for the
//! first time.
//!
//! Four ways it can go wrong, and all four were measured on the code as it
//! stands:
//!
//! | shape | engine today | truth |
//! |---|---|---|
//! | `if f(n): x = 0` | `AtMost(3)`, no holes | `3 + cost(f)` |
//! | `x = f(n)` | `Exact(1)` | `1 + cost(f)` |
//! | `for i in range(n): x = f(i)` | `Exact(2n)` | `n * (2 + cost(f))` |
//! | an `Unsupported` node with no parent | `Exact(...)` | not derivable |
//!
//! Each of those reports a **complete** bound — one a caller may compare
//! against a budget — that omits a cost the program really pays. That is the
//! failure class this file exists to catch, and it is worse than reporting
//! nothing: `Exact` is a two-sided claim.
//!
//! # How these assert
//!
//! * **By meaning, not by syntax.** Bounds are compared by evaluating them at
//!   concrete valuations, exactly as `exactness.rs` does. The bound algebra's
//!   normal form is allowed to change; an assertion on the printed form would
//!   fail for reasons that are not defects.
//! * **Over the whole vocabulary, not a sample.** Every test that is about
//!   totality iterates [`Construct::all()`] rather than naming `Call`. A
//!   construct added to that vocabulary tomorrow is covered here today, which
//!   is the only version of "total" a test can actually assert.
//! * **Collecting every failure.** A per-construct assertion that stops at the
//!   first failure tells the implementer about one row of twenty-one. These
//!   collect and report the whole set.

// A test that cannot build its fixture should stop loudly and immediately.
// The library lints stay in force; this is the established exception for test
// targets across the workspace.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, num::NonZeroI64};

use landav_bound::{Bound, Nat, Origin, Symbol, Valuation, VarId};
use landav_engine::{Hole, TripCount, cost};
use landav_its::{Construct, RangeSpec, SourceProgram, SourceProgramBuilder, VarName};

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

fn here() -> Origin {
    Origin::new("totality.py:7")
}

fn one() -> NonZeroI64 {
    NonZeroI64::new(1).expect("1 is non-zero")
}

/// A valuation over an explicit table, zero elsewhere.
///
/// Built from the names the *result* carries — never from a guessed hole name —
/// so a change to the hole naming scheme cannot silently turn these assertions
/// into checks against zero.
struct Bindings(BTreeMap<Symbol, u64>);

impl Valuation for Bindings {
    fn value_of(&self, var: &VarId) -> Nat {
        Nat::Fin(self.0.get(var.symbol()).copied().unwrap_or(0))
    }
}

/// The bound's value with `n` and the single hole bound to the given numbers.
fn value_at(bound: &Bound, hole: &Hole, n: u64, hole_value: u64) -> Nat {
    let mut table = BTreeMap::new();
    table.insert(Symbol::from("n"), n);
    table.insert(hole.var().symbol().clone(), hole_value);
    bound.eval(&Bindings(table))
}

/// Whether any hole in `result` blames `construct`.
fn blames(result: &TripCount, construct: Construct) -> bool {
    result
        .holes()
        .iter()
        .any(|hole| hole.construct() == construct.tag())
}

/// The hole blaming `construct`, for reading its variable back out.
fn hole_for(result: &TripCount, construct: Construct) -> Option<&Hole> {
    result
        .holes()
        .iter()
        .find(|hole| hole.construct() == construct.tag())
}

/// A short rendering of a result, for assertion messages.
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

// ---------------------------------------------------------------------------
// the five positions
//
// One `Unsupported` node, in each place the arenas can hold one. Every program
// is otherwise trivially analysable, so anything but a hole for the node is the
// engine reporting a cost it has not established.
// ---------------------------------------------------------------------------

/// `def f(n): <construct>`
fn in_statement_position(construct: Construct) -> SourceProgram {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let refused = build.unsupported_stmt(construct, here());
    build.build(vec![refused])
}

/// `def f(n): x = <construct>`
fn on_an_assignment_right_hand_side(construct: Construct) -> SourceProgram {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let value = build.unsupported_expr(construct, here());
    let assign = build.assign(VarName::new("x"), value, here());
    build.build(vec![assign])
}

/// `def f(n): if <construct>: x = 0`
fn in_an_if_condition(construct: Construct) -> SourceProgram {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let cond = build.unsupported_cond(construct, here());
    let zero = build.int(0, here());
    let assign = build.assign(VarName::new("x"), zero, here());
    let branch = build.if_else(cond, vec![assign], Vec::new(), here());
    build.build(vec![branch])
}

/// `def f(n): for i in range(0, <construct>): x = 0`
fn in_a_range_endpoint(construct: Construct) -> SourceProgram {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let start = build.int(0, here());
    let stop = build.unsupported_expr(construct, here());
    let zero = build.int(0, here());
    let assign = build.assign(VarName::new("x"), zero, here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        vec![assign],
        here(),
    );
    build.build(vec![loop_stmt])
}

/// `def f(n): x = 0`, with an `Unsupported` node built into the arena and
/// nothing pointing at it.
///
/// Not a contrived shape. `landav-python` translates the value of a `return`
/// and of a bare expression statement precisely so the refusal is recorded,
/// and this fragment's `Return` carries no value — so the node it builds has no
/// parent. `landav_its::lower` handles that by scanning the arenas rather than
/// by walking; the engine walks, and this is the case that walk misses.
fn orphaned_in_the_arena(construct: Construct) -> SourceProgram {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let _orphan = build.unsupported_expr(construct, here());
    let zero = build.int(0, here());
    let assign = build.assign(VarName::new("x"), zero, here());
    build.build(vec![assign])
}

/// One position an `Unsupported` node can occupy: its name, and how to build a
/// program that puts a construct there.
type Position = (&'static str, fn(Construct) -> SourceProgram);

/// Every position an `Unsupported` node can occupy, by name, for the totality
/// tests below.
fn attached_positions() -> Vec<Position> {
    vec![
        ("statement", in_statement_position as fn(_) -> _),
        (
            "assignment right-hand side",
            on_an_assignment_right_hand_side,
        ),
        ("if condition", in_an_if_condition),
        ("for-range endpoint", in_a_range_endpoint),
    ]
}

// ---------------------------------------------------------------------------
// totality
// ---------------------------------------------------------------------------

/// **Every construct, in every attached position, is charged as a hole that
/// names it.**
///
/// The acceptance criterion in one test. `Partial` and nothing else: `Exact`
/// and `AtMost` are complete answers a caller may compare against a budget, and
/// `Unknown` throws away everything derived around the region, which is the
/// regression `LAN-81` fixed.
///
/// The hole must **name the construct**, because named blame is the entire
/// deliverable — "441 functions now analysed apart from the call at line 12" is
/// actionable and "441 functions now have a hole" is not.
#[test]
fn every_construct_in_every_position_is_charged_as_a_hole_that_names_it() {
    let mut wrong = Vec::new();

    for (position, build) in attached_positions() {
        for construct in Construct::all() {
            let result = cost(&build(*construct));
            if !matches!(result, TripCount::Partial { .. }) || !blames(&result, *construct) {
                wrong.push(format!(
                    "  {position:<26} {:<26} -> {}",
                    construct.tag(),
                    describe(&result)
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "an unanalysable node was not charged as a hole naming it. Each row is \
         a program whose only difficulty is one `Unsupported` node in the named \
         position; the engine must report `Partial` carrying a hole whose \
         `construct()` is that construct's tag.\n\n{}\n\n\
         An `Exact` or `AtMost` here is the serious case: it is a complete \
         bound, offered for comparison against a budget, that omits a cost the \
         program pays.",
        wrong.join("\n")
    );
}

/// **The hole's variable actually appears in the bound.**
///
/// Separate from the test above because they fail differently and only one of
/// them is a soundness failure. A result can carry a hole in its ledger and a
/// bound that does not mention it — `holes: [call]`, `bound: 1` — and that
/// reads as "cost 1, with a footnote" to every consumer: `Bound::subst` has
/// nothing to substitute into, so filling the hole later changes nothing and
/// the number stays exceedable.
#[test]
fn the_bound_mentions_every_hole_it_carries() {
    let mut wrong = Vec::new();

    for (position, build) in attached_positions() {
        for construct in Construct::all() {
            let result = cost(&build(*construct));
            let Some(bound) = result.bound() else {
                continue;
            };
            for hole in result.holes() {
                if !bound.vars().contains(&hole.var()) {
                    wrong.push(format!(
                        "  {position:<26} {:<26} hole {} missing from {bound}",
                        construct.tag(),
                        hole.var().symbol(),
                    ));
                }
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "a hole was listed but does not occur in the bound, so the bound reads \
         as a complete cost with a footnote and `Bound::subst` has nothing to \
         fill:\n{}",
        wrong.join("\n")
    );
}

/// **An `Unsupported` node with no parent never yields a complete bound.**
///
/// The one that would otherwise ship. The traversal cannot reach the node, so
/// nothing in the walk is wrong; the result is simply a confident `Exact` for a
/// program with an unanalysed region in it. `landav_its::lower` already treats
/// this as the important case — see `refuse_every_unsupported_node`, which
/// scans the arenas exactly because "a silently dropped refusal is exactly the
/// truncation `LAN-67` criterion 4 forbids".
///
/// Deliberately does **not** require `Partial`: charging an orphan at top level
/// is itself unsound when the program contains a loop (see the fail-closed test
/// below), so `Unknown` is the correct answer here and `Partial` naming the
/// construct is acceptable. What is never acceptable is a complete bound.
#[test]
fn an_unsupported_node_with_no_parent_never_yields_a_complete_bound() {
    let mut wrong = Vec::new();

    for construct in Construct::all() {
        let result = cost(&orphaned_in_the_arena(*construct));
        if result.is_complete() {
            wrong.push(format!(
                "  {:<26} -> {}",
                construct.tag(),
                describe(&result)
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "an `Unsupported` node that nothing points at was omitted from the \
         result, which was then reported as a complete bound:\n{}\n\n\
         The engine walks the control structure; this node is only reachable by \
         scanning the arenas, which is why `landav_its::lower` scans. A result \
         that omits it is a bound the program can exceed.",
        wrong.join("\n")
    );
}

/// **Fail closed: an uncharged node makes the whole result `Unknown`.**
///
/// Not `Partial`. The tempting repair — sweep the unreached nodes up and add
/// one hole per node at the top level — is unsound, and measurably so: for
/// `orphan + for i in range(n): pass` a flat top-level charge gives
/// `n + #hole`, while the truth if the orphan belonged to the loop body is
/// `n * (1 + #hole)`. The flat charge *understates*. There is no position
/// information to recover, so there is no sound charge, so the answer is that
/// nothing is known.
#[test]
fn an_uncharged_node_makes_the_result_unknown_rather_than_partial() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let _orphan = build.unsupported_expr(Construct::Call, here());
    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        Vec::new(),
        here(),
    );
    let program = build.build(vec![loop_stmt]);

    let result = cost(&program);

    assert!(
        matches!(result, TripCount::Unknown),
        "an `Unsupported` node the traversal cannot reach must fail the whole \
         result closed, got {}.\n\n\
         `Partial` would be a claim about where the cost sits, and there is \
         nothing to base that claim on: charged at top level the answer is \
         `n + #hole`, and if the node belonged inside the loop the truth is \
         `n * (1 + #hole)`, which the flat charge understates.",
        describe(&result)
    );
}

// ---------------------------------------------------------------------------
// multiplicity
// ---------------------------------------------------------------------------

/// **A region inside a loop body is paid once per iteration.**
///
/// `for i in range(n): x = <call>` costs `n * (2 + call)` — the assignment, the
/// call, and the loop's own step, `n` times. Asserted by evaluation at a
/// valuation that separates every candidate wrong answer:
///
/// | answer | at `n = 3`, `call = 5` |
/// |---|---|
/// | `n * (2 + call)` — correct | 21 |
/// | `2n` — today's answer, the call dropped | 6 |
/// | `2n + call` — a flat charge outside the loop | 11 |
/// | `n * (1 + call)` — the assignment dropped | 18 |
///
/// This is the assertion that catches the flat-charge repair, which is the
/// cheapest wrong fix and the one a reviewer will propose.
#[test]
fn a_region_on_an_assignment_inside_a_loop_is_paid_once_per_iteration() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let value = build.unsupported_expr(Construct::Call, here());
    let assign = build.assign(VarName::new("x"), value, here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        vec![assign],
        here(),
    );
    let program = build.build(vec![loop_stmt]);

    let result = cost(&program);
    let shown = describe(&result);

    assert!(
        matches!(result, TripCount::Partial { .. }),
        "a loop body containing a call is not derivable, got {shown}"
    );
    let hole = hole_for(&result, Construct::Call)
        .unwrap_or_else(|| panic!("the call must be blamed by name, got {shown}"));
    let bound = result.bound().expect("a partial result carries a bound");

    for (n, call, expected) in [(3_u64, 5_u64, 21_u64), (0, 5, 0), (1, 0, 2), (4, 1, 12)] {
        assert_eq!(
            value_at(bound, hole, n, call),
            Nat::Fin(expected),
            "at n = {n} and a call costing {call}, the loop costs \
             n * (2 + call) = {expected}, got {shown}.\n\n\
             `2n` means the call was dropped; `2n + call` means it was charged \
             once outside the loop rather than once per iteration, which \
             understates the cost of every loop containing a call."
        );
    }

    // The hole and the parameter are the only things the caller has to supply.
    // Were the loop counter to escape, the numbers above could still line up at
    // a valuation that happens to bind it to zero.
    for var in bound.vars() {
        assert!(
            var.symbol().as_str() == "n" || Hole::is_hole(&var),
            "the bound mentions `{}`, which is neither a parameter nor a hole: \
             {shown}",
            var.symbol()
        );
    }
}

/// **The same, with the region as a whole statement.** Pinned here rather than
/// left implicit, because Phase 2's repair for the orphan case (charging
/// unreached nodes at the top level) would break it, and it would break
/// silently: the bound stays plausible and gets smaller.
///
/// # Amended from `n * (1 + call)`, for the reason recorded on
/// `calls_become_holes::a_call_in_a_loop_body_is_paid_once_per_iteration`
///
/// `SourceProgramBuilder::unsupported_stmt` builds a node of
/// `Extent::Statement` — one that stands for a whole source statement — so the
/// step that executing it costs is charged here and nowhere else. Under the
/// engine's declared unit that is `n * (2 + call)`: the loop's own step, the
/// statement, and the region, once per iteration.
///
/// The distinction is real and this is the test that pins the *statement* half
/// of it. A node built with `unsupported_fragment` — what `landav-python` emits
/// for the `f(n)` in `return f(n)`, where the `Return` beside it already pays a
/// step — is charged the region alone.
#[test]
fn a_region_as_a_statement_inside_a_loop_stays_paid_once_per_iteration() {
    let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let refused = build.unsupported_stmt(Construct::Call, here());
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one()),
        vec![refused],
        here(),
    );
    let program = build.build(vec![loop_stmt]);

    let result = cost(&program);
    let shown = describe(&result);
    let hole = hole_for(&result, Construct::Call)
        .unwrap_or_else(|| panic!("the call must be blamed by name, got {shown}"));
    let bound = result.bound().expect("a partial result carries a bound");

    // n * (2 + call): the loop's own step, the statement, and the call, n times.
    assert_eq!(
        value_at(bound, hole, 3, 5),
        Nat::Fin(21),
        "at n = 3 and a call costing 5 this loop costs n * (2 + call) = 21, \
         got {shown}"
    );
}

// ---------------------------------------------------------------------------
// control-flow constructs
// ---------------------------------------------------------------------------

/// **Holing an edge out of a loop cannot leave the trip count claimed exact.**
///
/// A `break` does not merely cost something unknown; it falsifies the equality
/// the loop's trip count rests on. `TripCount`'s own documentation says so —
/// the count is arithmetic rather than inference *because* "the fragment
/// refuses `break`, `continue` and exceptions, so nothing can leave early".
///
/// Today `for i in range(n): break` reports `Partial(n * (1 + #hole0))` with
/// `exact_elsewhere = true`, and that flag is a claim that everything outside
/// the hole was derived exactly — including a trip count of exactly `n` for a
/// loop that may run once.
///
/// The second half of the test is the control: a `call` in the same position
/// **must** keep `exact_elsewhere = true`, or the fix was "relax everything",
/// which would throw away the distinction that makes a partial bound worth
/// reporting.
#[test]
fn a_control_flow_construct_in_a_loop_body_relaxes_the_trip_count() {
    let build_loop = |construct: Construct| {
        let mut build = SourceProgramBuilder::new("f", here(), vec![VarName::new("n")]);
        let start = build.int(0, here());
        let stop = build.var(VarName::new("n"), here());
        let refused = build.unsupported_stmt(construct, here());
        let loop_stmt = build.for_range(
            VarName::new("i"),
            RangeSpec::new(start, stop, one()),
            vec![refused],
            here(),
        );
        build.build(vec![loop_stmt])
    };

    for construct in [Construct::LoopJump, Construct::ExceptionalControlFlow] {
        let result = cost(&build_loop(construct));
        assert!(
            !result.exact_outside_holes(),
            "`{}` can leave the loop before its counter is exhausted, so the \
             trip count is no longer an equality and nothing enclosing it was \
             derived exactly. Got {}, which asserts otherwise.",
            construct.tag(),
            describe(&result)
        );
    }

    let result = cost(&build_loop(Construct::Call));
    assert!(
        result.exact_outside_holes(),
        "a call costs something unknown but cannot leave the loop early, so \
         everything around it is still exact and must still say so - otherwise \
         `Theta(... apart from the call at line 7)` degrades to `O(...)` and \
         the partial bound stops being worth more than 'at most infinity'. Got \
         {}",
        describe(&result)
    );
}
