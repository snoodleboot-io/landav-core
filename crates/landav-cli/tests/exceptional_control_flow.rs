//! `LAN-92` acceptance: **`try`, `except`, `finally`, `raise` and `with`.**
//!
//! # Why this lane is the one that can produce an unsound bound
//!
//! Every other construct in the `LAN-91` sweep is an *unknown cost in a known
//! place*: a comprehension costs what it costs, and holing it where it stands
//! leaves the control structure around it exactly as it was. Exceptional
//! control flow is different in kind. It is an **edge out of the region being
//! counted**, and the region it leaves is a loop whose trip count this engine
//! reports as an equality.
//!
//! `landav_engine::analyse` already knows this shape and already handles it for
//! the one construct that could produce it before now:
//!
//! > A `return` is the one thing it does not refuse that can [leave a loop
//! > early], so a body containing one makes the count an over-estimate rather
//! > than an equality, and the result is relaxed.
//!
//! A `raise` is the same edge. If it is modelled as an ordinary statement — one
//! step, charged where it stands — without the accompanying relaxation, then
//! `for i in range(n): raise ValueError` reports `Theta(2n)` for a program that
//! executes two steps. That is not a loose bound; it is a **wrong** one, marked
//! exact, carrying no holes, offered to a budget gate for comparison. It is the
//! `LAN-88` failure class exactly, reached by a different route.
//!
//! # The five traps, each with a test below
//!
//! | # | Trap | Test |
//! |---|---|---|
//! | 1 | A `raise` in a loop is an early exit, so `Theta` must become `O` | [`a_raise_in_a_loop_is_never_reported_as_an_equality`] |
//! | 2 | A `try` body may exit at *any* statement, so the `try` does not cost the body | [`a_try_except_is_at_least_the_worse_of_its_two_paths`] |
//! | 3 | `finally` runs on the normal path *and* the exceptional one | [`a_finally_is_charged_on_both_paths`] |
//! | 4 | An `except` is not an `else`: the handler adds to a *prefix* of the body | [`an_except_handler_adds_to_a_body_prefix`] |
//! | 5 | `with` has an implicit `__exit__`, which is a call, which is a hole | [`a_with_statement_charges_its_implicit_exit_as_a_call`] |
//!
//! # What "sound" is asserted to mean here
//!
//! [`assert_never_below_the_truth`] is the load-bearing helper and it is
//! deliberately stronger than the one in `landav-engine/tests/value_soundness.rs`.
//! That one skips partial results, on the correct reasoning that an unfilled
//! hole denotes `omega` and so makes no finite claim. The cost of that reasoning
//! is vacuity: **every** program in this file is partial today, so a check
//! written that way would pass on all ten and measure nothing.
//!
//! So the holes are filled first, each with the program's own total true cost,
//! and the resulting *complete* bound is required to dominate. That is a
//! necessary condition of soundness rather than an extra demand, and the
//! argument is two lines:
//!
//! * one execution of a region is part of one execution of the program, and
//!   cost is additive and non-negative, so the true per-execution cost of any
//!   region is **at most** the program's total true cost;
//! * [`landav_bound::Bound`] is weakly monotone by construction, so replacing
//!   each hole by something that dominates its true value can only increase the
//!   result.
//!
//! Therefore `bound[holes := truth] >= bound[holes := true region costs]`, and
//! if the engine is sound the right-hand side is `>= truth`. A failure of the
//! left-hand inequality is a failure of the engine, never of this reading.
//!
//! # Why this file reads the libraries rather than driving the binary
//!
//! Same reason as its neighbours: the assertions are about the *shape* of a
//! bound — which label it carries, which variables it mentions, whether a hole
//! is named `call` or `exceptional-control-flow` — and the process boundary
//! offers only the rendered string, which these tests are forbidden to pin.

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
    let mut functions = landav_python::lower_module(Path::new("exceptional.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    assert_eq!(
        functions.len(),
        1,
        "expected exactly one function in:\n{source}"
    );
    functions.remove(0)
}

/// Everything a failure message needs about a result, in one line.
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
    format!(
        "{kind}({bound}) exact_outside_holes={} holes={holes:?}",
        result.exact_outside_holes()
    )
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

