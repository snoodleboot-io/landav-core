//! `LAN-67` criteria 1 and 2, construct by construct.
//!
//! Criterion 1 is *locations, transitions, guards and polynomial updates
//! emitted*; criterion 2 is *`while`, `for`-range, `if`/`else`, nested loops,
//! integer arithmetic*. The properties in `soundness` cover both in bulk. What
//! is here is the handful of cases where the bulk property is satisfiable by
//! something subtly wrong, and where the expected answer is therefore written
//! out **by hand** rather than computed by the reference — so that lowering
//! and reference cannot agree on the same mistake.

use std::{collections::BTreeMap, num::NonZeroI64};

use landav_bound::Origin;
use landav_its::{
    ArithOp, CompareOp, Construct, Location, RangeSpec, SourceProgramBuilder, VarName, lower,
};

use crate::reference::{Simulation, State, simulate};

/// A budget generous enough for every hand-written case here.
const BUDGET: u64 = 10_000;

fn origin(line: u32) -> Origin {
    Origin::new(format!("hand.py:{line}:1"))
}

fn state(pairs: &[(&str, i128)]) -> State {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), *value))
        .collect::<BTreeMap<String, i128>>()
}

/// Simulates, and fails loudly if the run did not reach the exit.
fn run_to_exit(program: &landav_its::SourceProgram, initial: &State) -> (State, u64) {
    let its = match lower(program) {
        Ok(its) => its,
        Err(error) => panic!("lowering refused a program inside the fragment: {error}"),
    };
    match simulate(&its, initial, BUDGET) {
        Simulation::Terminated { state, length } => (state, length),
        other => panic!("simulation did not terminate: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// criterion 1: what gets emitted
// ---------------------------------------------------------------------------

/// **Criterion 1.** An assignment becomes one transition carrying a genuinely
/// polynomial update — a product of two variables, which no linear encoding
/// could represent.
#[test]
fn a_polynomial_update_is_emitted() {
    let mut builder = SourceProgramBuilder::new("quadratic", origin(1), vec![VarName::new("n")]);
    // x = n * n + n
    let left = builder.var(VarName::new("n"), origin(2));
    let right = builder.var(VarName::new("n"), origin(2));
    let square = builder.arith(ArithOp::Mul, left, right, origin(2));
    let plus = builder.var(VarName::new("n"), origin(2));
    let total = builder.arith(ArithOp::Add, square, plus, origin(2));
    let assign = builder.assign(VarName::new("x"), total, origin(2));
    let program = builder.build(vec![assign]);

    let its = lower(&program).expect("inside the fragment");

    assert!(!its.locations().is_empty(), "no locations were emitted");
    assert!(!its.transitions().is_empty(), "no transitions were emitted");

    let update = its
        .transitions()
        .iter()
        .find_map(|transition| transition.update().get(&landav_its::ItsVar::new("x")))
        .expect("no transition assigns x");

    assert_eq!(update.degree(), 2, "n * n should be degree two: {update}");
    assert_eq!(
        update.monomials().len(),
        2,
        "n^2 + n has two terms: {update}"
    );

    // Evaluated by hand at n = 5: 25 + 5.
    let (final_state, _) = run_to_exit(&program, &state(&[("n", 5)]));
    assert_eq!(final_state.get("x"), Some(&30));
}

/// **Criterion 1.** Both polarities of a condition become guards, and they
/// partition: exactly one is available in any state.
#[test]
fn the_two_branches_of_an_if_partition_the_state_space() {
    for (n, expected) in [(-1_i128, 100_i128), (0, 100), (1, 200), (7, 200)] {
        let mut builder = SourceProgramBuilder::new("branch", origin(1), vec![VarName::new("n")]);
        let read = builder.var(VarName::new("n"), origin(2));
        let zero = builder.int(0, origin(2));
        let cond = builder.compare(CompareOp::Gt, read, zero, origin(2));

        let two_hundred = builder.int(200, origin(3));
        let then_stmt = builder.assign(VarName::new("r"), two_hundred, origin(3));
        let one_hundred = builder.int(100, origin(4));
        let else_stmt = builder.assign(VarName::new("r"), one_hundred, origin(4));

        let branch = builder.if_else(cond, vec![then_stmt], vec![else_stmt], origin(2));
        let program = builder.build(vec![branch]);

        let (final_state, _) = run_to_exit(&program, &state(&[("n", n)]));
        assert_eq!(
            final_state.get("r"),
            Some(&expected),
            "n = {n} took the wrong branch"
        );
    }
}

// ---------------------------------------------------------------------------
// criterion 2: the two counted-loop traps
// ---------------------------------------------------------------------------

/// A body that grows the endpoint must not lengthen the loop.
///
/// `for i in range(0, n): n = n + 10` runs `n` times, not forever. The
/// expected values are computed here by hand: with `n = 3` the loop runs three
/// times, `n` ends at `33`, and `i` ends at `2`.
///
/// This is the direction that is unsound *and unbounded*: an ITS that re-read
/// `n` each iteration would not terminate, and the derived bound would be
/// `omega` where the truth is linear.
#[test]
fn a_counted_loop_evaluates_its_endpoint_once() {
    let mut builder = SourceProgramBuilder::new("growing", origin(1), vec![VarName::new("n")]);
    let start = builder.int(0, origin(2));
    let stop = builder.var(VarName::new("n"), origin(2));

    let read = builder.var(VarName::new("n"), origin(3));
    let ten = builder.int(10, origin(3));
    let grown = builder.arith(ArithOp::Add, read, ten, origin(3));
    let body = builder.assign(VarName::new("n"), grown, origin(3));

    let loop_stmt = builder.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, NonZeroI64::new(1).expect("one is not zero")),
        vec![body],
        origin(2),
    );
    let program = builder.build(vec![loop_stmt]);

    let (final_state, _) = run_to_exit(&program, &state(&[("n", 3)]));
    assert_eq!(
        final_state.get("n"),
        Some(&33),
        "the loop ran the wrong number of times"
    );
    assert_eq!(
        final_state.get("i"),
        Some(&2),
        "the loop variable ended wrong"
    );
}

/// A body that writes the loop variable must not shorten the loop.
///
/// `for i in range(0, 3): i = 100` runs three times. Counting on the visible
/// loop variable would run it once, which admits *fewer* executions than the
/// program has — the unsound direction.
#[test]
fn a_counted_loop_does_not_count_on_the_loop_variable() {
    let mut builder = SourceProgramBuilder::new("clobber", origin(1), vec![VarName::new("n")]);
    let start = builder.int(0, origin(2));
    let stop = builder.int(3, origin(2));

    let hundred = builder.int(100, origin(3));
    let clobber = builder.assign(VarName::new("i"), hundred, origin(3));

    let read = builder.var(VarName::new("t"), origin(4));
    let one = builder.int(1, origin(4));
    let bumped = builder.arith(ArithOp::Add, read, one, origin(4));
    let tally = builder.assign(VarName::new("t"), bumped, origin(4));

    let loop_stmt = builder.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, NonZeroI64::new(1).expect("one is not zero")),
        vec![clobber, tally],
        origin(2),
    );
    let program = builder.build(vec![loop_stmt]);

    let (final_state, _) = run_to_exit(&program, &state(&[("n", 0), ("t", 0)]));
    assert_eq!(
        final_state.get("t"),
        Some(&3),
        "the loop body ran the wrong number of times"
    );
}

