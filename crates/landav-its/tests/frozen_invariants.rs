//! The invariants that stand between this crate and a **hang**.
//!
//! # Why this is a separate, fast test target
//!
//! `Lowering::expr_poly` and `Lowering::cond_dnf` are worklist traversals with
//! no iteration cap of their own. They terminate for exactly one reason: every
//! edge they follow goes to a *strictly smaller* arena index, so the chain of
//! visits from a node at index `n` is at most `n` long. That fact is not a
//! property of the data — handles are `Copy` and can be moved between
//! programs, so an arena really can contain a forward reference — it is
//! enforced at the moment of following, by `expr_child_ok` and
//! `cond_child_ok`.
//!
//! Remove either guard and the traversal has nothing left bounding it. A
//! program whose node 0 names node 1 and whose node 1 names node 0 is then
//! walked forever. Mutation testing measured precisely that: all six of
//!
//! | mutant | what it removes |
//! |---|---|
//! | `expr_child_ok -> true` | the expression guard entirely |
//! | `cond_child_ok -> true` | the condition guard entirely |
//! | `delete !` in `expr_poly`'s binary arm | the guard's sense, so a good child returns early and a bad one is followed |
//! | `delete !` in `cond_dnf`'s binary arm | the same, for conditions |
//! | `\|\|` → `&&` in `expr_poly`'s binary arm | the short-circuit, so a bad **right** operand is never checked |
//! | `\|\|` → `&&` in `cond_dnf`'s binary arm | the same, for conditions |
//!
//! were killed by a 120-second clock rather than by an assertion, and a mutant
//! killed by the clock is indistinguishable from CI being slow.
//!
//! `refusal.rs` already asserts the right property — `a_self_referential_handle_is_refused_rather_than_followed`
//! builds a genuine two-node cycle and requires a [`LoweringError::Malformed`].
//! It cannot *report*, because under those six mutants it is the test that
//! hangs, and one hanging test blocks the whole binary from reporting however
//! many of its siblings already failed. That is the finding, and it is the
//! third time this milestone that a missing guard has shown up as
//! non-termination rather than as a failed assertion.
//!
//! # This file must never build a cycle
//!
//! Every malformed program below points *forward to a leaf*: node 1 names node
//! 2, and node 2 is an integer literal. With the guard in place the traversal
//! never gets there and the program is refused. With any of the six mutants
//! the traversal does get there, finds a literal, finishes in microseconds,
//! and the program **lowers successfully** — so the assertion that it must be
//! refused fails immediately instead of hanging. A forward reference is enough
//! to separate every one of the six; only a cycle costs termination, and there
//! is deliberately no cycle here.
//!
//! Cargo runs a package's integration-test targets in name order and stops at
//! the first failing target, so `frozen_invariants` completes before
//! `properties` starts.
//!
//! # Is the non-termination reachable through the public API?
//!
//! No, and this file is the record of why. [`SourceProgramBuilder`] is
//! append-only: a node's index is the number of nodes pushed before it, so a
//! handle obtained from the *same* builder always names an earlier index than
//! any node built from it. A forward reference requires moving a handle
//! between two programs, which the guards refuse. Both halves are pinned
//! below, because the guards are only load-bearing while the builder half
//! holds.

// The panic lints are relaxed in test code only, as in `properties/main.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use landav_bound::Origin;
use landav_its::{
    ArithOp, CompareOp, CondId, ExprId, ItsVar, LoweringError, Polynomial, SourceProgramBuilder,
    VarName, lower,
};

fn origin(line: u32) -> Origin {
    Origin::new(format!("frozen.py:{line}:1"))
}

/// A valuation for [`Polynomial::evaluate`].
fn at(pairs: &'static [(&'static str, i128)]) -> impl Fn(&ItsVar) -> Option<i128> {
    move |var: &ItsVar| {
        pairs
            .iter()
            .find(|(name, _)| *name == var.as_str())
            .map(|(_, value)| *value)
    }
}

/// The polynomial the lowered program assigns to `name`.
fn assigned(program: &landav_its::SourceProgram, name: &str) -> Polynomial {
    let its = match lower(program) {
        Ok(its) => its,
        Err(error) => panic!("a well-formed program was refused: {error}"),
    };
    its.transitions()
        .iter()
        .find_map(|transition| transition.update().get(&ItsVar::new(name)).cloned())
        .unwrap_or_else(|| panic!("nothing assigned {name} in the emitted system"))
}

// ---------------------------------------------------------------------------
// the guards themselves, on programs that cannot loop even without them
// ---------------------------------------------------------------------------

