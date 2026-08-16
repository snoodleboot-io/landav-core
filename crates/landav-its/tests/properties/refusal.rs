//! `LAN-67` criterion 4: **an unsupported construct produces an explicit
//! diagnostic, never silent truncation.**
//!
//! This is the soundness-critical criterion. Silently dropping a construct
//! produces a bound that is *wrong* rather than absent, and a wrong bound is
//! the single failure class with a zero target. The story asks that truncation
//! be impossible **by construction**, so the tests here check the construction
//! and not merely a sample of behaviours:
//!
//! * every [`Construct`] in the vocabulary refuses, and names itself;
//! * a refusal names a **position**, so it can be acted on;
//! * a refused program yields **no** transition system, not a partial one;
//! * **every** refusal is reported, not just the first;
//! * a refusal in code the control flow cannot reach is still reported;
//! * the whole thing survives input deep enough to overflow a recursive
//!   traversal's stack, which is non-negotiable 2.

use landav_bound::Origin;
use landav_its::{
    ArithOp, CompareOp, Construct, LoweringError, SourceProgramBuilder, VarName, lower,
};

fn origin(line: u32) -> Origin {
    Origin::new(format!("refused.py:{line}:1"))
}

/// **Every construct in the vocabulary refuses, and names itself.**
///
/// Driven off [`Construct::all`] rather than a hand-written list, so a variant
/// added later without a lowering rule fails here rather than being quietly
/// forgotten.
#[test]
fn every_construct_refuses_and_names_itself() {
    for construct in Construct::all() {
        let mut builder = SourceProgramBuilder::new("refuses", origin(1), vec![]);
        let offending = builder.unsupported_stmt(*construct, origin(7));
        let program = builder.build(vec![offending]);

        let error = match lower(&program) {
            Ok(_) => panic!("{construct} lowered instead of refusing"),
            Err(error) => error,
        };
        let refusals = match error.refusals() {
            Some(refusals) => refusals,
            None => panic!("{construct} produced a malformed error rather than a refusal"),
        };

        assert!(
            refusals.constructs().contains(construct),
            "refusing {construct} did not name it: {refusals}"
        );
        assert!(
            refusals
                .as_slice()
                .iter()
                .any(|record| record.origin().as_str() == "refused.py:7:1"),
            "refusing {construct} did not name a position: {refusals}"
        );
        assert!(
            error.to_string().contains(construct.tag()),
            "the rendered error for {construct} does not mention its tag: {error}"
        );
    }
}

/// Every construct has a distinct tag and a distinct description.
///
/// The tags are the codes a coverage report groups by, so two constructs
/// sharing one would silently merge two rows.
#[test]
fn the_diagnostic_vocabulary_has_no_duplicates() {
    let mut tags: Vec<&str> = Construct::all().iter().map(|c| c.tag()).collect();
    let total = tags.len();
    tags.sort_unstable();
    tags.dedup();
    assert_eq!(tags.len(), total, "two constructs share a tag");

    let mut descriptions: Vec<&str> = Construct::all().iter().map(|c| c.describe()).collect();
    descriptions.sort_unstable();
    descriptions.dedup();
    assert_eq!(
        descriptions.len(),
        total,
        "two constructs share a description"
    );

    assert!(
        Construct::all().iter().all(|c| !c.tag().is_empty()),
        "a construct has an empty tag"
    );
    assert!(
        Construct::all().iter().all(|c| !c.describe().is_empty()),
        "a construct has an empty description"
    );

    // `Display` is the tag, and a report may render either. If they drifted
    // apart, two spellings of one diagnostic code would reach a baseline.
    for construct in Construct::all() {
        assert_eq!(
            construct.to_string(),
            construct.tag(),
            "{construct:?} renders differently from its tag"
        );
    }
}