/// A descending range guards the other way.
///
/// `for i in range(5, 0, -2)` visits 5, 3, 1 — three iterations. Written out
/// rather than derived, because the sign test is exactly the thing a symbolic
/// step could not do and is therefore worth pinning.
#[test]
fn a_descending_range_counts_down() {
    let mut builder = SourceProgramBuilder::new("down", origin(1), vec![VarName::new("n")]);
    let start = builder.int(5, origin(2));
    let stop = builder.int(0, origin(2));

    let read = builder.var(VarName::new("t"), origin(3));
    let one = builder.int(1, origin(3));
    let bumped = builder.arith(ArithOp::Add, read, one, origin(3));
    let tally = builder.assign(VarName::new("t"), bumped, origin(3));

    let loop_stmt = builder.for_range(
        VarName::new("i"),
        RangeSpec::new(
            start,
            stop,
            NonZeroI64::new(-2).expect("minus two is not zero"),
        ),
        vec![tally],
        origin(2),
    );
    let program = builder.build(vec![loop_stmt]);

    let (final_state, _) = run_to_exit(&program, &state(&[("n", 0), ("t", 0)]));
    assert_eq!(
        final_state.get("t"),
        Some(&3),
        "5, 3, 1 is three iterations"
    );
    assert_eq!(
        final_state.get("i"),
        Some(&1),
        "the last value visited is 1"
    );
}