/// Evaluate a result with the parameters bound to `params` and **every hole
/// bound to `hole_value`**.
///
/// See the module docs for why filling holes with the program's own true cost
/// keeps the reading a necessary condition of soundness rather than an
/// additional demand.
fn value_with_holes_at(result: &TripCount, params: &[(&str, u64)], hole_value: u64) -> Nat {
    let bound = result
        .bound()
        .unwrap_or_else(|| panic!("`Unknown` carries no bound: {}", describe(result)));

    let mut table: BTreeMap<Symbol, u64> = params
        .iter()
        .map(|(name, value)| (Symbol::from(*name), *value))
        .collect();
    for var in bound.vars() {
        if Hole::is_hole(&var) {
            table.insert(var.symbol().clone(), hole_value);
        } else {
            assert!(
                table.contains_key(var.symbol()),
                "the bound mentions `{}`, which is neither a parameter this test \
                 supplies nor a hole, so it evaluates to zero for every caller \
                 and the number is not a function of the inputs: {}",
                var.symbol().as_str(),
                describe(result)
            );
        }
    }
    bound.eval(&Bindings(table))
}

/// **The direction that must never be violated.**
///
/// `truth` is a step count this analyst executed by hand — the arithmetic is in
/// each caller's doc comment — and it is always the cost of a run the program
/// genuinely performs, never a hypothetical worst case. A bound below it is a
/// bound the program exceeds.
fn assert_never_below_the_truth(
    result: &TripCount,
    params: &[(&str, u64)],
    truth: u64,
    source: &str,
) {
    let reported = value_with_holes_at(result, params, truth);
    assert!(
        reported.magnitude_cmp(Nat::Fin(truth)) != core::cmp::Ordering::Less,
        "SOUNDNESS: at {params:?} this program executes {truth} steps and the \
         reported bound evaluates to {reported:?}.\n\n\
         A cost bound may be loose in one direction only. Too large is a bound \
         a user over-provisions against; too small is a bound a budget gate \
         passes and production breaks. Every hole here was filled with {truth} \
         — the whole program's cost, which dominates any single region inside \
         it — so no unfilled region can be what closed the gap.\n\
         got: {}\nfor:\n{source}",
        describe(result)
    );
}

/// **The label must not claim an equality the program does not satisfy.**
///
/// Three answers are acceptable and one is not. `AtMost` is the honest complete
/// answer; `Partial` with `exact_elsewhere: false` is the honest partial one;
/// `Unknown` claims nothing. `Exact` — and equally `Partial` still asserting it
/// was exact outside its holes — says the cost *equals* the number, which an
/// early exit makes false.
fn assert_not_an_equality_claim(result: &TripCount, why: &str, source: &str) {
    assert!(
        !result.is_exact(),
        "LABEL: {why}\nA `Theta` claim asserts the program costs this much, not \
         merely at most this much, and an execution that leaves early costs \
         less. `TripCount::relax` is the method for exactly this.\n\
         got: {}\nfor:\n{source}",
        describe(result)
    );
    assert!(
        !result.exact_outside_holes(),
        "LABEL: {why}\n`exact_elsewhere` is the claim that everything outside \
         the holes was derived exactly, and it is rendered to the user as \
         \"exact except for the region at line N\". The trip count of a loop an \
         exception can leave is not among the things derived exactly, so this \
         flag must be false — `TripCount::relax` clears it.\n\
         got: {}\nfor:\n{source}",
        describe(result)
    );
}

/// The bound must be written in terms of `name`.
///
/// The whole value of this lane is the 109 stdlib functions whose only obstacle
/// is a `try`. Turning the `try` into one opaque hole that swallows its body
/// unblocks none of them: the loop inside it is what carries the scale, and a
/// bound that has never heard of `n` is not a bound on anything.
fn assert_mentions(result: &TripCount, name: &str, why: &str, source: &str) {
    let names = mentioned(result);
    assert!(
        names.iter().any(|var| var == name),
        "DERIVATION: {why}\nThe bound must be a function of `{name}`, but it \
         mentions {names:?}. A `try` charged as a single opaque region hides \
         every loop inside it, which is the state this ticket exists to leave.\n\
         got: {}\nfor:\n{source}",
        describe(result)
    );
}