/// **No partial system.** A program that is nine tenths inside the fragment
/// still lowers to nothing.
///
/// This is the decision that makes the refusal sound. A system built from the
/// understood part admits fewer executions than the program has — the refused
/// construct might have been a loop — and a bound derived from it can be
/// exceeded.
#[test]
fn a_refused_program_yields_no_transition_system() {
    let mut builder = SourceProgramBuilder::new("mostly_fine", origin(1), vec![VarName::new("n")]);

    // Nine perfectly good statements...
    let mut statements = Vec::new();
    for line in 2..11 {
        let read = builder.var(VarName::new("n"), origin(line));
        let one = builder.int(1, origin(line));
        let sum = builder.arith(ArithOp::Add, read, one, origin(line));
        statements.push(builder.assign(VarName::new("n"), sum, origin(line)));
    }
    // ...and one that is not.
    statements.push(builder.unsupported_stmt(Construct::Call, origin(11)));

    let program = builder.build(statements);
    assert!(
        lower(&program).is_err(),
        "a program containing one refused construct must not lower at all"
    );
}

/// **Every refusal, not just the first.**
///
/// A lowering that stopped at the first would make a coverage report a lie by
/// omission, and would turn adoption into a game of fix-one-discover-the-next.
#[test]
fn every_refusal_is_reported_not_only_the_first() {
    let mut builder = SourceProgramBuilder::new("several", origin(1), vec![]);
    let first = builder.unsupported_stmt(Construct::Call, origin(2));
    let second = builder.unsupported_stmt(Construct::Subscript, origin(3));
    let third = builder.unsupported_stmt(Construct::Comprehension, origin(4));
    let program = builder.build(vec![first, second, third]);

    let error = lower(&program).expect_err("three refusals");
    let refusals = error.refusals().expect("a refusal, not malformed");

    assert_eq!(
        refusals.len(),
        3,
        "only some refusals were reported: {refusals}"
    );
    for construct in [
        Construct::Call,
        Construct::Subscript,
        Construct::Comprehension,
    ] {
        assert_eq!(
            refusals.count_of(construct),
            1,
            "{construct} was not reported exactly once"
        );
    }
    assert_eq!(refusals.constructs().len(), 3);
}

/// A refusal inside an expression, a condition and a statement all reach the
/// ledger.
///
/// Three different traversals reach three different arenas, and a refusal
/// found by one of them is easy to lose.
#[test]
fn refusals_are_found_in_expressions_conditions_and_statements() {
    let mut builder = SourceProgramBuilder::new("everywhere", origin(1), vec![]);

    // An expression-level refusal, inside an assignment.
    let bad_value = builder.unsupported_expr(Construct::Attribute, origin(2));
    let assign = builder.assign(VarName::new("x"), bad_value, origin(2));

    // A condition-level refusal, in an `if`.
    let bad_cond = builder.unsupported_cond(Construct::Call, origin(3));
    let branch = builder.if_else(bad_cond, vec![], vec![], origin(3));

    // A statement-level refusal.
    let bad_stmt = builder.unsupported_stmt(Construct::Coroutine, origin(4));

    let program = builder.build(vec![assign, branch, bad_stmt]);
    let error = lower(&program).expect_err("three refusals");
    let refusals = error.refusals().expect("a refusal");

    for construct in [Construct::Attribute, Construct::Call, Construct::Coroutine] {
        assert!(
            refusals.constructs().contains(&construct),
            "{construct} was not reported: {refusals}"
        );
    }
}

/// A refusal in unreachable code is still reported.
///
/// "We did not look" and "we looked and it was fine" are different answers.
/// Reporting only what the control flow reaches would make the coverage metric
/// depend on reachability analysis this crate does not do, and would let a
/// construct hide behind an early `return`.
#[test]
fn a_refusal_after_a_return_is_still_reported() {
    let mut builder = SourceProgramBuilder::new("unreachable", origin(1), vec![]);
    let early = builder.return_stmt(origin(2));
    let hidden = builder.unsupported_stmt(Construct::Comprehension, origin(3));
    let program = builder.build(vec![early, hidden]);

    let error = lower(&program).expect_err("the hidden construct must still refuse");
    let refusals = error.refusals().expect("a refusal");
    assert!(refusals.constructs().contains(&Construct::Comprehension));
}