/// **A forward reference in the *right* operand is refused.**
///
/// The right operand is the discriminating one. `if !ok(left) || !ok(right)`
/// evaluates `ok(left)` first, and for a well-formed left operand that is
/// `true`, so `!ok(left)` is `false` and the `||` goes on to check the right.
/// Replace `||` with `&&` and the expression short-circuits to `false` on that
/// same `false` — the right operand is **never checked**, nothing is marked
/// malformed, and the traversal follows a handle it has not validated. Delete
/// the first `!` and it returns early on a *good* left operand instead,
/// likewise without marking anything.
///
/// Both mutants, and `expr_child_ok -> true` with them, therefore let this
/// program lower cleanly. The guard, intact, refuses it. The forward target is
/// an integer literal, so nothing here can loop whichever way the comparison
/// goes.
#[test]
fn a_forward_reference_in_the_right_operand_is_refused() {
    let mut donor = SourceProgramBuilder::new("donor", origin(1), vec![]);
    let stolen: Vec<ExprId> = (0..3).map(|value| donor.int(value, origin(2))).collect();
    let ahead = stolen.get(2).copied().expect("three were built");
    assert_eq!(ahead.index(), 2, "the third handle names index two");

    let mut builder = SourceProgramBuilder::new("forward_right", origin(1), vec![]);
    let behind = builder.int(1, origin(2));
    let sum = builder.arith(ArithOp::Add, behind, ahead, origin(3));
    // Index two, and a *leaf*: with the guard removed the traversal reaches
    // this node, finds a literal and stops. Without it being a leaf this test
    // would be the hang it exists to replace.
    let target = builder.int(5, origin(2));

    assert_eq!(behind.index(), 0);
    assert_eq!(sum.index(), 1);
    assert_eq!(
        target.index(),
        2,
        "the forward target must exist and be a leaf"
    );

    let assign = builder.assign(VarName::new("x"), sum, origin(3));
    let program = builder.build(vec![assign]);

    match lower(&program) {
        Err(LoweringError::Malformed { function, detail }) => {
            assert_eq!(function.as_str(), "forward_right");
            assert!(
                !detail.as_str().is_empty(),
                "a malformed error must say why"
            );
        }
        other => panic!("an operand naming a later node must be refused, not followed: {other:?}"),
    }
}

/// The same, for the *left* operand: the guard must not depend on which side
/// the bad handle arrives on.
#[test]
fn a_forward_reference_in_the_left_operand_is_refused() {
    let mut donor = SourceProgramBuilder::new("donor", origin(1), vec![]);
    let stolen: Vec<ExprId> = (0..3).map(|value| donor.int(value, origin(2))).collect();
    let ahead = stolen.get(2).copied().expect("three were built");

    let mut builder = SourceProgramBuilder::new("forward_left", origin(1), vec![]);
    let behind = builder.int(1, origin(2));
    let sum = builder.arith(ArithOp::Add, ahead, behind, origin(3));
    let target = builder.int(5, origin(2));
    assert_eq!((sum.index(), target.index()), (1, 2));

    let assign = builder.assign(VarName::new("x"), sum, origin(3));
    let program = builder.build(vec![assign]);

    assert!(
        matches!(lower(&program), Err(LoweringError::Malformed { .. })),
        "a left operand naming a later node must be refused"
    );
}

/// The unary arms take the same guard by a different call, so they need their
/// own case: `Neg` and `Pow` each check a single operand.
#[test]
fn a_forward_reference_in_a_unary_operand_is_refused() {
    let mut donor = SourceProgramBuilder::new("donor", origin(1), vec![]);
    let stolen: Vec<ExprId> = (0..3).map(|value| donor.int(value, origin(2))).collect();
    let ahead = stolen.get(2).copied().expect("three were built");

    for build_unary in [0u8, 1] {
        let mut builder = SourceProgramBuilder::new("forward_unary", origin(1), vec![]);
        let _filler = builder.int(1, origin(2));
        let node = if build_unary == 0 {
            builder.neg(ahead, origin(3))
        } else {
            builder.pow(ahead, 2, origin(3))
        };
        let target = builder.int(5, origin(2));
        assert_eq!((node.index(), target.index()), (1, 2));

        let assign = builder.assign(VarName::new("x"), node, origin(3));
        let program = builder.build(vec![assign]);

        assert!(
            matches!(lower(&program), Err(LoweringError::Malformed { .. })),
            "a unary operand naming a later node must be refused"
        );
    }
}