/// **Criterion 2, nested loops.** The KoAT worked example's shape: an outer
/// loop over `n` and an inner loop that doubles, so the body runs
/// `n * (log2(n) + 1)` times.
///
/// The expected count is written out by hand for `n = 8`: the inner loop runs
/// with `j` at 1, 2, 4 — three iterations — for each of eight outer passes, so
/// twenty-four.
#[test]
fn nested_loops_multiply_their_trip_counts() {
    let mut builder = SourceProgramBuilder::new("koat", origin(1), vec![VarName::new("n")]);

    // j = 1
    let one = builder.int(1, origin(3));
    let seed = builder.assign(VarName::new("j"), one, origin(3));

    // while j < n: t = t + 1; j = j * 2
    let read_j = builder.var(VarName::new("j"), origin(4));
    let read_n = builder.var(VarName::new("n"), origin(4));
    let test = builder.compare(CompareOp::Lt, read_j, read_n, origin(4));

    let read_t = builder.var(VarName::new("t"), origin(5));
    let plus_one = builder.int(1, origin(5));
    let bumped = builder.arith(ArithOp::Add, read_t, plus_one, origin(5));
    let tally = builder.assign(VarName::new("t"), bumped, origin(5));

    let read_j2 = builder.var(VarName::new("j"), origin(6));
    let two = builder.int(2, origin(6));
    let doubled = builder.arith(ArithOp::Mul, read_j2, two, origin(6));
    let double = builder.assign(VarName::new("j"), doubled, origin(6));

    let inner = builder.while_loop(test, vec![tally, double], origin(4));

    // for i in range(0, n): <seed; inner>
    let start = builder.int(0, origin(2));
    let stop = builder.var(VarName::new("n"), origin(2));
    let outer = builder.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, NonZeroI64::new(1).expect("one is not zero")),
        vec![seed, inner],
        origin(2),
    );
    let program = builder.build(vec![outer]);

    let (final_state, _) = run_to_exit(&program, &state(&[("n", 8), ("t", 0), ("j", 0)]));
    assert_eq!(
        final_state.get("t"),
        Some(&24),
        "eight outer passes times three inner iterations"
    );
}

// ---------------------------------------------------------------------------
// the edges of the encoding
// ---------------------------------------------------------------------------

/// A loop whose exit is unreachable emits no exit transition, rather than one
/// guarded by a contradiction.
///
/// `while 1 != 0` is the fragment's spelling of `while True`. Its positive
/// polarity is trivially true and its negative one trivially false, so the
/// exit edge is dropped — which is correct, and is the *only* place this
/// lowering discards a transition. Dropping a transition no execution can take
/// removes no execution.
#[test]
fn an_unsatisfiable_exit_guard_is_dropped_rather_than_emitted() {
    let mut builder = SourceProgramBuilder::new("forever", origin(1), vec![]);
    let one = builder.int(1, origin(2));
    let zero = builder.int(0, origin(2));
    let always = builder.compare(CompareOp::Ne, one, zero, origin(2));
    let loop_stmt = builder.while_loop(always, vec![], origin(2));
    let program = builder.build(vec![loop_stmt]);

    let its = lower(&program).expect("inside the fragment");

    let reaches_exit = its
        .transitions()
        .iter()
        .any(|transition| transition.target() == its.exit());
    assert!(
        !reaches_exit,
        "an always-true loop must not have a reachable exit: {}",
        its.to_koat()
    );
    assert!(
        its.transitions()
            .iter()
            .all(|transition| { !transition.guard().is_trivially_unsatisfiable() }),
        "a transition was emitted with a contradictory guard"
    );
}

/// Integer arithmetic that leaves `i64` refuses rather than wrapping.
///
/// Wrapping would turn one program into a different one, and a bound derived
/// from a different program can be exceeded. It is therefore a refusal like
/// any other construct outside the fragment, and it names itself.
#[test]
fn coefficient_overflow_refuses_rather_than_wrapping() {
    let mut builder = SourceProgramBuilder::new("huge", origin(1), vec![]);
    let big = builder.int(i64::MAX, origin(2));
    let also_big = builder.int(i64::MAX, origin(2));
    let sum = builder.arith(ArithOp::Add, big, also_big, origin(2));
    let assign = builder.assign(VarName::new("x"), sum, origin(2));
    let program = builder.build(vec![assign]);

    let error = lower(&program).expect_err("i64::MAX + i64::MAX must not lower");
    let refusals = error
        .refusals()
        .expect("overflow is a refusal, not malformed");
    assert!(
        refusals
            .constructs()
            .contains(&Construct::ArithmeticOverflow),
        "overflow must name itself: {refusals}"
    );
}