/// A refusal carries blame in the `landav-bound` vocabulary too.
///
/// The `F-015` seam: a caller that wants to publish a partial bound rather
/// than nothing gets a non-empty [`landav_bound::Blames`] without this crate
/// depending on the bound algebra's report types.
#[test]
fn a_refusal_converts_to_a_non_empty_blame_ledger() {
    let mut builder = SourceProgramBuilder::new("blamed", origin(1), vec![]);
    let offending = builder.unsupported_stmt_detailed(Construct::Call, "sorted", origin(5));
    let program = builder.build(vec![offending]);

    let error = lower(&program).expect_err("a refusal");
    let blames = error.blames();
    assert!(!blames.is_empty(), "the blame ledger is empty");

    let record = blames.as_slice().first().expect("non-empty");
    assert_eq!(record.origin.as_str(), "refused.py:5:1");
    assert_eq!(record.unaccounted.as_str(), Construct::Call.tag());

    // The frontend-supplied detail survives into the assumption.
    assert!(
        format!("{:?}", record.assumption).contains("sorted"),
        "the detail was lost: {:?}",
        record.assumption
    );
}

/// A malformed program is reported separately from a refusal.
///
/// A coverage report that counted frontend bugs as unsupported language
/// constructs would send someone to write a lowering rule for a construct that
/// does not exist.
#[test]
fn a_malformed_program_is_not_reported_as_an_unsupported_construct() {
    // A handle from one program used against another.
    let mut first = SourceProgramBuilder::new("first", origin(1), vec![]);
    let stranger = first.int(1, origin(2));

    let mut second = SourceProgramBuilder::new("second", origin(1), vec![]);
    let assign = second.assign(VarName::new("x"), stranger, origin(2));
    let program = second.build(vec![assign]);

    match lower(&program) {
        Err(LoweringError::Malformed { function, detail }) => {
            assert_eq!(function.as_str(), "second");
            assert!(
                !detail.as_str().is_empty(),
                "a malformed error must say why"
            );
        }
        Err(LoweringError::Refused { refusals, .. }) => {
            panic!("a frontend bug was reported as an unsupported construct: {refusals}")
        }
        Err(other) => panic!("unexpected lowering error: {other}"),
        Ok(_) => panic!("a handle naming no node must not lower"),
    }
}

/// **Non-negotiable 2.** Input deep enough to overflow a recursive traversal
/// is lowered without incident.
///
/// A stack overflow is an abort, not a panic: `unwrap_used`, `panic` and
/// `forbid(unsafe_code)` cannot see it, and it destroys the blame path that
/// makes a partial result useful. The arena representation plus the worklist
/// traversals are what make this pass; a `Box`-linked expression tree would
/// die here on the way in, on the way through, **or** in `Drop`.
#[test]
fn deep_input_does_not_overflow_the_stack() {
    const DEPTH: usize = 200_000;

    let mut builder = SourceProgramBuilder::new("deep", origin(1), vec![]);
    let mut expression = builder.int(1, origin(2));
    for _ in 0..DEPTH {
        let one = builder.int(1, origin(2));
        expression = builder.arith(ArithOp::Add, expression, one, origin(2));
    }
    let assign = builder.assign(VarName::new("x"), expression, origin(2));
    let program = builder.build(vec![assign]);

    let its = lower(&program).expect("a very long sum is still a polynomial");
    let update = its
        .transitions()
        .iter()
        .find_map(|transition| transition.update().get(&landav_its::ItsVar::new("x")))
        .expect("x is assigned");
    assert_eq!(
        update.as_constant(),
        i64::try_from(DEPTH + 1).ok(),
        "the sum folded to the wrong constant"
    );

    // And dropping it must not overflow either, which is the hazard that gets
    // missed: it is generated code that no test names.
    drop(program);
}

/// Deeply nested *conditions* are lowered without incident too.
///
/// A separate traversal from the expression one, with its own stack.
#[test]
fn deeply_nested_conditions_do_not_overflow_the_stack() {
    const DEPTH: usize = 50_000;

    let mut builder = SourceProgramBuilder::new("deep_cond", origin(1), vec![VarName::new("n")]);
    let read = builder.var(VarName::new("n"), origin(2));
    let zero = builder.int(0, origin(2));
    let mut condition = builder.compare(CompareOp::Gt, read, zero, origin(2));
    for _ in 0..DEPTH {
        condition = builder.not(condition, origin(2));
    }
    let branch = builder.if_else(condition, vec![], vec![], origin(2));
    let program = builder.build(vec![branch]);

    assert!(
        lower(&program).is_ok(),
        "a deeply negated condition should lower"
    );
    drop(program);
}