/// **A forward reference in a condition's right operand is refused.**
///
/// `cond_dnf`'s `And`/`Or` arm carries the same compound guard as `expr_poly`'s
/// binary arm and fails the same three ways.
#[test]
fn a_forward_reference_in_the_right_condition_operand_is_refused() {
    let mut donor = SourceProgramBuilder::new("donor", origin(1), vec![]);
    let one = donor.int(1, origin(2));
    let two = donor.int(2, origin(2));
    let stolen: Vec<CondId> = (0..3)
        .map(|_| donor.compare(CompareOp::Lt, one, two, origin(2)))
        .collect();
    let ahead = stolen.get(2).copied().expect("three were built");
    assert_eq!(ahead.index(), 2);

    let mut builder = SourceProgramBuilder::new("forward_cond", origin(1), vec![]);
    let left = builder.int(1, origin(2));
    let right = builder.int(2, origin(2));
    let behind = builder.compare(CompareOp::Lt, left, right, origin(2));
    let both = builder.and(behind, ahead, origin(3));
    // Condition index two, and a leaf: a comparison has no condition children.
    let target = builder.compare(CompareOp::Gt, left, right, origin(2));

    assert_eq!((behind.index(), both.index(), target.index()), (0, 1, 2));

    let branch = builder.if_else(both, vec![], vec![], origin(3));
    let program = builder.build(vec![branch]);

    match lower(&program) {
        Err(LoweringError::Malformed { function, detail }) => {
            assert_eq!(function.as_str(), "forward_cond");
            assert!(!detail.as_str().is_empty());
        }
        other => panic!("a condition operand naming a later node must be refused: {other:?}"),
    }
}

/// The same, on the left, and through `Or` rather than `And` — the two share
/// one match arm, but a future split must keep both guarded.
#[test]
fn a_forward_reference_in_the_left_condition_operand_is_refused() {
    let mut donor = SourceProgramBuilder::new("donor", origin(1), vec![]);
    let one = donor.int(1, origin(2));
    let two = donor.int(2, origin(2));
    let stolen: Vec<CondId> = (0..3)
        .map(|_| donor.compare(CompareOp::Lt, one, two, origin(2)))
        .collect();
    let ahead = stolen.get(2).copied().expect("three were built");

    let mut builder = SourceProgramBuilder::new("forward_cond_left", origin(1), vec![]);
    let left = builder.int(1, origin(2));
    let right = builder.int(2, origin(2));
    let behind = builder.compare(CompareOp::Lt, left, right, origin(2));
    let either = builder.or(ahead, behind, origin(3));
    let target = builder.compare(CompareOp::Gt, left, right, origin(2));
    assert_eq!((either.index(), target.index()), (1, 2));

    let branch = builder.if_else(either, vec![], vec![], origin(3));
    let program = builder.build(vec![branch]);

    assert!(
        matches!(lower(&program), Err(LoweringError::Malformed { .. })),
        "a disjunction operand naming a later node must be refused"
    );
}

/// `Not` checks a single condition operand, on its own call.
#[test]
fn a_forward_reference_in_a_negated_condition_is_refused() {
    let mut donor = SourceProgramBuilder::new("donor", origin(1), vec![]);
    let one = donor.int(1, origin(2));
    let two = donor.int(2, origin(2));
    let stolen: Vec<CondId> = (0..3)
        .map(|_| donor.compare(CompareOp::Lt, one, two, origin(2)))
        .collect();
    let ahead = stolen.get(2).copied().expect("three were built");

    let mut builder = SourceProgramBuilder::new("forward_not", origin(1), vec![]);
    let left = builder.int(1, origin(2));
    let right = builder.int(2, origin(2));
    let _behind = builder.compare(CompareOp::Lt, left, right, origin(2));
    let negated = builder.not(ahead, origin(3));
    let target = builder.compare(CompareOp::Gt, left, right, origin(2));
    assert_eq!((negated.index(), target.index()), (1, 2));

    let branch = builder.if_else(negated, vec![], vec![], origin(3));
    let program = builder.build(vec![branch]);

    assert!(
        matches!(lower(&program), Err(LoweringError::Malformed { .. })),
        "a negated operand naming a later node must be refused"
    );
}

// ---------------------------------------------------------------------------
// the other half: a *well-formed* operand must be followed, not refused
// ---------------------------------------------------------------------------

