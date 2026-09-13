//! `LAN-93` acceptance: **an `async def` is analysed, or at minimum counted.**
//!
//! # The failure this file exists to catch is silence
//!
//! `lower_module` (`landav-python/src/lowering.rs`) walks the module body and
//! matches `Stmt::FunctionDef`. There is no arm for `Stmt::AsyncFunctionDef`.
//! A top-level `async def` parses without complaint and produces **no**
//! [`landav_python::LoweredFunction`] at all.
//!
//! That is a different failure from every other gap in this suite, and worse
//! than all of them. A construct outside the fragment is *refused*: it becomes
//! an `Unsupported` node, it carries a position, it appears in the hole ledger,
//! and `Coverage` counts the function as not lowered. Refusal is loud. This is
//! not refused. The function never enters the pipeline, so it contributes to
//! neither the numerator nor the **denominator**:
//!
//! ```text
//! $ landav check probe.py --json      # one `def`, one `async def`
//!   "functions": 1, "lowered": 1, "coverage_percent": 100, "outcome": "clean"
//! ```
//!
//! A file half of which was never looked at reports full coverage, exactly.
//! There is no output an operator could read, and no exit code CI could branch
//! on, that distinguishes that run from one over a file with no async in it.
//!
//! Measured on this branch, before any fix, by parsing each corpus with
//! CPython's `ast` and comparing against what `landav check --json` reports:
//!
//! | corpus | landav `functions` | top-level `def` | top-level `async def` |
//! |---|---|---|---|
//! | `/usr/lib/python3.12` | 3057 | 3057 (in the 567 files the frontend parses) | **15** |
//! | a typed backend and its `.venv` | 14952 | 14952 | **353** |
//!
//! The two `def` columns agree exactly with what landav reports, which is what
//! makes the third column a clean measurement of what is missing rather than an
//! estimate. (`LAN-93` quotes 219 for the second corpus. 219 is the count with
//! symlinks unfollowed; the walk followed `.venv/lib64 -> lib` at the time and
//! so analysed that tree twice, which is why its denominator is 14952 rather
//! than 7571. Against that denominator the loss is 353. `LAN-95` has since
//! made the walk analyse each file once, so a run today reports the
//! undoubled figures; the ratio each column states is unchanged, which is
//! why the conclusion drawn here still holds.)
//!
//! # What the fix owes, and what it does not
//!
//! Not good bounds. An `await` suspends and resumes; the cost of what is
//! awaited is not this function's cost, and this fragment has no model of a
//! scheduler. [`landav_its::Construct::Coroutine`] already exists and already
//! describes exactly that — "yield, await or async construct" — and a named
//! hole at a position is the honest answer. `async def f(): await g()` should
//! read `Partial(1 + #hole0)` blaming a coroutine, the same way
//! `def f(): return g()` blames a call. Visible, not accurate.
//!
//! # What already works, and is pinned here so the fix cannot break it
//!
//! Three of the four async-flavoured arms in the lowering are already written
//! and already correct — they are simply unreachable from a top-level
//! `async def`, because nothing ever lowers one:
//!
//! * `Expr::Await` refuses [`Construct::Coroutine`] (`lowering.rs`, the
//!   `Await | Yield | YieldFrom` arm);
//! * `Stmt::AsyncFor` and `Stmt::AsyncWith` refuse the same;
//! * `Stmt::AsyncFunctionDef` in *statement* position — a nested `async def` —
//!   refuses the same, exactly as a nested `def` refuses
//!   [`Construct::Declaration`].
//!
//! The pinned-today tests below say so explicitly in their own doc comments.
//! They pass now. They exist because the mechanical half of this fix edits the
//! module-level walk, and a fix that reached that walk by *removing* the
//! statement-level arms would satisfy every failing test here and silently
//! change what a nested `async def` reports.
//!
//! # Why most of this reads the libraries rather than driving the binary
//!
//! The assertions are about the *shape* of a result — which construct is
//! blamed, whether the bound still mentions a parameter, whether reconciliation
//! left it `Unknown` — and the process boundary offers only a rendered string,
//! which these tests are forbidden to pin. The two assertions that are about
//! **counting** drive the binary and read `--json`, because the denominator is
//! a property of the report and of nothing else.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::{collections::BTreeMap, io, path::Path};

