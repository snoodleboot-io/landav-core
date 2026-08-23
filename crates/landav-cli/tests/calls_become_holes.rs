//! `LAN-87` acceptance, from Python source: **a call is charged where it
//! stands.**
//!
//! # Why this file is here and not in `landav-engine`
//!
//! The engine suites work at the `SourceProgram` contract, which is the right
//! level for totality — every position an `Unsupported` node can occupy, every
//! construct in the vocabulary. But two of the four measured leaks are not
//! reachable from that level at all, because they are things the **frontend**
//! does before the engine ever runs:
//!
//! * `landav-python`'s `bare_expression` translates `f(n)` and then returns no
//!   statement, so `def g(n): f(n)` arrives at the engine as an *empty body*;
//! * `Stmt::Return` translates the returned expression and drops the handle,
//!   so `return f(n)` arrives as a bare `Return`.
//!
//! In both cases the `Unsupported` node exists in the arena with nothing
//! pointing at it. That is deliberate and correct for `landav_its::lower`,
//! which scans the arenas — but the engine walks the control structure, and a
//! test written against a hand-built `SourceProgram` would never see the shape
//! the frontend actually produces. Bare calls and `return f(...)` are where the
//! calls in the 441 stdlib functions live, so this is the coverage the ticket
//! is actually buying.
//!
//! This is the only package that already depends on both the frontend and the
//! engine, so the test lives here and adds no dependency edge. Unlike its
//! neighbours it reads the libraries directly rather than driving the built
//! binary: the assertions are about the *shape* of a bound — that a hole
//! multiplies rather than adds — and the process boundary only offers the
//! rendered string, which these tests are forbidden to pin.
//!
//! # `Theta(0)` is the case to look at first
//!
//! `def g(n): f(n)` reports `Exact(Bound::zero())` today: a function whose
//! entire body is a call is reported as costing nothing, exactly, with no
//! holes. It is the most confident wrong answer the tool can produce.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, path::Path};

use landav_bound::{Nat, Symbol, Valuation, VarId};
use landav_engine::{Hole, TripCount, cost};
use landav_python::LoweredFunction;

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

/// A valuation over an explicit table, zero elsewhere.
struct Bindings(BTreeMap<Symbol, u64>);

impl Valuation for Bindings {
    fn value_of(&self, var: &VarId) -> Nat {
        Nat::Fin(self.0.get(var.symbol()).copied().unwrap_or(0))
    }
}