/// One row of the soundness table: the source, the inputs, and the hand-computed
/// truth at those inputs.
type SoundnessCase = (&'static str, &'static [(&'static str, u64)], u64);

/// Convenience: translate, analyse, and hand back the result.
fn analyse(source: &str) -> TripCount {
    cost(only_function(source).program())
}

// ---------------------------------------------------------------------------
// trap 1: an exceptional exit is an edge out of the loop
// ---------------------------------------------------------------------------

/// **A `raise` in a loop body makes the trip count an upper bound, not an
/// equality.**
///
/// ```python
/// def g(n: int) -> int:
///     for i in range(n):
///         raise ValueError
///     return 0
/// ```
///
/// By hand, at `n = 3`: the loop's own step for the first iteration (1), then
/// the `raise` (1). The exception leaves the function; iterations two and three
/// never happen and `return 0` never runs. **Two steps.** At `n = 0` the loop
/// body never runs and the `return` does: **one step**. The count is `n` and the
/// cost is `2` — the gap is unbounded in `n`.
///
/// A bound of `1 + 2n` is perfectly sound here and this test does not object to
/// it. What it objects to is the *label*: `Theta(1 + 2n)` says the program costs
/// `1 + 2n`, and at `n = 100` it costs two.
///
/// The second shape is the one that will actually be met in the stdlib — a
/// guarded raise, which may or may not fire. It is no better: the engine
/// performs no reachability analysis, so it has no evidence the loop runs to
/// completion, and the count is an upper bound either way.
#[test]
fn a_raise_in_a_loop_is_never_reported_as_an_equality() {
    let unconditional =
        "def g(n: int) -> int:\n    for i in range(n):\n        raise ValueError\n    return 0\n";
    let guarded = "def g(n: int) -> int:\n    x = 0\n    for i in range(n):\n        x = x + 1\n        if x > 4:\n            raise ValueError\n    return 0\n";

    for source in [unconditional, guarded] {
        let result = analyse(source);
        assert_not_an_equality_claim(
            &result,
            "a `raise` in a loop body is an edge out of the iteration space, \
             exactly as a `return` is — `analyse::loop_cost` already relaxes for \
             the latter and must for this",
            source,
        );
    }

    // The number stays sound at every input, including the ones where the loop
    // does run to completion.
    let result = analyse(unconditional);
    assert_never_below_the_truth(&result, &[("n", 0)], 1, unconditional);
    assert_never_below_the_truth(&result, &[("n", 3)], 2, unconditional);
    assert_never_below_the_truth(&result, &[("n", 100)], 2, unconditional);
}

// ---------------------------------------------------------------------------
// trap 2: a `try` does not cost its body
// ---------------------------------------------------------------------------

/// **A bare `try`/`except` is at least the worse of its two paths, and its body
/// is derived rather than swallowed.**
///
/// ```python
/// def g(n: int) -> int:
///     x = 0
///     try:
///         for i in range(n):
///             x = x + 1
///     except ValueError:
///         x = 2
///     return x
/// ```
///
/// By hand on the normal path: `x = 0` (1), then `n` iterations each costing
/// the body statement plus the loop's own step (`2n`), then `return x` (1).
/// **`2 + 2n` steps**, and this is a run the program genuinely performs — the
/// body is integer arithmetic, which in this fragment cannot raise.
///
/// The exceptional path is bounded by `1 + 2n + 1 + 1 = 3 + 2n`: some prefix of
/// the loop, the handler, the `return`. So `3 + 2n` dominates both and is the
/// honest answer; `2 + 2n` would also be defensible if the engine can see
/// nothing in the body raises. Either way the bound must be *at least* `2 + 2n`
/// and must mention `n`.
///
/// Today it is `Partial(2 + #hole0)`: the whole `try` is one region, the loop
/// inside it is never walked, and the bound has never heard of `n`. That is
/// sound and useless, and it is the state this ticket exists to leave.
#[test]
fn a_try_except_is_at_least_the_worse_of_its_two_paths() {
    let source = "def g(n: int) -> int:\n    x = 0\n    try:\n        for i in range(n):\n            x = x + 1\n    except ValueError:\n        x = 2\n    return x\n";
    let result = analyse(source);

    assert_mentions(
        &result,
        "n",
        "the loop is inside the `try`, and it is where all the cost is",
        source,
    );
    for n in [0_u64, 1, 5, 40] {
        assert_never_below_the_truth(&result, &[("n", n)], 2 + 2 * n, source);
    }
    assert_not_an_equality_claim(
        &result,
        "which of the two paths runs is not decided by anything the engine can \
         see, so the maximum over them is attained only if the expensive one is \
         reachable — `TripCount::branching` makes exactly this argument for `if`",
        source,
    );
}

// ---------------------------------------------------------------------------
// trap 3: `finally` runs on both paths
// ---------------------------------------------------------------------------

/// **`finally` is charged on the normal path as well as the exceptional one.**
///
/// ```python
/// def g(n: int, m: int) -> int:
///     try:
///         for i in range(n):
///             x = 0
///     finally:
///         for j in range(m):
///             y = 0
///     return 0
/// ```
///
/// By hand on the normal path: `2n` for the `try` loop, `2m` for the `finally`
/// loop, `1` for the `return`. **`1 + 2n + 2m` steps**, and it is executed —
/// nothing in either loop can raise.
///
/// The arrangement this test exists to reject is charging the `finally` to the
/// exceptional path only, on the reading that `finally` is "the cleanup when
/// something goes wrong". Then the normal path reads `2n + 1`, the exceptional
/// path reads `2n + 2m`, and the maximum of the two is
/// `max(2n + 1, 2n + 2m)` — which at `n = 6, m = 0` is 13 against a truth of
/// 13, fine, but at `m = 0` the two agree by accident. Take `n = 6, m = 6`:
/// that reading gives `max(13, 24) = 24` against a truth of `25`. It
/// **understates by one for every `m`**, and by the whole `finally` body if the
/// normal path is the one that drops it.
///
/// The correct shape is `max(normal, exceptional) + finally`: the `finally`
/// body is outside the maximum because it runs whichever way the `try` went.
#[test]
fn a_finally_is_charged_on_both_paths() {
    let source = "def g(n: int, m: int) -> int:\n    try:\n        for i in range(n):\n            x = 0\n    finally:\n        for j in range(m):\n            y = 0\n    return 0\n";
    let result = analyse(source);

    assert_mentions(
        &result,
        "n",
        "the `try` body's loop runs on the normal path",
        source,
    );
    assert_mentions(
        &result,
        "m",
        "the `finally` body runs on EVERY path — a bound that does not mention \
         `m` has charged it on no path at all",
        source,
    );
    for (n, m) in [(0_u64, 0_u64), (6, 6), (6, 0), (0, 6), (30, 3)] {
        assert_never_below_the_truth(&result, &[("n", n), ("m", m)], 1 + 2 * n + 2 * m, source);
    }
}

// ---------------------------------------------------------------------------
// trap 4: an `except` is not an `else`
// ---------------------------------------------------------------------------

/// **The handler's cost adds to a prefix of the body, rather than replacing the
/// body.**
///
/// ```python
/// def g(n: int, m: int) -> int:
///     try:
///         for i in range(n):
///             x = 0
///         raise ValueError
///     except ValueError:
///         for j in range(m):
///             y = 0
///     return 0
/// ```
///
/// The `raise` is at the *end* of the `try` body, so the whole body has already
/// been paid before the handler starts. By hand: `2n` for the loop, `1` for the
/// `raise`, `2m` for the handler's loop, `1` for the `return`. **`2 + 2n + 2m`
/// steps**, executed exactly, for every `n` and `m`.
///
/// The wrong model is `max(body, handler)`, which reads `except` as though it
/// were the `else` arm of an `if` — the handler runs *instead of* the body. At
/// `n = m = 4` that gives `max(9, 8) = 9` against a truth of `18`: a factor of
/// two, growing without limit as the two loops grow together. It is wrong
/// because an exception is raised from *inside* the body, so the body's cost up
/// to that point is already spent and the handler is added on top.
///
/// A sound engine may not know *where* in the body the exception was raised, so
/// the honest upper bound is `body + handler` — the maximum over prefixes is
/// the whole body. That over-approximates whenever the raise is early, which is
/// why the result may not be exact.
#[test]
fn an_except_handler_adds_to_a_body_prefix() {
    let source = "def g(n: int, m: int) -> int:\n    try:\n        for i in range(n):\n            x = 0\n        raise ValueError\n    except ValueError:\n        for j in range(m):\n            y = 0\n    return 0\n";
    let result = analyse(source);

    assert_mentions(
        &result,
        "n",
        "the `try` body's loop is paid in full before the `raise`",
        source,
    );
    assert_mentions(&result, "m", "the handler's loop is paid after it", source);
    for (n, m) in [(4_u64, 4_u64), (0, 0), (10, 1), (1, 10)] {
        assert_never_below_the_truth(&result, &[("n", n), ("m", m)], 2 + 2 * n + 2 * m, source);
    }
}

// ---------------------------------------------------------------------------
// trap 5: `with` is two calls
// ---------------------------------------------------------------------------

/// **`with` runs `__enter__` and `__exit__`, and a call is a hole.**
///
/// ```python
/// def g(n: int) -> int:
///     with lock:
///         x = 1
///     return x
/// ```
///
/// `with lock:` evaluates `lock`, calls `lock.__enter__()`, runs the body, and
/// calls `lock.__exit__(...)` — on the normal path *and* on the exceptional
/// one, which is the entire reason the statement exists. Both are calls to
/// arbitrary user code. `landav-python` already refuses a call as
/// `Construct::Call` wherever it can see one, and these two are no different
/// for being implicit.
///
/// The failure this pins is a `with` that reports a complete bound. There is no
/// number for `__exit__`; charging it zero is the same defect
/// `calls_become_holes.rs` was written for, one level further down, and harder
/// to spot because the call does not appear in the source text.
///
/// A hole named `exceptional-control-flow` is *not* good enough. The name is
/// what the user acts on, and "we could not analyse your exception handling" is
/// not actionable advice about a context manager whose `__exit__` we would need
/// a bound for. The construct tag must say `call`.
#[test]
fn a_with_statement_charges_its_implicit_exit_as_a_call() {
    let source = "def g(n: int) -> int:\n    with lock:\n        x = 1\n    return x\n";
    let result = analyse(source);
    let shown = describe(&result);

    assert!(
        !result.is_complete(),
        "`with` calls `__enter__` and `__exit__`, whose costs are unknown, so \
         no complete bound is available for a function containing one. A \
         complete bound here has charged two calls to arbitrary user code as \
         zero: {shown}\nfor:\n{source}"
    );
    let tags: Vec<&str> = result.holes().iter().map(Hole::construct).collect();
    assert!(
        tags.contains(&"call"),
        "the implicit `__exit__` is a CALL and must be blamed as one — a hole \
         tagged {tags:?} tells the user their exception handling was refused, \
         when what they need to know is that a context manager's `__exit__` has \
         no bound. `landav-python` refuses every explicit call as \
         `Construct::Call`; an implicit one is not a different kind of \
         thing.\ngot: {shown}\nfor:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// composition: per-iteration charging, and nesting
// ---------------------------------------------------------------------------

/// **A `try` in a loop body is paid once per iteration.**
///
/// ```python
/// def g(n: int, m: int) -> int:
///     for i in range(n):
///         try:
///             for j in range(m):
///                 x = 0
///         except ValueError:
///             x = 1
///     return 0
/// ```
///
/// By hand on the normal path: each of the `n` outer iterations costs the outer
/// loop's own step (1) plus the inner loop (`2m`), and then `return 0` (1).
/// **`1 + n + 2mn` steps.**
///
/// The arithmetic separates every plausible wrong answer at `n = m = 4`:
///
/// | answer | value | what went wrong |
/// |---|---|---|
/// | `1 + n + 2mn` — correct | 37 | |
/// | `1 + n + 2m` | 13 | the `try` charged once, outside the loop |
/// | `1 + n` | 5 | the `try` body dropped entirely |
/// | `1 + 2mn` | 33 | the outer loop's own step dropped |
///
/// The middle row is the one to watch. A `try` handled by rewriting it into a
/// flat "cost of body plus cost of handler" *before* the loop walk sees it
/// would land there, and the understatement grows with the trip count.
#[test]
fn a_try_inside_a_loop_is_paid_once_per_iteration() {
    let source = "def g(n: int, m: int) -> int:\n    for i in range(n):\n        try:\n            for j in range(m):\n                x = 0\n        except ValueError:\n            x = 1\n    return 0\n";
    let result = analyse(source);

    assert_mentions(&result, "n", "the outer loop", source);
    assert_mentions(
        &result,
        "m",
        "the inner loop is inside the `try`, and a bound that does not mention \
         `m` has swallowed the whole body",
        source,
    );
    for (n, m) in [(4_u64, 4_u64), (0, 9), (9, 0), (7, 3)] {
        assert_never_below_the_truth(&result, &[("n", n), ("m", m)], 1 + n + 2 * m * n, source);
    }
}

/// **Nesting `try` inside `try` does not lose the innermost body.**
///
/// ```python
/// def g(n: int) -> int:
///     try:
///         try:
///             for i in range(n):
///                 x = 0
///         except ValueError:
///             x = 1
///     except TypeError:
///         x = 2
///     return 0
/// ```
///
/// By hand on the normal path: `2n` for the loop and `1` for the `return`.
/// **`1 + 2n` steps.**
///
/// This is the recursion check. Whatever rule handles one `try` has to be the
/// rule that handles the inner one too — a rule stated only for a `try` whose
/// body is straight-line statements would report the inner handler correctly
/// and the outer one over a body it never analysed. The observable symptom is
/// the same one as everywhere else in this file: `n` goes missing.
#[test]
fn a_nested_try_still_derives_its_innermost_body() {
    let source = "def g(n: int) -> int:\n    try:\n        try:\n            for i in range(n):\n                x = 0\n        except ValueError:\n            x = 1\n    except TypeError:\n        x = 2\n    return 0\n";
    let result = analyse(source);

    assert_mentions(
        &result,
        "n",
        "the loop is two `try` levels down and is still where all the cost is",
        source,
    );
    for n in [0_u64, 1, 12] {
        assert_never_below_the_truth(&result, &[("n", n)], 1 + 2 * n, source);
    }
}

/// **Everything after an unconditional `raise` is unreachable.**
///
/// ```python
/// def g(n: int) -> int:
///     raise ValueError
///     for i in range(n):
///         x = 0
///     return 0
/// ```
///
/// By hand: the `raise` (1). Nothing else runs, ever, for any `n`. **One step.**
///
/// Both directions are pinned, and they pull against each other, which is why
/// they are asserted together:
///
/// * the bound may not be *below* one — the `raise` itself is a statement and
///   is executed;
/// * the bound may not be labelled an equality while it charges the loop, for
///   the same reason `body_cost` relaxes a statement list whose tail sits past
///   a `return`: the sum still dominates, but it is no longer attained.
///
/// An implementer who chooses instead to *drop* the unreachable tail — reporting
/// `Theta(1)` — satisfies both, and should: that is a genuine equality. The
/// assertion is written to permit it. What it forbids is `Theta(1 + 2n)`, which
/// is the answer `body_cost` would give if an exceptional exit were charged like
/// an ordinary statement and `returns_within` were left unextended.
#[test]
fn an_unconditional_raise_leaves_the_rest_of_the_function_unreachable() {
    let source = "def g(n: int) -> int:\n    raise ValueError\n    for i in range(n):\n        x = 0\n    return 0\n";
    let result = analyse(source);
    let shown = describe(&result);

    for n in [0_u64, 5, 50] {
        assert_never_below_the_truth(&result, &[("n", n)], 1, source);
    }

    let claims_equality = result.is_exact() || result.exact_outside_holes();
    let charges_the_loop = mentioned(&result).iter().any(|name| name == "n");
    assert!(
        !(claims_equality && charges_the_loop),
        "LABEL: this program costs one step for every `n`, and the loop after \
         the `raise` never runs. Reporting a bound that grows with `n` is sound; \
         reporting one that grows with `n` and calling it an equality is not — \
         `body_cost` already relaxes a statement list whose tail is reachable \
         only past a `return`, and an exceptional exit is the same shape. \
         Dropping the tail and reporting `Theta(1)` is the better answer and is \
         permitted here.\ngot: {shown}\nfor:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// the central soundness net
// ---------------------------------------------------------------------------

/// **THE ONE THAT MUST NEVER FAIL: no reported bound is below the true cost.**
///
/// Eight programs, each with its step count computed by hand below. Every truth
/// is the cost of a run the program genuinely performs, not a hypothetical
/// worst case, so a reported bound below it is a bound the program *has already*
/// exceeded.
///
/// The unit is the engine's declared one: one step per statement executed, plus
/// one per loop iteration for the loop's own test and increment. `pass`
/// translates to no statement and costs nothing.
///
/// | # | program | at | truth | arithmetic |
/// |---|---|---|---|---|
/// | 1 | `try: x = 1 / except: x = 2 / return x` | — | 2 | body (1) + `return` (1) |
/// | 2 | `try: x = 1 / finally: x = 2 / return x` | — | 3 | body (1) + finally (1) + `return` (1) |
/// | 3 | `assert n > 0 / return n` | n=5 | 2 | assert (1) + `return` (1) |
/// | 4 | loop in `try`, loop in `except` | n=6,m=6 | 13 | `2n` (12) + `return` (1); handler unreached |
/// | 5 | loop in `try`, loop in `finally` | n=6,m=6 | 25 | `2n` (12) + `2m` (12) + `return` (1) |
/// | 6 | raise at end of body, loop in handler | n=6,m=6 | 26 | `2n` (12) + raise (1) + `2m` (12) + `return` (1) |
/// | 7 | `try` per iteration of a loop | n=5,m=5 | 56 | `n * (1 + 2m)` (55) + `return` (1) |
/// | 8 | **`try` reassigns the trip count** | n=5 | 52 | `n = n * n` (1) + `2n^2` (50) + `return` (1) |
///
/// # Row 8 is the one that keeps this file honest
///
/// ```python
/// def g(n: int) -> int:
///     try:
///         n = n * n
///     except ValueError:
///         n = 0
///     for i in range(n):
///         x = 0
///     return 0
/// ```
///
/// The loop after the `try` runs `n * n` times on the normal path, and its trip
/// count is a value the `try` body wrote. This is `LAN-87a`'s defect reached
/// through the new construct: an engine that walks into the `try` body to
/// derive its cost, and forgets that walking in also means the variables it
/// assigns are no longer the caller's, reports `Theta(2 + 2n)` — **12** at
/// `n = 5` against a truth of **52**. Complete, exact, no holes, offered to a
/// budget gate.
///
/// `analyse::Walk::region` is the single place that rule is enforced today, and
/// it is enforced by *clearing* `readable`. Any implementation of this ticket
/// that stops routing a `try` body through `region` inherits the obligation to
/// clear or narrow `readable` itself. Worse than an ordinary assignment: which
/// statements in the body ran at all depends on where the exception hit, so
/// neither the entry value nor the post-body value is the one that holds.
#[test]
fn a_reported_bound_is_never_below_the_true_cost() {
    let cases: &[SoundnessCase] = &[
        (
            "def g(n: int) -> int:\n    try:\n        x = 1\n    except ValueError:\n        x = 2\n    return x\n",
            &[("n", 4)],
            2,
        ),
        (
            "def g(n: int) -> int:\n    try:\n        x = 1\n    finally:\n        x = 2\n    return x\n",
            &[("n", 4)],
            3,
        ),
        (
            "def g(n: int) -> int:\n    assert n > 0\n    return n\n",
            &[("n", 5)],
            2,
        ),
        (
            "def g(n: int, m: int) -> int:\n    try:\n        for i in range(n):\n            x = 0\n    except ValueError:\n        for j in range(m):\n            y = 0\n    return 0\n",
            &[("n", 6), ("m", 6)],
            13,
        ),
        (
            "def g(n: int, m: int) -> int:\n    try:\n        for i in range(n):\n            x = 0\n    finally:\n        for j in range(m):\n            y = 0\n    return 0\n",
            &[("n", 6), ("m", 6)],
            25,
        ),
        (
            "def g(n: int, m: int) -> int:\n    try:\n        for i in range(n):\n            x = 0\n        raise ValueError\n    except ValueError:\n        for j in range(m):\n            y = 0\n    return 0\n",
            &[("n", 6), ("m", 6)],
            26,
        ),
        (
            "def g(n: int, m: int) -> int:\n    for i in range(n):\n        try:\n            for j in range(m):\n                x = 0\n        except ValueError:\n            x = 1\n    return 0\n",
            &[("n", 5), ("m", 5)],
            56,
        ),
        (
            "def g(n: int) -> int:\n    try:\n        n = n * n\n    except ValueError:\n        n = 0\n    for i in range(n):\n        x = 0\n    return 0\n",
            &[("n", 5)],
            52,
        ),
    ];

    for (source, params, truth) in cases {
        let result = analyse(source);
        assert_never_below_the_truth(&result, params, *truth, source);
    }
}

/// **A complete bound never mentions a variable the caller cannot supply.**
///
/// The companion to the soundness net, and the reason it cannot be satisfied by
/// smuggling. A hole variable in a *complete* result is a finite-looking claim
/// with an `omega`-valued term in it, which every consumer supplying only the
/// parameters reads as zero — the exact mechanism by which
/// `close_over_counter` once published `O(2 + n * (1 + #hole0 + 2n))` with an
/// empty hole ledger. A local or a loop counter is the same defect with a
/// different name.
///
/// Applied across every program in this file, because the risk is a `try`
/// rewrite that reports the handler's cost in terms of a name bound inside the
/// handler, or a `with` that names its context variable.
#[test]
fn no_bound_mentions_something_the_caller_cannot_supply() {
    let sources = [
        "def g(n: int) -> int:\n    try:\n        for i in range(n):\n            x = 0\n    except ValueError as e:\n        x = 1\n    return 0\n",
        "def g(n: int) -> int:\n    with lock as handle:\n        x = 1\n    return x\n",
        "def g(n: int) -> int:\n    try:\n        for i in range(n):\n            x = 0\n    finally:\n        for j in range(n):\n            y = 0\n    return 0\n",
        "def g(n: int) -> int:\n    for i in range(n):\n        raise ValueError\n    return 0\n",
    ];

    for source in sources {
        let function = only_function(source);
        let params: Vec<String> = function
            .program()
            .params()
            .iter()
            .map(|param| param.symbol().as_str().to_owned())
            .collect();
        let result = cost(function.program());
        let partial = !result.is_complete();
        for name in mentioned(&result) {
            assert!(
                params.contains(&name) || (partial && name.starts_with("#hole")),
                "the bound mentions `{name}`, which is not a parameter of `g`. \
                 The caller has nothing to supply for it, so `Bound::eval` reads \
                 it as zero and whatever cost it carried vanishes.\n\
                 got: {}\nfor:\n{source}",
                describe(&result)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// what must stay a hole
// ---------------------------------------------------------------------------

/// **The shapes that cannot be modelled soundly stay holes, and a later
/// implementer may not quietly accept them.**
///
/// Every entry here is a case where an exceptional exit interacts with
/// something the engine already declines to reason about, and where "we made
/// `try` work" is not an argument for accepting it. A hole is the right answer,
/// and this test exists so that broadening the `try` rule cannot silently take
/// it away.
///
/// * **`while ... else`** — `landav-python` refuses this as
///   `ExceptionalControlFlow`, and the `else` clause runs exactly when the loop
///   finished without a `break`. Deciding that requires the ranking argument the
///   engine does not have for the `while` in the first place.
/// * **A `try` around a `while`** — the `try` becoming analysable must not make
///   the `while` inside it so. A `while` costs a hole because its trip count
///   needs a ranking function, and wrapping it in an exception handler supplies
///   none.
/// * **A `try` whose body reassigns the loop bound** — the `n` seen by the loop
///   after the `try` is neither the caller's value nor the body's result, since
///   which statements ran depends on where the exception hit. There is no
///   expression for it, so there is no complete bound.
///
/// The assertion is deliberately weak — *a hole exists* — rather than pinning
/// today's exact answer. Pinning the answer would fail the moment the surrounding
/// derivation improves, which is progress, and a test that punishes progress
/// gets deleted rather than heeded.
#[test]
fn the_shapes_that_cannot_be_modelled_soundly_stay_holes() {
    let cases: &[(&str, &str)] = &[
        (
            "def g(n: int) -> int:\n    i = 0\n    while i < n:\n        i = i + 1\n    else:\n        x = 1\n    return 0\n",
            "`while ... else` needs to know the loop finished without a `break`, \
             which needs the ranking argument the `while` itself does not have",
        ),
        (
            "def g(n: int) -> int:\n    try:\n        i = 0\n        while i < n:\n            i = i + 1\n    except ValueError:\n        x = 1\n    return 0\n",
            "a `while` inside a `try` still needs a ranking function; making the \
             `try` analysable supplies none",
        ),
        (
            "def g(n: int) -> int:\n    try:\n        n = n * n\n    except ValueError:\n        n = 0\n    for i in range(n):\n        x = 0\n    return 0\n",
            "the trip count is a value the `try` body wrote, and which statements \
             of that body ran depends on where the exception hit — so neither \
             the entry value nor the post-body value is the one that holds",
        ),
    ];

    for (source, why) in cases {
        let result = analyse(source);
        assert!(
            !result.is_complete(),
            "REFUSAL: {why}.\nA complete bound here is a finite claim the engine \
             has no argument for. If a later ticket finds one, it belongs in a \
             test that states it — not in the quiet disappearance of this \
             one.\ngot: {}\nfor:\n{source}",
            describe(&result)
        );
        assert!(
            !result.holes().is_empty(),
            "REFUSAL: {why}.\nAn unanalysable construct must be a named, placed \
             region rather than a bare `Unknown` — `Unknown` tells the user \
             nothing they can act on.\ngot: {}\nfor:\n{source}",
            describe(&result)
        );
    }
}

/// **A refused construct is still refused by the lowering.**
///
/// The engine's reach and the transition system's are two different numbers.
/// `Update` is a total map with no havoc, so a transition system admitting a
/// `try` would assert the integer state is unchanged across a body that may
/// have run in part — which is worse than refusing it. `Coverage::lowered()` is
/// the headline number and it must not move because the engine learned
/// something.
///
/// This is `calls_become_holes.rs`'s last test, restated for this lane, and it
/// is the half of the ticket that is easiest to lose: an implementer who makes
/// the engine total over `try` by teaching `landav-python` to emit real
/// statements for it will pass every test above and silently start lowering
/// programs whose integer state nobody can account for.
#[test]
fn exceptional_control_flow_still_stops_a_program_from_lowering() {
    let sources = [
        "def g(n: int) -> int:\n    try:\n        x = 1\n    except ValueError:\n        x = 2\n    return x\n",
        "def g(n: int) -> int:\n    try:\n        x = 1\n    finally:\n        x = 2\n    return x\n",
        "def g(n: int) -> int:\n    for i in range(n):\n        raise ValueError\n    return 0\n",
        "def g(n: int) -> int:\n    with lock:\n        x = 1\n    return x\n",
        "def g(n: int) -> int:\n    assert n > 0\n    return n\n",
    ];

    for source in sources {
        let function = only_function(source);
        let refused = landav_its::lower(function.program());
        assert!(
            refused.is_err(),
            "exceptional control flow must still stop a program from lowering: \
             the engine's reach is a second number, not a change to \
             coverage:\n{source}"
        );
        // Non-negotiable 3: a refusal carries blame. Which construct is named
        // may legitimately change - a `with` refused for its implicit
        // `__exit__` is a `Call` refusal, not an exception one - so the
        // assertion is that some construct is named, not which.
        let constructs = refused
            .as_ref()
            .err()
            .and_then(landav_its::LoweringError::refusals)
            .map(landav_its::Refusals::constructs)
            .unwrap_or_default();
        assert!(
            !constructs.is_empty(),
            "the refusal must name what it refused - an unnamed refusal cannot \
             be counted, grouped, or acted on:\n{source}"
        );
    }
}