use common::Project;
use landav_bound::{Nat, Symbol, Valuation, VarId};
use landav_engine::{Hole, TripCount, cost};
use landav_its::Construct;
use landav_python::LoweredFunction;
use serde_json::Value;

/// The construct tag an `await`, an `async for`, an `async with` and a nested
/// `async def` must all be reported under. Written out rather than taken from
/// [`Construct::Coroutine`], for the same reason the exit codes are written out
/// in `common`: this is a machine-readable tag that reports group by and
/// baselines pin, so a rename must fail a test rather than pass one.
const COROUTINE_TAG: &str = "coroutine";

/// The tag a nested `def` — as opposed to a nested `async def` — is reported
/// under today.
const DECLARATION_TAG: &str = "declaration";

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

/// Translates `source`.
///
/// Deliberately *not* asserting anything about how many functions came back:
/// "no function came back" is this ticket's entire subject, and a helper that
/// panicked on it would make every test below fail from inside the harness
/// rather than at an assertion the implementer can read.
fn functions_of(source: &str) -> Vec<LoweredFunction> {
    landav_python::lower_module(Path::new("coroutines.py"), source).unwrap_or_else(|error| {
        panic!("failed to parse — this is not the defect:\n{source}\n{error}")
    })
}

/// Translates `source` and returns its single function, asserting — in this
/// ticket's own words — that one came back at all.
///
/// The `assert!` is the point. It is the failure every test below hits until
/// the module-level walk gains its arm, and its message has to say which defect
/// it is rather than reading as a broken fixture.
fn only_function(source: &str) -> LoweredFunction {
    let mut functions = functions_of(source);
    assert_eq!(
        functions.len(),
        1,
        "`lower_module` returned {} functions for a module holding exactly one \
         function definition. `LAN-93`: the module-level walk matches \
         `Stmt::FunctionDef` and has no arm for `Stmt::AsyncFunctionDef`, so a \
         top-level `async def` is not refused, not holed and not counted — it \
         is absent. Source:\n{source}",
        functions.len()
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

/// The names a bound mentions.
fn mentioned(result: &TripCount) -> Vec<String> {
    result.bound().map_or_else(Vec::new, |bound| {
        bound
            .vars()
            .iter()
            .map(|var| var.symbol().as_str().to_owned())
            .collect()
    })
}

/// The result must name the construct, place it, and put its variable in the
/// bound.
///
/// One helper rather than three assertions per test, because they fail together
/// for one reason and apart for three different ones, and the message is what
/// tells the implementer which. A hole with no origin sends the reader to grep;
/// a hole absent from the bound reads as a complete cost with a footnote, and
/// leaves `Bound::subst` nothing to fill.
fn assert_blames(result: &TripCount, construct: &str, source: &str) -> Hole {
    let shown = describe(result);
    let hole = result
        .holes()
        .iter()
        .find(|hole| hole.construct() == construct)
        .unwrap_or_else(|| {
            panic!(
                "the region must be blamed on `{construct}` by name — that is \
                 what the reader can act on — got {shown} for:\n{source}"
            )
        });
    assert!(
        hole.origin().as_str().contains(':'),
        "a hole must be placed as well as named, got {} for:\n{source}",
        hole.origin()
    );
    let bound = result
        .bound()
        .unwrap_or_else(|| panic!("a result carrying a hole carries a bound: {shown}"));
    assert!(
        bound.vars().contains(&hole.var()),
        "the hole {} does not occur in {bound}, so the bound reads as a \
         complete cost with a footnote: {shown} for:\n{source}",
        hole.var().symbol()
    );
    hole.clone()
}

/// Reconciliation must not have failed the whole function closed.
///
/// [`landav_engine::analyse`]'s `Walk::reconciled` answers `Unknown` when the
/// traversal did not charge every `Unsupported` node in the arena — there is no
/// position to recover, so there is no sound charge. That is right, and it is
/// also the way this ticket's fix can go wrong: an `await` that produces a node
/// nothing points at turns the whole function unreadable, which trades one kind
/// of invisibility for another.
fn assert_not_unknown(result: &TripCount, source: &str) {
    assert!(
        !matches!(result, TripCount::Unknown),
        "the function derived nothing at all: `Walk::reconciled` found an \
         `Unsupported` node the traversal never charged and failed the result \
         closed. An `await` must be charged where it stands, not left orphaned \
         in the arena — an unreadable function is no more visible than an \
         absent one. Got {} for:\n{source}",
        describe(result)
    );
}

/// The single function reported by a `--json` run over `source`.
fn run_json(project: &Project, name: &str, source: &str) -> io::Result<Value> {
    let target = project.write(name, source)?;
    let run = project.check(&target, &["--json"])?;
    run.assert_did_not_crash();
    Ok(serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not valid JSON ({why}): {}", run.describe())))
}

// ---------------------------------------------------------------------------
// 1. the headline: it exists at all
// ---------------------------------------------------------------------------

/// **A top-level `async def` produces a `LoweredFunction`.**
///
/// The simplest possible statement of the defect, and the one every other test
/// in this file is downstream of. `lower_module` returns an empty vector for
/// this module today.
///
/// Asserted against the *control* in the same test rather than alone: the same
/// body written as a plain `def` comes back, so a failure here cannot be blamed
/// on the fixture, the parser, or the annotation.
#[test]
fn a_top_level_async_def_is_lowered_at_all() {
    let plain = "def ready(n: int) -> int:\n    return n\n";
    let coroutine = "async def ready(n: int) -> int:\n    return n\n";

    assert_eq!(
        functions_of(plain).len(),
        1,
        "the control failed: a plain `def` must lower, or nothing below means \
         anything"
    );

    let lowered = functions_of(coroutine);
    assert_eq!(
        lowered.len(),
        1,
        "`lower_module` produced no function for an `async def` whose body is \
         one `return`, while producing one for the identical plain `def`. It \
         is not refused, not holed and not counted — it is absent from the \
         coverage denominator, which is why nothing in the report says so."
    );
}

/// **The lowered async function keeps its name and its position.**
///
/// A function that arrived unnamed or placed at line zero would satisfy the
/// test above and still be useless in a report: the whole value of counting it
/// is that a reader can go and look at it. `async` is five characters and a
/// space before the `def`, and an implementation that took the position of the
/// inner `FunctionDef` node rather than the statement would land in the wrong
/// column.
#[test]
fn a_lowered_async_function_is_named_and_placed() {
    let source = "async def fetch_rows(n: int) -> int:\n    return n\n";
    let function = only_function(source);

    assert_eq!(
        function.name(),
        "fetch_rows",
        "the lowered function must carry the name as written"
    );
    assert_eq!(
        function.location().line(),
        1,
        "the `async def` is on line 1, and a report that points somewhere else \
         cannot be acted on"
    );
    assert_eq!(
        function.location().column(),
        1,
        "the position is the `async` keyword, where the statement begins — not \
         the `def` five characters later"
    );
}

// ---------------------------------------------------------------------------
// 2. the denominator: it is counted in the run
// ---------------------------------------------------------------------------

/// **A module with one `def` and one `async def` reports two functions.**
///
/// The acceptance criterion stated as the number an operator reads. Every
/// corpus measurement quoted on this project is over this denominator, so a
/// missing entry does not merely lose one function — it silently improves every
/// ratio computed from it.
#[test]
fn both_functions_in_a_mixed_module_are_counted() -> io::Result<()> {
    let source = "\
def plain(n: int) -> int:
    return n


async def coroutine(n: int) -> int:
    return n
";
    let project = Project::new()?;
    let run = run_json(&project, "mixed.py", source)?;

    assert_eq!(
        run["summary"]["functions"],
        Value::from(2),
        "a module holding one `def` and one `async def` reported {} \
         function(s). The `async def` is not in the denominator, so the \
         coverage ratio is computed over a population that excludes it.\n{run}",
        run["summary"]["functions"]
    );

    let names: Vec<&str> = run["functions"]
        .as_array()
        .map(|fs| fs.iter().filter_map(|f| f["name"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        names.contains(&"coroutine"),
        "the async function is missing from the `functions` list, so no \
         consumer of the JSON — a CI gate or an agent deciding what to change — \
         can even learn that it exists. Got {names:?}.\n{run}"
    );
    Ok(())
}

/// **A file whose async function was never analysed does not report full
/// coverage.**
///
/// The operator-visible half of the same defect, and the one that makes it
/// worse than a refusal. A refusal moves `lowered` below `functions` and the
/// summary says so. An omission moves *both*, and `1 of 1` is arithmetically
/// true about a population that quietly excluded the function nobody looked at.
///
/// Deliberately asserted as "not 100%" rather than as a particular number: what
/// the fix must destroy is the claim of completeness, and the exact ratio
/// depends on how much of the async body turns out to be inside the fragment.
#[test]
fn a_file_with_an_unanalysed_async_function_does_not_claim_full_coverage() -> io::Result<()> {
    let source = "\
def plain(n: int) -> int:
    return n


async def coroutine(n: int) -> int:
    await fetch(n)
    return n
";
    let project = Project::new()?;
    let run = run_json(&project, "half_async.py", source)?;

    assert_ne!(
        run["summary"]["coverage_percent"],
        Value::from(100),
        "a file containing an `await` this analysis has no model for reported \
         100% coverage. That is not a loose claim, it is a claim about code \
         that never entered the pipeline — and it is indistinguishable, in \
         every byte of output and in the exit code, from a run over a file with \
         no coroutine in it.\n{run}"
    );
    assert!(
        run["summary"]["refusals"].as_u64().unwrap_or(0) > 0,
        "the `await` was not counted as a refusal. `Construct::Coroutine` \
         exists precisely so that this is sayable.\n{run}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. `await` is a named, placed hole
// ---------------------------------------------------------------------------

/// **`await g(n)` is blamed on a coroutine, at a position.**
///
/// The ticket's own worked example. `async def f(): await g()` must read like
/// `def f(): return g()` does — a partial bound with a hole naming the thing
/// the reader can act on — rather than like a function that was never seen.
///
/// The value assertion separates the plausible wrong answers: the `await`
/// statement costs its own step plus whatever it suspends for, so at a
/// coroutine cost of 5 the two-statement body is 7. A 6 would mean the
/// statement itself was charged nothing, which leaves `Bound::subst` a step
/// short per region on every async function in the corpus.
#[test]
fn an_awaited_call_is_a_named_placed_hole() {
    let source = "\
async def fetch(n: int) -> int:
    await query(n)
    return n
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_not_unknown(&result, source);
    assert!(
        matches!(result, TripCount::Partial { .. }),
        "a function containing an `await` is not derivable and must say so \
         with a hole rather than claim a cost: {} for:\n{source}",
        describe(&result)
    );
    let hole = assert_blames(&result, COROUTINE_TAG, source);

    let mut table = BTreeMap::new();
    table.insert(hole.var().symbol().clone(), 5_u64);
    let bound = result.bound().expect("a partial result carries a bound");
    assert_eq!(
        bound.eval(&Bindings(table)),
        Nat::Fin(7),
        "the `await` is one statement costing one step plus whatever it \
         suspends for, and the `return` is another: {} for:\n{source}",
        describe(&result)
    );
}

/// **`Construct::Coroutine` is the construct the lowering refuses, so the
/// function is not claimed to have lowered.**
///
/// The engine deriving a partial bound is a second number, not a change to
/// coverage. An `await` still has an unknown effect on the integer state —
/// anything can run while this coroutine is suspended — and `Update` is a total
/// map with no havoc, so a transition system admitting this program would
/// assert `n` is unchanged across the `await`.
#[test]
fn an_await_still_stops_the_program_from_lowering() {
    let source = "\
async def fetch(n: int) -> int:
    await query(n)
    return n
";
    let function = only_function(source);
    let refused = landav_its::lower(function.program());

    assert!(
        refused.is_err(),
        "an `await` must stop a program from lowering: a coroutine's \
         suspension point is exactly where foreign code may change the integer \
         state.\n{source}"
    );
    let constructs = refused
        .as_ref()
        .err()
        .and_then(landav_its::LoweringError::refusals)
        .map(landav_its::Refusals::constructs)
        .unwrap_or_default();
    assert!(
        constructs.contains(&Construct::Coroutine),
        "the refusal must name the coroutine, got {constructs:?} for:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// 4. the analysable part is still analysed
// ---------------------------------------------------------------------------

/// **An `async def` whose body is entirely inside the fragment lowers
/// completely.**
///
/// The whole value of visibility. `async` is a property of how the function is
/// *called*, not of the arithmetic in it, and a counted `range` loop counts the
/// same either way. If lowering an async function produced a hole merely for
/// being async, the fix would have converted a silence into a blanket refusal
/// and bought nothing.
#[test]
fn an_async_def_with_no_async_constructs_derives_a_complete_bound() {
    let source = "\
async def walk(n: int) -> int:
    total = 0
    for i in range(n):
        total = total + i
    return total
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_not_unknown(&result, source);
    assert!(
        result.is_complete(),
        "nothing in this body suspends: it is a counted `range` loop and \
         integer arithmetic, and the `async` keyword changes none of it. A hole \
         here would mean async functions are refused wholesale rather than \
         analysed: {} for:\n{source}",
        describe(&result)
    );
    let names = mentioned(&result);
    assert!(
        names.iter().any(|var| var == "n"),
        "the bound must be a function of the parameter, got {names:?}: {} \
         for:\n{source}",
        describe(&result)
    );
}

/// **A loop inside an async function is still counted around the `await` in
/// it.**
///
/// The shape that decides whether counting async functions is worth anything:
/// the loop is derivable, the `await` is not, and the honest answer keeps both
/// facts. A result that dropped `n` would say the loop is free; a result that
/// dropped the hole would claim a cost for a suspension nobody has modelled.
///
/// The hole must **multiply**, not add. At `n = 3` with a coroutine costing 5,
/// `n * (2 + await)` is 21 — one step for the iteration, one for the statement,
/// and the suspension — and `n + await` (8) is the answer that understates
/// every loop that awaits anything, which is the direction a resource bound
/// must never move.
#[test]
fn a_counted_loop_around_an_await_keeps_both_the_count_and_the_hole() {
    let source = "\
async def gather(n: int) -> int:
    for i in range(n):
        await query(i)
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_not_unknown(&result, source);
    let hole = assert_blames(&result, COROUTINE_TAG, source);
    let names = mentioned(&result);
    assert!(
        names.iter().any(|var| var == "n"),
        "the loop is counted whatever its body does, so the bound must still \
         mention `n`, got {names:?}: {} for:\n{source}",
        describe(&result)
    );

    let bound = result.bound().expect("a partial result carries a bound");
    for (n, await_cost, expected) in [(3_u64, 5_u64, 21_u64), (0, 5, 0), (4, 0, 8)] {
        let mut table = BTreeMap::new();
        table.insert(Symbol::from("n"), n);
        table.insert(hole.var().symbol().clone(), await_cost);
        assert_eq!(
            bound.eval(&Bindings(table)),
            Nat::Fin(expected),
            "at n = {n} with an `await` costing {await_cost} the loop costs \
             n * (2 + await) = {expected}. `n + await` would mean the \
             suspension was charged once outside the loop, which understates \
             every async loop in the corpus: {}",
            describe(&result)
        );
    }
}

// ---------------------------------------------------------------------------
// 5. `async for` and `async with`
// ---------------------------------------------------------------------------

/// **`async for` is named, not deferred.**
///
/// # The decision, and why it is "named"
///
/// Naming these costs nothing and deferring them would cost the ticket its
/// point. `Stmt::AsyncFor` and `Stmt::AsyncWith` **already** have arms in the
/// lowering refusing [`Construct::Coroutine`], and those arms already fire
/// today for an `async for` written inside a plain `def` — which the pinned
/// regression guard below demonstrates. Nothing has to be designed, decided or
/// written: the arms are simply unreachable from a top-level `async def`,
/// because nothing lowers one. The moment the module-level walk gains its arm
/// they start firing, and a test that "deferred" them would be recording an
/// absence that the mechanical half of the fix has already filled.
///
/// Naming them is also the only honest answer available. An `async for` calls
/// `__anext__` and suspends on each turn, so neither its trip count nor its
/// per-iteration cost is anything this fragment can see. Counting the loop
/// would be a claim; holing the statement is not.
#[test]
fn an_async_for_is_a_named_placed_coroutine() {
    let source = "\
async def drain(n: int) -> int:
    total = 0
    async for row in rows(n):
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_not_unknown(&result, source);
    assert_blames(&result, COROUTINE_TAG, source);
    assert!(
        !result.is_complete(),
        "an `async for` suspends on every turn and its trip count is whatever \
         the async iterator decides, so a complete bound over it would be a \
         claim this analysis cannot support: {} for:\n{source}",
        describe(&result)
    );
}

/// **`async with` is named the same way.**
///
/// `__aenter__` and `__aexit__` both suspend, and a lock's may block for
/// unbounded time. Same argument as [`an_async_for_is_a_named_placed_coroutine`]
/// and the same already-written arm.
#[test]
fn an_async_with_is_a_named_placed_coroutine() {
    let source = "\
async def guarded(n: int) -> int:
    async with lock(n):
        n = n + 1
    return n
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_not_unknown(&result, source);
    assert_blames(&result, COROUTINE_TAG, source);
}

/// **Pinned today, as a regression guard: `async for` in a plain `def` is
/// already a placed coroutine hole.**
///
/// This passes on the unfixed branch and is expected to. `rustpython-parser`
/// does not enforce that `async for` and `await` appear only inside an
/// `async def`, so both arms are reachable from a plain `def` and both already
/// fire — which is the evidence behind the decision recorded on
/// [`an_async_for_is_a_named_placed_coroutine`] that nothing about these
/// constructs needs designing.
///
/// It earns its place because the mechanical half of this fix edits the walk
/// over module statements, and an implementation that reached the goal by
/// *moving* the `AsyncFor` / `AsyncWith` / `Await` handling would satisfy every
/// failing test in this file while quietly changing what these report. This
/// test is what makes that a failure rather than a surprise.
#[test]
fn async_constructs_inside_a_plain_def_already_name_a_coroutine() {
    for source in [
        "def drain(n: int) -> int:\n    total = 0\n    async for row in rows(n):\n        total = total + 1\n    return total\n",
        "def guarded(n: int) -> int:\n    async with lock(n):\n        n = n + 1\n    return n\n",
        "def fetch(n: int) -> int:\n    await query(n)\n    return n\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert_not_unknown(&result, source);
        assert_blames(&result, COROUTINE_TAG, source);
    }
}

// ---------------------------------------------------------------------------
// 6. nested definitions
// ---------------------------------------------------------------------------

/// **A nested `async def` inside a top-level `async def` is named.**
///
/// The half of the nesting question `LAN-93` owns. Everything a top-level
/// `async def` contains is invisible today for one reason — the enclosing
/// function is never lowered — and a nested definition is no exception.
///
/// # What a nested definition does today, checked first
///
/// A nested `def` is **not** a second `LoweredFunction`. It refuses
/// [`Construct::Declaration`] and becomes a placed hole in its parent, and a
/// nested `async def` refuses [`Construct::Coroutine`] the same way (both
/// pinned in [`nested_definitions_inside_a_plain_def_are_already_named`]). That
/// is a deliberate design and not a second silence: a nested definition is
/// *visible*, it is simply reported as a region of its parent rather than as a
/// function of its own. Whether inner functions should be analysed separately —
/// they carry their own closure, so they are not free-standing — is a question
/// this ticket neither owns nor is blocked by, and this test pins only that the
/// existing treatment survives being reached through an `async def`.
#[test]
fn a_nested_async_def_inside_an_async_def_is_named() {
    let source = "\
async def outer(n: int) -> int:
    async def inner(m: int) -> int:
        return m
    return n
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_not_unknown(&result, source);
    assert_blames(&result, COROUTINE_TAG, source);
}

/// **A nested plain `def` inside a top-level `async def` is named too.**
///
/// The other direction across the same boundary, and it must land on
/// `declaration` rather than `coroutine`: what is nested here is an ordinary
/// function, and reporting it as a coroutine because its parent happens to be
/// one would send the reader looking for a suspension that is not there.
#[test]
fn a_nested_plain_def_inside_an_async_def_is_named_a_declaration() {
    let source = "\
async def outer(n: int) -> int:
    def inner(m: int) -> int:
        return m
    return n
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_not_unknown(&result, source);
    assert_blames(&result, DECLARATION_TAG, source);
}

/// **Pinned today, as a regression guard: nested definitions inside a plain
/// `def` are already named, and named apart.**
///
/// Passes on the unfixed branch. It is here for two reasons. It is the evidence
/// for the finding recorded above — nested functions are *not* a second
/// instance of this ticket's silence, they are already visible as holes — so a
/// reader of this file does not have to take that on trust. And it fixes the
/// `declaration` / `coroutine` distinction in place before the fix, so an
/// implementation that folded the two statement-level arms together while
/// adding the module-level one would fail here rather than pass everywhere.
#[test]
fn nested_definitions_inside_a_plain_def_are_already_named() {
    let plain = "def outer(n: int) -> int:\n    def inner(m: int) -> int:\n        return m\n    return n\n";
    let coroutine = "def outer(n: int) -> int:\n    async def inner(m: int) -> int:\n        return m\n    return n\n";

    for (source, tag) in [(plain, DECLARATION_TAG), (coroutine, COROUTINE_TAG)] {
        let functions = functions_of(source);
        assert_eq!(
            functions.len(),
            1,
            "a nested definition is a region of its parent, not a second \
             lowered function: {} came back for:\n{source}",
            functions.len()
        );
        let result = cost(functions[0].program());
        assert_not_unknown(&result, source);
        assert_blames(&result, tag, source);
    }
}

// ---------------------------------------------------------------------------
// 7. reconciliation: every node charged
// ---------------------------------------------------------------------------

/// **An `await` in every position it can occupy leaves the function readable.**
///
/// `Walk::reconciled` fails the whole result closed to `Unknown` when an
/// `Unsupported` node in the arena was never charged by the traversal — there
/// is no position to recover, so no sound charge exists. That is correct, and
/// it is the trap for this ticket: the frontend builds nodes for expressions it
/// then discards (`bind` refuses at *statement* level, `Return` drops the
/// handle it translated), so an `await` reached through one of those paths can
/// end up in the arena with nothing pointing at it.
///
/// The outcome would be a function that is counted, is not silent, and reports
/// *nothing at all* — which trades one invisibility for another and loses the
/// blame this ticket exists to produce. Every position gets its own case,
/// because they fail separately and the `Unknown` they produce looks identical.
#[test]
fn an_await_in_any_position_still_derives_a_blamed_result() {
    for source in [
        // bare statement
        "async def a(n: int) -> int:\n    await query(n)\n    return n\n",
        // assigned — `bind` refuses at statement level and the translated
        // expression is orphaned
        "async def b(n: int) -> int:\n    x = await query(n)\n    return x\n",
        // returned — `Return` translates the expression and drops the handle
        "async def c(n: int) -> int:\n    return await query(n)\n",
        // in a condition
        "async def d(n: int) -> int:\n    x = 0\n    if await ready(n):\n        x = 1\n    return x\n",
        // nested inside a call argument
        "async def e(n: int) -> int:\n    await outer(await inner(n))\n    return n\n",
        // several, so a partial reconciliation cannot pass by luck
        "async def f(n: int) -> int:\n    await one(n)\n    await two(n)\n    return n\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());

        assert_not_unknown(&result, source);
        assert!(
            !result.holes().is_empty(),
            "the function derived a result with no holes at all, so the \
             `await` was charged as though it were free: {} for:\n{source}",
            describe(&result)
        );
        assert_blames(&result, COROUTINE_TAG, source);
    }
}

// ---------------------------------------------------------------------------
// 8. decorators
// ---------------------------------------------------------------------------

/// **A decorated `async def` is lowered.**
///
/// Not a separate mechanism — decorators are ignored by the lowering, and a
/// decorated plain `def` lowers today exactly as an undecorated one does. It is
/// here because it is what the corpus actually looks like: the async functions
/// in a typed backend are route handlers, fixtures and context managers, and
/// almost every one of them carries at least one decorator. A fix that matched
/// `Stmt::AsyncFunctionDef` but reached it through a path that skipped
/// decorated statements would recover almost none of the 353 functions
/// measured.
///
/// The multi-decorator case is included because stacking is the common shape
/// and costs one line to pin.
#[test]
fn a_decorated_async_def_is_lowered_and_counted() {
    let source = "\
@route
@authenticated
async def handler(n: int) -> int:
    return n
";
    let function = only_function(source);

    assert_eq!(
        function.name(),
        "handler",
        "a decorator does not change the function's name"
    );
    let result = cost(function.program());
    assert_not_unknown(&result, source);
    assert!(
        result.is_complete(),
        "nothing in this body suspends, and a decorator is not a construct of \
         the body: {} for:\n{source}",
        describe(&result)
    );
}

/// **A decorated `async def` is counted in the run, beside a decorated plain
/// one.**
///
/// The denominator assertion for the shape the corpus is made of. Driven
/// through the binary for the same reason
/// [`both_functions_in_a_mixed_module_are_counted`] is: how many functions a
/// run found is a property of the report.
#[test]
fn a_decorated_async_def_appears_in_the_json_run() -> io::Result<()> {
    let source = "\
@route
def sync_handler(n: int) -> int:
    return n


@route
async def async_handler(n: int) -> int:
    return n
";
    let project = Project::new()?;
    let run = run_json(&project, "handlers.py", source)?;

    assert_eq!(
        run["summary"]["functions"],
        Value::from(2),
        "a decorated `async def` beside a decorated `def` reported {} \
         function(s).\n{run}",
        run["summary"]["functions"]
    );
    let names: Vec<&str> = run["functions"]
        .as_array()
        .map(|fs| fs.iter().filter_map(|f| f["name"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        names.contains(&"async_handler"),
        "the decorated async handler — the dominant shape of async code in the \
         measured corpus — is missing from the run. Got {names:?}.\n{run}"
    );
    Ok(())
}