/// Translates `source` and returns its single function.
fn only_function(source: &str) -> LoweredFunction {
    let mut functions = landav_python::lower_module(Path::new("calls.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    assert_eq!(
        functions.len(),
        1,
        "expected exactly one function in:\n{source}"
    );
    functions.remove(0)
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
    let holes: Vec<String> = result.holes().iter().map(ToString::to_string).collect();
    format!("{kind}({bound}) holes={holes:?}")
}

/// The result must name the call, place it, and put its variable in the bound.
///
/// One helper rather than three assertions per test, because these three fail
/// together for one reason and apart for three different ones, and the message
/// is what tells the implementer which.
fn assert_blames_a_call(result: &TripCount, source: &str) -> Hole {
    let shown = describe(result);
    assert!(
        matches!(result, TripCount::Partial { .. }),
        "a function containing a call is not derivable and must say so with a \
         hole, got {shown} for:\n{source}"
    );
    let hole = result
        .holes()
        .iter()
        .find(|hole| hole.construct() == "call")
        .unwrap_or_else(|| {
            panic!(
                "the region must be blamed on the call by name - `call` is what \
                 the user can act on - got {shown} for:\n{source}"
            )
        });
    assert!(
        hole.origin().as_str().contains(':'),
        "a hole must be placed as well as named, got {} for:\n{source}",
        hole.origin()
    );
    let bound = result.bound().expect("a partial result carries a bound");
    assert!(
        bound.vars().contains(&hole.var()),
        "the hole {} does not occur in {bound}, so the bound reads as a \
         complete cost with a footnote and `Bound::subst` has nothing to fill \
         for:\n{source}",
        hole.var().symbol()
    );
    hole.clone()
}

/// The bound's value with `n` and the call's cost bound to concrete numbers.
fn value_at(result: &TripCount, hole: &Hole, n: u64, call: u64) -> Nat {
    let mut table = BTreeMap::new();
    table.insert(Symbol::from("n"), n);
    table.insert(hole.var().symbol().clone(), call);
    result
        .bound()
        .expect("a partial result carries a bound")
        .eval(&Bindings(table))
}

// ---------------------------------------------------------------------------
// the four leaks, from source
// ---------------------------------------------------------------------------

/// **A function whose body is one call does not cost zero.**
///
/// `Exact(Bound::zero())` today. Asserted explicitly against zero as well as
/// against the shape, because this is the failure a reader of the report cannot
/// distinguish from a genuinely trivial function.
#[test]
fn a_bare_call_statement_is_charged_rather_than_dropped() {
    let source = "def g(n: int) -> int:\n    f(n)\n";
    let function = only_function(source);
    let result = cost(function.program());

    assert!(
        !matches!(&result, TripCount::Exact(bound) if bound.vars().is_empty()),
        "a function whose whole body is a call was reported as costing a \
         constant, exactly: {}",
        describe(&result)
    );
    let hole = assert_blames_a_call(&result, source);

    // One statement, whose cost is the call's.
    assert_eq!(
        value_at(&result, &hole, 0, 5),
        Nat::Fin(6),
        "the statement costs one step plus whatever the call costs: {}",
        describe(&result)
    );
}

/// **`return f(n)` is charged.**
///
/// The `Return` node of this fragment carries no value — runtime is modelled,
/// not results — so the frontend translates the returned expression purely to
/// record what it could not handle, and the node it builds has no parent. The
/// engine must charge it at the `return`, not lose it.
#[test]
fn a_returned_call_is_charged_at_the_return() {
    let source = "def g(n: int) -> int:\n    return f(n)\n";
    let function = only_function(source);
    let result = cost(function.program());

    let hole = assert_blames_a_call(&result, source);
    assert_eq!(
        value_at(&result, &hole, 0, 5),
        Nat::Fin(6),
        "the `return` is one step and the call costs what it costs: {}",
        describe(&result)
    );
}

/// **A call in a loop body is paid once per iteration.**
///
/// The assertion the whole coverage claim rests on. `for i in range(n): f(i)`
/// costs `n * (2 + f)` — the loop's own step, the statement, and the call, `n`
/// times — and every plausible wrong answer is separated by evaluation:
///
/// | answer | at `n = 3`, `f = 5` |
/// |---|---|
/// | `n * (2 + f)` — correct | 21 |
/// | `n` — the call dropped | 3 |
/// | `n + f` — charged once, outside the loop | 8 |
/// | `n * (1 + f)` — the statement's own step dropped | 18 |
///
/// # This criterion was amended, and the amendment is the point
///
/// As first written it asserted `n * (1 + f)` = 18. That is not satisfiable
/// alongside `a_bare_call_statement_is_charged_rather_than_dropped`, and the
/// argument needs nothing but those two tests: **this loop's body is literally
/// that test's program**. If a bare call statement costs `1 + f` (6, which that
/// test asserts), then by `TripCount::iterating`'s one-step-per-iteration
/// overhead this loop costs `n * (2 + f)`. No frontend or engine choice makes
/// one statement cost `1 + f` on its own and `f` inside a loop.
///
/// 21 is the number the engine's *declared* unit gives — "one per statement
/// executed, plus one per loop iteration", `landav_engine::cost` — and it is the
/// number that keeps holes composable. `f(i)` is one statement; filling `#hole0`
/// with what `f` costs must reproduce the bound the engine would have derived
/// had it known `f` all along, and that bound charges the statement its step.
/// The 18 reading treats the hole as "the cost of the whole statement, its own
/// step included", which is self-consistent but leaves `Bound::subst` — the
/// documented path for landing a callee's real bound — short one step per
/// region, on every one of the 441 functions this ticket is for.
#[test]
fn a_call_in_a_loop_body_is_paid_once_per_iteration() {
    let source = "def g(n: int) -> int:\n    for i in range(n):\n        f(i)\n";
    let function = only_function(source);
    let result = cost(function.program());

    let hole = assert_blames_a_call(&result, source);
    for (n, call, expected) in [(3_u64, 5_u64, 21_u64), (0, 5, 0), (4, 0, 8)] {
        assert_eq!(
            value_at(&result, &hole, n, call),
            Nat::Fin(expected),
            "at n = {n} with a call costing {call} the loop costs \
             n * (2 + call) = {expected}: {}\n\n\
             `n + call` means the call was charged once outside the loop, which \
             understates every loop that calls anything - the shape this ticket \
             exists to report. `n * (1 + call)` means the statement itself was \
             charged nothing, which is one step short per region once the hole \
             is filled.",
            describe(&result)
        );
    }
}

/// **A call in an `if` condition is charged.**
///
/// The condition is evaluated whichever branch is taken, so its cost is
/// additive and unconditional. Today the branch arithmetic proceeds as though
/// the test were free.
#[test]
fn a_call_in_an_if_condition_is_charged() {
    let source = "def g(n: int) -> int:\n    x = 0\n    if f(n):\n        x = 1\n    return x\n";
    let function = only_function(source);
    let result = cost(function.program());

    let hole = assert_blames_a_call(&result, source);
    // `x = 0`, the test, the branch, the `return`: four steps plus the call.
    assert_eq!(
        value_at(&result, &hole, 0, 5),
        Nat::Fin(9),
        "the condition is evaluated on every path, so its cost is added rather \
         than maximised over the branches: {}",
        describe(&result)
    );
}

/// **`x = f(n)` keeps naming its region, and does not fail closed.**
///
/// This shape already produces a hole, by a route worth knowing: `bind`
/// refuses at *statement* level, because assigning a call's result leaves `x`
/// with a value no later guard can use. So the arena holds two nodes — the
/// refused statement, which the traversal reaches, and the translated call
/// expression, which nothing points at.
///
/// That makes it the collision case for Phase 2. Reconciliation says an
/// `Unsupported` node the traversal did not charge fails the whole result
/// closed to `Unknown` — and the discarded call expression is exactly such a
/// node. A literal reading turns today's `Partial` into `Unknown` and loses the
/// blame on the most common call shape in the corpus. Whatever the resolution
/// (attaching the node in the frontend, or treating a node subsumed by a
/// refusing parent as charged), the observable outcome must not regress.
#[test]
fn an_assigned_call_still_reports_a_placed_region() {
    let source = "def g(n: int) -> int:\n    x = f(n)\n    return x\n";
    let function = only_function(source);
    let result = cost(function.program());

    assert!(
        matches!(result, TripCount::Partial { .. }),
        "`x = f(n)` is reported today as a partial bound naming its region, and \
         must not regress to `Unknown` when unreached arena nodes start failing \
         the result closed: {}",
        describe(&result)
    );
    let hole = result
        .holes()
        .first()
        .unwrap_or_else(|| panic!("a partial result names its regions: {}", describe(&result)));
    assert!(
        hole.origin().as_str().contains(':'),
        "the region must be placed: {}",
        describe(&result)
    );
}

// ---------------------------------------------------------------------------
// the half of option (A) that must not move
// ---------------------------------------------------------------------------

/// **The lowering still refuses every one of them.**
///
/// Option (A) is "the engine gets total, the ITS does not get a representation
/// for unknown cost". A call still has an unknown *effect* on the integer
/// state, and `Update` is a total map with no havoc, so a transition system
/// that admitted these programs would assert `x` is unchanged across
/// `x = f(n)`.
///
/// This is also what keeps `Coverage::lowered()` honest: the headline number
/// counts transition systems, and it must not move because the engine's reach
/// did.
#[test]
fn a_call_still_stops_the_program_from_lowering() {
    for source in [
        "def g(n: int) -> int:\n    f(n)\n",
        "def g(n: int) -> int:\n    return f(n)\n",
        "def g(n: int) -> int:\n    for i in range(n):\n        f(i)\n",
        "def g(n: int) -> int:\n    x = 0\n    if f(n):\n        x = 1\n    return x\n",
        "def g(n: int) -> int:\n    x = f(n)\n    return x\n",
    ] {
        let function = only_function(source);
        let refused = landav_its::lower(function.program());
        assert!(
            refused.is_err(),
            "a call must still stop a program from lowering - the engine's new \
             reach is a second number, not a change to coverage:\n{source}"
        );
        let constructs = refused
            .as_ref()
            .err()
            .and_then(landav_its::LoweringError::refusals)
            .map(landav_its::Refusals::constructs)
            .unwrap_or_default();
        assert!(
            constructs.contains(&landav_its::Construct::Call),
            "the refusal must still name the call, got {constructs:?} \
             for:\n{source}"
        );
    }
}