/// A condition whose normal form exceeds [`landav_its::MAX_DNF_CLAUSES`] is
/// **widened to true**, which makes both branches available.
///
/// That is the safe direction: the system admits executions the program does
/// not have, so a bound derived from it is still an upper bound. The test
/// asserts both halves — that the widening actually happened (an unguarded
/// edge into the consequent) and that the real execution is still among the
/// admitted ones.
#[test]
fn a_condition_past_the_clause_cap_widens_to_true() {
    let mut builder = SourceProgramBuilder::new("wide", origin(1), vec![VarName::new("n")]);

    // n != 0 and n != 1 and ... and n != 6 — seven `!=`s, so 2^7 = 128
    // clauses positively, past the cap of 64.
    let mut conjunction = None;
    for value in 0..7 {
        let read = builder.var(VarName::new("n"), origin(2));
        let literal = builder.int(value, origin(2));
        let comparison = builder.compare(CompareOp::Ne, read, literal, origin(2));
        conjunction = Some(match conjunction {
            None => comparison,
            Some(previous) => builder.and(previous, comparison, origin(2)),
        });
    }
    let cond = conjunction.expect("seven conjuncts were built");

    let hit = builder.int(1, origin(3));
    let then_stmt = builder.assign(VarName::new("r"), hit, origin(3));
    let miss = builder.int(2, origin(4));
    let else_stmt = builder.assign(VarName::new("r"), miss, origin(4));
    let branch = builder.if_else(cond, vec![then_stmt], vec![else_stmt], origin(2));
    let program = builder.build(vec![branch]);

    let its = lower(&program).expect("widening is not a refusal");

    let widened = its
        .transitions_from(its.start())
        .chain(its.transitions())
        .any(|transition| transition.guard().is_always());
    assert!(
        widened,
        "the condition should have been widened to an unguarded edge: {}",
        its.to_koat()
    );

    // And the real execution is still admitted. At n = 3 the condition holds,
    // so the program sets r = 1; the widened system must have a run doing so.
    use crate::reference::explore;
    let exploration = explore(&its, &state(&[("n", 3)]), 5_000, 500);
    assert!(
        exploration.visited() > 2,
        "the exploration was trivial, so `admits` below proves little"
    );
    assert!(
        exploration.admits(&state(&[("n", 3), ("r", 1)]), 0),
        "the widened system lost the execution the program actually has: {:?}",
        exploration.exit_states()
    );
}

/// Every comparison's negation is its own inverse.
///
/// The lowering computes both polarities of a condition independently, and
/// this is the algebraic fact that makes doing so safe. Cheap to state and it
/// pins a table that is easy to mistype.
#[test]
fn comparison_negation_is_an_involution() {
    for op in crate::reference::all_comparisons() {
        assert_eq!(
            op.negate().negate(),
            op,
            "{op} is not its own double negation"
        );
        for left in -3_i128..=3 {
            for right in -3_i128..=3 {
                assert_ne!(
                    op.holds(left, right),
                    op.negate().holds(left, right),
                    "{op} and its negation agreed at ({left}, {right})"
                );
            }
        }
    }
}

/// The operator spellings are pinned, one by one.
///
/// These tables are rendered into diagnostics and, for [`Relation`], into the
/// emitted KoAT text. Nothing else in the suite reads them back — a function
/// returning one constant for every operator would satisfy every other
/// assertion here, which is precisely the shape of mutant that survives an
/// otherwise strong suite. Pinning them is two lines and closes that class.
#[test]
fn the_operator_spellings_are_pinned() {
    use landav_its::{ArithOp, Relation};

    assert_eq!(ArithOp::Add.as_str(), "+");
    assert_eq!(ArithOp::Sub.as_str(), "-");
    assert_eq!(ArithOp::Mul.as_str(), "*");
    assert_eq!(ArithOp::Add.to_string(), "+");
    assert_eq!(ArithOp::Mul.to_string(), "*");

    assert_eq!(CompareOp::Lt.as_str(), "<");
    assert_eq!(CompareOp::Le.as_str(), "<=");
    assert_eq!(CompareOp::Gt.as_str(), ">");
    assert_eq!(CompareOp::Ge.as_str(), ">=");
    assert_eq!(CompareOp::Eq.as_str(), "==");
    assert_eq!(CompareOp::Ne.as_str(), "!=");
    assert_eq!(CompareOp::Ne.to_string(), "!=");

    // These three reach the emitted document, so a wrong spelling is a file
    // KoAT would reject rather than merely a confusing message.
    assert_eq!(Relation::Ge.as_str(), ">=");
    assert_eq!(Relation::Gt.as_str(), ">");
    assert_eq!(Relation::Eq.as_str(), "=");
    assert_eq!(Relation::Ge.to_string(), ">=");
}