/// Deleting the first `!` in either compound guard inverts it, and a
/// well-formed binary node then returns the placeholder — zero for an
/// expression, `true` for a condition — without recording anything. The
/// program still lowers, so nothing above catches it; what catches it is that
/// every arithmetic expression in the crate becomes the constant zero.
///
/// Stated as the property it is: `a + b` denotes addition.
#[test]
fn a_binary_arithmetic_node_denotes_its_operation() {
    for (op, expected) in [
        (ArithOp::Add, 7i128),
        (ArithOp::Sub, -1),
        (ArithOp::Mul, 12),
    ] {
        let mut builder = SourceProgramBuilder::new(
            "binary",
            origin(1),
            vec![VarName::new("a"), VarName::new("b")],
        );
        let a = builder.var(VarName::new("a"), origin(2));
        let b = builder.var(VarName::new("b"), origin(2));
        let combined = builder.arith(op, a, b, origin(2));
        let assign = builder.assign(VarName::new("x"), combined, origin(2));
        let program = builder.build(vec![assign]);

        let value = assigned(&program, "x");
        assert_eq!(
            value.evaluate(&at(&[("a", 3), ("b", 4)])),
            Some(expected),
            "a {op} b lowered to {value}, which is not the operation"
        );
        assert!(
            !value.is_zero(),
            "a binary node lowered to zero: the operand guard returned early on a good operand"
        );
    }
}

/// The unary arms, for the same reason.
#[test]
fn a_unary_arithmetic_node_denotes_its_operation() {
    let mut builder = SourceProgramBuilder::new("unary", origin(1), vec![VarName::new("a")]);
    let a = builder.var(VarName::new("a"), origin(2));
    let negated = builder.neg(a, origin(2));
    let squared = builder.pow(a, 2, origin(2));
    let first = builder.assign(VarName::new("x"), negated, origin(2));
    let second = builder.assign(VarName::new("y"), squared, origin(2));
    let program = builder.build(vec![first, second]);

    let its = lower(&program).expect("a well-formed program");
    let get = |name: &str| {
        its.transitions()
            .iter()
            .find_map(|transition| transition.update().get(&ItsVar::new(name)).cloned())
            .unwrap_or_else(|| panic!("nothing assigned {name}"))
    };
    assert_eq!(get("x").evaluate(&at(&[("a", 5)])), Some(-5), "-a");
    assert_eq!(get("y").evaluate(&at(&[("a", 5)])), Some(25), "a ** 2");
}

/// The condition counterpart: a conjunction of two comparisons must reach the
/// emitted system as **two constraints**, not as `true`.
///
/// `cond_dnf`'s inverted guard returns `dnf_true()` for every well-formed
/// `And`, `Or` and `Not`, which widens every branch condition to "always" — a
/// silent over-approximation that leaves a lowerable program and an emitted
/// system with no guards at all. That is the shape a soundness suite is least
/// likely to notice, because widening is *sound*.
#[test]
fn a_conjunction_reaches_the_system_as_constraints_not_as_true() {
    let mut builder = SourceProgramBuilder::new(
        "conjunction",
        origin(1),
        vec![VarName::new("a"), VarName::new("b")],
    );
    let a = builder.var(VarName::new("a"), origin(2));
    let b = builder.var(VarName::new("b"), origin(2));
    let zero = builder.int(0, origin(2));
    let left = builder.compare(CompareOp::Lt, a, b, origin(2));
    let right = builder.compare(CompareOp::Gt, a, zero, origin(2));
    let both = builder.and(left, right, origin(3));
    let body = builder.assign(VarName::new("x"), a, origin(4));
    let branch = builder.if_else(both, vec![body], vec![], origin(3));
    let program = builder.build(vec![branch]);

    let its = lower(&program).expect("a well-formed program");
    let widest = its
        .transitions()
        .iter()
        .map(|transition| transition.guard().constraints().len())
        .max()
        .unwrap_or(0);
    assert!(
        widest >= 2,
        "`a < b and a > 0` produced no transition with two constraints; the \
         conjunction was widened to `true`"
    );
    assert!(
        its.transitions()
            .iter()
            .any(|transition| !transition.guard().is_always()),
        "every transition is unguarded: the condition traversal returned `true` \
         for a well-formed conjunction"
    );
}

/// And for `Not`, whose arm carries the single-operand guard.
#[test]
fn a_negation_reaches_the_system_as_constraints_not_as_true() {
    let mut builder = SourceProgramBuilder::new(
        "negation",
        origin(1),
        vec![VarName::new("a"), VarName::new("b")],
    );
    let a = builder.var(VarName::new("a"), origin(2));
    let b = builder.var(VarName::new("b"), origin(2));
    let inner = builder.compare(CompareOp::Lt, a, b, origin(2));
    let negated = builder.not(inner, origin(3));
    let body = builder.assign(VarName::new("x"), a, origin(4));
    let branch = builder.if_else(negated, vec![body], vec![], origin(3));
    let program = builder.build(vec![branch]);

    let its = lower(&program).expect("a well-formed program");
    assert!(
        its.transitions()
            .iter()
            .any(|transition| !transition.guard().is_always()),
        "`not (a < b)` produced no guarded transition"
    );
}