/// Deeply nested *statements* are lowered without incident.
#[test]
fn deeply_nested_statements_do_not_overflow_the_stack() {
    const DEPTH: usize = 20_000;

    let mut builder = SourceProgramBuilder::new("deep_stmt", origin(1), vec![VarName::new("n")]);
    let read = builder.var(VarName::new("n"), origin(2));
    let zero = builder.int(0, origin(2));
    let condition = builder.compare(CompareOp::Gt, read, zero, origin(2));

    let one = builder.int(1, origin(3));
    let mut body = vec![builder.assign(VarName::new("x"), one, origin(3))];
    for _ in 0..DEPTH {
        body = vec![builder.if_else(condition, body, vec![], origin(2))];
    }
    let program = builder.build(body);

    assert!(lower(&program).is_ok(), "deeply nested ifs should lower");
    drop(program);
}

/// A builder fed past [`landav_its::MAX_ARENA_NODES`] refuses rather than
/// panicking or allocating without limit.
#[test]
fn an_overflowing_arena_is_refused_rather_than_truncated_silently() {
    let mut builder = SourceProgramBuilder::new("enormous", origin(1), vec![]);
    for _ in 0..(landav_its::MAX_ARENA_NODES + 8) {
        let _ = builder.int(1, origin(2));
    }
    let value = builder.int(1, origin(2));
    let assign = builder.assign(VarName::new("x"), value, origin(2));
    let program = builder.build(vec![assign]);

    assert!(program.overflowed(), "the overflow flag was not set");
    match lower(&program) {
        Err(LoweringError::Malformed { .. }) => {}
        other => panic!("an overflowed arena must not lower: {other:?}"),
    }
}

/// **A refusal that nothing points at is still reported.**
///
/// The most likely way for a frontend to lose a refusal is not to mishandle it
/// but to *drop it on the floor*: build the `Unsupported` node, then find there
/// is nowhere to attach it. `return f()` is the case that forces this — the
/// call must be refused, and this fragment's `return` carries no expression, so
/// the node the frontend builds for `f()` has no parent.
///
/// Lowering scans the arenas rather than relying on the traversal, so an
/// unattached node refuses exactly like an attached one. Without that, this
/// program would lower cleanly and the derived bound would silently omit the
/// call.
#[test]
fn an_unattached_refusal_is_still_reported() {
    let mut builder = SourceProgramBuilder::new("orphan", origin(1), vec![]);
    // Built, and deliberately never referenced by any statement.
    let _orphan = builder.unsupported_expr_detailed(Construct::Call, "expensive", origin(9));
    let only = builder.return_stmt(origin(9));
    let program = builder.build(vec![only]);

    let error = lower(&program).expect_err("an unattached refusal must still refuse");
    let refusals = error.refusals().expect("a refusal");
    assert!(
        refusals.constructs().contains(&Construct::Call),
        "the unattached node was dropped: {refusals}"
    );
    assert!(
        refusals
            .as_slice()
            .iter()
            .any(|record| record.origin().as_str() == "refused.py:9:1"),
        "the unattached refusal lost its position: {refusals}"
    );
}

/// The same, for an unattached condition.
#[test]
fn an_unattached_condition_refusal_is_still_reported() {
    let mut builder = SourceProgramBuilder::new("orphan_cond", origin(1), vec![]);
    let _orphan = builder.unsupported_cond(Construct::Subscript, origin(4));
    let only = builder.return_stmt(origin(5));
    let program = builder.build(vec![only]);

    let error = lower(&program).expect_err("an unattached condition must still refuse");
    let refusals = error.refusals().expect("a refusal");
    assert!(refusals.constructs().contains(&Construct::Subscript));
}