/// Each comparison's *negation table* is pinned, not merely its involution.
///
/// `comparison_negation_is_an_involution` proves the table is a permutation
/// pairing each operator with a genuine complement, but a table that mapped
/// `Lt` to `Gt` and `Le` to `Ge` would also be an involution — and wrong.
#[test]
fn the_negation_table_is_pinned() {
    assert_eq!(CompareOp::Lt.negate(), CompareOp::Ge);
    assert_eq!(CompareOp::Le.negate(), CompareOp::Gt);
    assert_eq!(CompareOp::Gt.negate(), CompareOp::Le);
    assert_eq!(CompareOp::Ge.negate(), CompareOp::Lt);
    assert_eq!(CompareOp::Eq.negate(), CompareOp::Ne);
    assert_eq!(CompareOp::Ne.negate(), CompareOp::Eq);
}

/// A `Location`'s label and rendered name survive into the system.
///
/// The labels are what makes an emitted document readable when a bound comes
/// back wrong; nothing else asserts they are distinct or non-empty.
#[test]
fn locations_carry_labels_that_name_their_construct() {
    let mut builder = SourceProgramBuilder::new("labelled", origin(1), vec![VarName::new("n")]);
    let read = builder.var(VarName::new("n"), origin(2));
    let zero = builder.int(0, origin(2));
    let cond = builder.compare(CompareOp::Gt, read, zero, origin(2));
    let body = builder.assign(VarName::new("x"), zero, origin(3));
    let loop_stmt = builder.while_loop(cond, vec![body], origin(2));
    let program = builder.build(vec![loop_stmt]);

    let its = lower(&program).expect("inside the fragment");
    let labels: Vec<&str> = its
        .locations()
        .iter()
        .map(|location| location.label().as_str())
        .collect();

    assert!(labels.contains(&"entry"), "{labels:?}");
    assert!(labels.contains(&"exit"), "{labels:?}");
    assert!(labels.contains(&"while.head"), "{labels:?}");
    assert!(labels.contains(&"while.body"), "{labels:?}");
    assert!(
        its.location(its.start())
            .is_some_and(|l| l.rendered_name() == "l0"),
        "the start location renders as l0"
    );
    // Every location the system reports is findable by its own identity.
    for location in its.locations() {
        assert_eq!(
            its.location(location.id()).map(Location::label),
            Some(location.label()),
            "a location could not be found by its own identity"
        );
    }
}