// ---------------------------------------------------------------------------
// why the guards are never *needed* by an honest frontend
// ---------------------------------------------------------------------------

/// **The builder is append-only, and its handles strictly increase.**
///
/// This is the half of the argument that lives outside `lowering.rs`. A node's
/// index is the number of nodes pushed into its arena before it, so every
/// handle a caller can pass as an operand was issued *earlier* and names a
/// smaller index. A program assembled from one builder therefore satisfies
/// "a child's index precedes its parent's" without the traversal having to
/// check anything — the guards exist only for handles that came from
/// somewhere else.
///
/// Weaken this and the guards start rejecting honest programs, which is the
/// opposite failure and just as bad.
#[test]
fn builder_handles_strictly_increase_within_each_arena() {
    let mut builder = SourceProgramBuilder::new("monotone", origin(1), vec![]);

    let mut previous: Option<u32> = None;
    for step in 0..64u32 {
        let issued = builder.int(i64::from(step), origin(2)).index();
        assert_eq!(
            issued, step,
            "expression handle {step} named index {issued}: the arena is not append-only"
        );
        if let Some(before) = previous {
            assert!(
                issued > before,
                "expression handle {issued} did not exceed its predecessor {before}"
            );
        }
        previous = Some(issued);
    }

    // The condition and statement arenas are numbered independently, and each
    // must be append-only in its own right.
    let left = builder.int(0, origin(2));
    let right = builder.int(1, origin(2));
    for step in 0..16u32 {
        let issued = builder
            .compare(CompareOp::Lt, left, right, origin(2))
            .index();
        assert_eq!(issued, step, "condition arena is not append-only");
    }
    for step in 0..16u32 {
        let issued = builder.return_stmt(origin(2)).index();
        assert_eq!(issued, step, "statement arena is not append-only");
    }
}

/// The consequence, stated on a term: an operand handle is always smaller than
/// the handle of the node built from it.
#[test]
fn an_operand_handle_always_precedes_the_node_built_from_it() {
    let mut builder = SourceProgramBuilder::new("precedes", origin(1), vec![]);
    let mut deepest = builder.int(1, origin(2));
    for _ in 0..32 {
        let addend = builder.int(1, origin(2));
        let combined = builder.arith(ArithOp::Add, deepest, addend, origin(2));
        assert!(
            deepest.index() < combined.index() && addend.index() < combined.index(),
            "an operand ({}, {}) did not precede its parent ({})",
            deepest.index(),
            addend.index(),
            combined.index()
        );
        deepest = combined;
    }

    // A 32-deep left spine still lowers, which is the positive control: the
    // guard admits every honest program the builder can produce.
    let assign = builder.assign(VarName::new("x"), deepest, origin(2));
    let program = builder.build(vec![assign]);
    let value = assigned(&program, "x");
    assert_eq!(
        value.as_constant(),
        Some(33),
        "a 32-deep sum of ones is 33, not {value}"
    );
}

/// Past [`landav_its::MAX_ARENA_NODES`] the builder stops issuing increasing
/// handles — every further node gets the `u32::MAX` sentinel — so the
/// monotonicity argument above lapses exactly there. What covers the gap is
/// that the overflow flag is set and lowering refuses the program outright,
/// before any traversal starts.
///
/// Pinned without building a `MAX_ARENA_NODES`-sized arena: the property is
/// that the sentinel is out of range for any arena the builder can fill, which
/// is a statement about the two constants.
#[test]
fn the_overflow_sentinel_cannot_name_a_node() {
    let sentinel = usize::try_from(u32::MAX).expect("a 32-bit index fits a usize here");
    assert!(
        landav_its::MAX_ARENA_NODES <= sentinel,
        "the overflow sentinel names an index inside a full arena, so an \
         overflowed program could still be traversed"
    );

    // And the flag is what a caller sees, on a program that did not overflow.
    let mut builder = SourceProgramBuilder::new("small", origin(1), vec![]);
    let value = builder.int(1, origin(2));
    let assign = builder.assign(VarName::new("x"), value, origin(2));
    let program = builder.build(vec![assign]);
    assert!(
        !program.overflowed(),
        "a two-node program reported an overflow"
    );
}