/// **A self-referential handle is refused, and does not hang.**
///
/// [`landav_its::ExprId`]s cannot be forged, but they *can* be moved between
/// programs — and a handle whose index is in range for the receiving arena can
/// name the very node that holds it. That is a cycle, and a traversal without
/// the "a child's index precedes its parent's" check would follow it forever.
/// An infinite loop is worse than a panic: nothing times it out and no lint
/// sees it.
///
/// Constructed here by taking the sixth expression of one program and using it
/// as an operand of the sixth expression of another.
#[test]
fn a_self_referential_handle_is_refused_rather_than_followed() {
    let mut donor = SourceProgramBuilder::new("donor", origin(1), vec![]);
    let stolen: Vec<_> = (0..6).map(|value| donor.int(value, origin(2))).collect();
    let sixth = stolen.get(5).copied().expect("six were built");
    assert_eq!(sixth.index(), 5, "the sixth handle names index five");

    // Five nodes first, so the operation built next lands at index five — the
    // very index the stolen handle names.
    let mut builder = SourceProgramBuilder::new("cyclic", origin(1), vec![]);
    let mut filler = None;
    for _ in 0..5 {
        filler = Some(builder.int(1, origin(2)));
    }
    let other = filler.expect("five were built");
    let looped = builder.arith(ArithOp::Add, sixth, other, origin(3));
    assert_eq!(looped.index(), 5, "the operation lands on the stolen index");
    let assign = builder.assign(VarName::new("x"), looped, origin(3));
    let program = builder.build(vec![assign]);

    match lower(&program) {
        Err(LoweringError::Malformed { detail, .. }) => {
            assert!(
                !detail.as_str().is_empty(),
                "a malformed error must say why"
            );
        }
        other => panic!("a self-referential operand must be refused: {other:?}"),
    }
}

/// The same, for a condition.
#[test]
fn a_self_referential_condition_handle_is_refused() {
    let mut donor = SourceProgramBuilder::new("donor", origin(1), vec![]);
    let one = donor.int(1, origin(2));
    let two = donor.int(2, origin(2));
    let stolen: Vec<_> = (0..4)
        .map(|_| donor.compare(CompareOp::Lt, one, two, origin(2)))
        .collect();
    let fourth = stolen.get(3).copied().expect("four were built");
    assert_eq!(fourth.index(), 3);

    let mut builder = SourceProgramBuilder::new("cyclic_cond", origin(1), vec![]);
    let left = builder.int(1, origin(2));
    let right = builder.int(2, origin(2));
    let mut last = None;
    for _ in 0..3 {
        last = Some(builder.compare(CompareOp::Lt, left, right, origin(2)));
    }
    let other = last.expect("three were built");
    let looped = builder.and(fourth, other, origin(3));
    assert_eq!(
        looped.index(),
        3,
        "the conjunction lands on the stolen index"
    );
    let branch = builder.if_else(looped, vec![], vec![], origin(3));
    let program = builder.build(vec![branch]);

    assert!(
        matches!(lower(&program), Err(LoweringError::Malformed { .. })),
        "a self-referential condition operand must be refused"
    );
}

/// A power expression built through the builder lowers to the right
/// polynomial.
///
/// `Polynomial::power` is exercised directly by the algebra suite, but nothing
/// reached it *through* a `SourceExpr::Pow` — so the guard on that arm was
/// unreachable from any test, and deleting it changed nothing.
#[test]
fn a_power_expression_lowers_to_a_polynomial() {
    let mut builder = SourceProgramBuilder::new("cubed", origin(1), vec![VarName::new("n")]);
    let base = builder.var(VarName::new("n"), origin(2));
    let cubed = builder.pow(base, 3, origin(2));
    let assign = builder.assign(VarName::new("x"), cubed, origin(2));
    let program = builder.build(vec![assign]);

    let its = lower(&program).expect("a literal exponent is inside the fragment");
    let update = its
        .transitions()
        .iter()
        .find_map(|transition| transition.update().get(&landav_its::ItsVar::new("x")))
        .expect("x is assigned");
    assert_eq!(update.degree(), 3, "n ** 3 is degree three: {update}");
    assert_eq!(update.to_string(), "n^3");

    // And a power past the degree cap refuses rather than grinding.
    let mut builder = SourceProgramBuilder::new("too_high", origin(1), vec![VarName::new("n")]);
    let base = builder.var(VarName::new("n"), origin(2));
    let enormous = builder.pow(base, landav_its::MAX_DEGREE + 1, origin(2));
    let assign = builder.assign(VarName::new("x"), enormous, origin(2));
    let program = builder.build(vec![assign]);

    let error = lower(&program).expect_err("past the degree cap");
    let refusals = error.refusals().expect("a refusal");
    assert!(refusals.constructs().contains(&Construct::PolynomialDegree));
}