/// **A transition renders as the whole step: both endpoints, guard and
/// update.**
///
/// The suite reads transitions through their accessors and through
/// `Its::to_koat`, never through [`landav_its::Transition`]'s own `Display` —
/// so replacing that `Display` with one that writes *nothing at all* passed
/// every test. The same held for [`landav_its::VarName`].
///
/// These two renderings are the ones a person reads when a lowering surprises
/// them: a panic message, a `{:?}`-free assertion failure, a debug dump. A
/// silent `Display` turns each of those into an empty string at precisely the
/// moment it is needed, and nothing else in the crate notices.
#[test]
fn a_transition_renders_as_a_readable_step() {
    let mut builder = SourceProgramBuilder::new(
        "step",
        origin(1),
        vec![VarName::new("n"), VarName::new("i")],
    );
    let i = builder.var(VarName::new("i"), origin(2));
    let n = builder.var(VarName::new("n"), origin(2));
    let one = builder.int(1, origin(2));
    let guard = builder.compare(CompareOp::Lt, i, n, origin(2));
    let next = builder.arith(ArithOp::Add, i, one, origin(3));
    let bump = builder.assign(VarName::new("i"), next, origin(3));
    let loop_stmt = builder.while_loop(guard, vec![bump], origin(2));
    let program = builder.build(vec![loop_stmt]);

    let its = lower(&program).expect("a counted loop is inside the fragment");

    for transition in its.transitions() {
        let rendered = transition.to_string();
        assert!(
            !rendered.is_empty(),
            "a transition rendered as nothing at all"
        );
        // Two arrow spellings, both meaning "and then": a bare `->` when the
        // step costs one, and `-{c}>` when it costs anything else. The
        // annotated form is not decoration - a transition the lowering
        // invented costs nothing, and hiding that would make the rendering
        // disagree with what is emitted to the solver.
        assert!(
            rendered.contains("->") || rendered.contains("}>"),
            "a transition must render both of its endpoints: {rendered:?}"
        );
        assert!(
            rendered.contains(&transition.source().to_string())
                && rendered.contains(&transition.target().to_string()),
            "a transition named endpoints it does not have: {rendered:?}"
        );
        assert!(
            rendered.contains(&transition.update().to_string()),
            "a transition dropped its update: {rendered:?}"
        );
    }

    // The two steps worth naming: one that carries a real guard, and one that
    // carries a real update. Between them every part of the rendering is
    // something other than a default, so a `Display` that dropped any one part
    // would show here.
    let guarded = its
        .transitions()
        .iter()
        .find(|transition| !transition.guard().is_always())
        .expect("the loop header is guarded by i < n");
    let rendered = guarded.to_string();
    let guard_text = guarded.guard().to_string();
    assert!(
        rendered.contains(&guard_text),
        "the guard {guard_text:?} is missing from {rendered:?}"
    );
    assert!(
        rendered.find(&guarded.source().to_string()) < rendered.find("->"),
        "the source must be rendered before the arrow: {rendered:?}"
    );
    assert!(
        rendered.find("->") < rendered.find(&guard_text),
        "the guard must be rendered after the endpoints: {rendered:?}"
    );

    let assigning = its
        .transitions()
        .iter()
        .find(|transition| !transition.update().is_identity())
        .expect("the loop body increments i");
    let rendered = assigning.to_string();
    let update_text = assigning.update().to_string();
    assert!(
        update_text.contains(":="),
        "a non-identity update must render as an assignment: {update_text:?}"
    );
    assert!(
        rendered.contains(&update_text),
        "the update {update_text:?} is missing from {rendered:?}"
    );
    assert!(
        rendered.find("->") < rendered.find(&update_text),
        "the update must be rendered after the endpoints: {rendered:?}"
    );

    // Two different transitions must not render alike.
    let renderings: std::collections::BTreeSet<String> = its
        .transitions()
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    assert!(
        renderings.len() > 1,
        "every transition in a loop rendered identically: {renderings:?}"
    );
}

/// A variable name renders as the name the frontend spelled.
///
/// Trivial, and it survived mutation as a `Display` that writes nothing:
/// every assertion in the suite reached the name through `as_str` instead.
#[test]
fn a_variable_name_renders_as_itself() {
    for spelling in ["x", "counter", "n_items", "\u{e9}l\u{e8}ve"] {
        let name = VarName::new(spelling);
        assert_eq!(name.to_string(), spelling);
        assert_eq!(name.to_string(), name.as_str());
        assert!(!name.to_string().is_empty());
    }
    assert_ne!(
        VarName::new("x").to_string(),
        VarName::new("y").to_string(),
        "two different names must not render alike"
    );
}

/// **A range's stride is never zero**, which is why `RangeSpec::ascending`'s
/// `step > 0` and `step >= 0` cannot be separated by any test.
///
/// The stride is a [`NonZeroI64`], so the two comparisons agree on every value
/// the type admits and the mutation is equivalent. That is a property of the
/// *type*, and it is the reason the surviving mutant is acceptable — so it is
/// pinned here, and a future change to a plain `i64` fails this instead of
/// quietly turning `range(0, 10, 0)` into an ascending loop that never
/// advances.
#[test]
fn a_range_stride_cannot_be_zero_so_ascending_is_total() {
    assert!(NonZeroI64::new(0).is_none(), "the stride type refuses zero");

    let mut builder = SourceProgramBuilder::new("strides", origin(1), vec![]);
    let start = builder.int(0, origin(2));
    let stop = builder.int(10, origin(2));

    for step in [1_i64, 2, 7, i64::MAX] {
        let stride = NonZeroI64::new(step).expect("non-zero");
        assert!(
            RangeSpec::new(start, stop, stride).ascending(),
            "a positive stride ascends"
        );
    }
    for step in [-1_i64, -3, i64::MIN] {
        let stride = NonZeroI64::new(step).expect("non-zero");
        assert!(
            !RangeSpec::new(start, stop, stride).ascending(),
            "a negative stride descends"
        );
    }
}
