//! The last open half of the cross-cutting hazard: **a call written inside a
//! call's arguments is never translated at all.**
//!
//! # The shape, and why it is the one that still bites
//!
//! `landav_python`'s `expression_children` returns `Vec::new()` for every
//! *refused* form. That is correct and deliberate for a form whose
//! `Unsupported` node stands for the whole region: the node denotes `omega`,
//! `omega` dominates whatever is written inside it, and translating the
//! interior would leave every refusal in it pointing at nothing - the orphan
//! shape LAN-90 closed.
//!
//! The construct lanes then went through the vocabulary and, for each container
//! they *accepted*, added the matching `expression_children` arm: a display and
//! an f-string in a discarded position (`[g(n), 2]`), a boolean, a comparison
//! and a ternary in a position that reads no value (`return f(n) or g(n)`).
//! Both are pinned - `collection_literals.rs` and
//! `discarded_expressions::a_discarded_boolean_names_every_call_inside_it`.
//!
//! One container was never revisited, because it is not *accepted* and so
//! looked safe: the **call itself**. `fetch(g(n))` builds exactly one
//! `Unsupported` node, for `fetch`. `g(n)` is not translated, is not in the
//! arena, and has no hole anywhere. Nothing in the report distinguishes
//! `fetch(n)` from `fetch(g(n))`, and `landav_engine::Walk::reconciled` cannot
//! notice, because a node that was never built is not in the arena to
//! reconcile against.
//!
//! # Why "sound for the cost bound" does not settle it
//!
//! For the **cost** bound the argument holds: `#hole0` stands for the cost of
//! evaluating the whole `fetch(...)` region, nested call included, so a
//! consumer that substitutes a real cost for it is not being lied to.
//!
//! For the **query count** it does not, and `queries` is a derived number the
//! CLI prints today. `crates/landav-cli/src/resource_bound.rs` reads the
//! coefficient of the `call` holes out of that same bound: one node, one call,
//! `queries = 1`, `bound_kind = "exact"`. Two call expressions are evaluated.
//! That is an **under-count in a confident position**, which
//! `resource_registry.rs` names as the one direction a resource bound may never
//! go - a loose bound is unhelpful, a low one is a gate waving a function
//! through with a number that looks entirely reasonable on the way past.
//! `resource_bound.rs`'s own module documentation says so, and ends with the
//! sentence this file exists to delete: "until that lands, `queries` must not
//! be used as a hard budget gate on code that nests calls inside call
//! arguments."
//!
//! # What it is worth, measured rather than asserted
//!
//! Joining a Python `ast` walk of each corpus against
//! `landav check <corpus> --json --resource queries`, over the module-level
//! functions landav actually analyses, deduplicated by real path:
//!
//! | corpus | confident `queries` numbers | of those, under-counts | calls missing |
//! |---|---|---|---|
//! | `/usr/lib/python3.12` | 568 | **185 (32.6%)** | 311 |
//! | the typed corpus | 1773 | **342 (19.3%)** | 883 |
//!
//! The worst single case is an Alembic migration's `upgrade`, which reports
//! `queries = 10` and issues 78. A seven-fold under-count on a function whose
//! entire job is issuing database operations is the exact failure the resource
//! is named for.
//!
//! # The three neighbouring shapes, checked and reported rather than assumed
//!
//! The brief asked whether the same hole exists inside an f-string, a subscript
//! and an attribute chain. Measured, they are three different answers:
//!
//! * **f-string**: `f"{g(n)}"` written on its own is already right - the
//!   `JoinedStr` arm of `expression_children` descends in a discarded position.
//!   `log(f"{g(n)}")` is wrong, and the container to blame is the *call*, not
//!   the f-string. It is here as a case, not as a fourth gap.
//! * **subscript** and **attribute**: `table[g(n)]` and `g(n).field` report
//!   `bound_kind: "partial"` with **no number at all**, because the subscript
//!   and attribute regions survive into the query bound as unfilled variables.
//!   Honest and weak, never low. They are regression guards below, and the fix
//!   must not turn either into a confident number.
//!
//! # The fence on the fix
//!
//! Descending into a refused call's arguments must not resurrect the orphan
//! LAN-90 closed. In the hoisted positions - a bare statement, a `return`, an
//! assignment's right-hand side - `Translator::hoisted` translates into a
//! scratch builder and re-emits every refusal as a placed statement, so an
//! interior refusal has somewhere to live. A **condition** is not hoisted, and
//! `an_unsound_query_count_in_a_condition_is_still_unsound` is deliberately in
//! this file so that the harder half is not quietly dropped: it is a confident
//! under-count today (`upper`, `1`) and the fix owes it an answer, whether by
//! attaching the argument to the refused node - the shape
//! `unsupported_expr_bounded` already uses for `//`'s dividend - or by some
//! other route that leaves nothing unreferenced.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::{io, path::Path};

use landav_engine::{Hole, TripCount, cost};
use landav_python::LoweredFunction;
use serde_json::Value;

use common::{Project, Run};

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

/// Translates `source` and returns its single function.
fn only_function(source: &str) -> LoweredFunction {
    let mut functions = landav_python::lower_module(Path::new("nested.py"), source)
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

/// The regions this result blames on `construct`.
fn holes_blamed_on<'a>(result: &'a TripCount, construct: &str) -> Vec<&'a Hole> {
    result
        .holes()
        .iter()
        .filter(|hole| hole.construct() == construct)
        .collect()
}

/// Asserts that `source` names exactly `expected` call regions, each placed,
/// and each at a position of its own.
///
/// The distinct-origins half is not decoration. A translation that visited the
/// inner call but reported it at the outer call's position would satisfy a bare
/// count and would send a reader of the report to the wrong column - and, worse,
/// it is what a fix that reuses the parent's origin produces by accident.
fn assert_names_calls(source: &str, expected: usize) {
    let function = only_function(source);
    let result = cost(function.program());
    let calls = holes_blamed_on(&result, "call");

    assert_eq!(
        calls.len(),
        expected,
        "{expected} call expressions are evaluated here and {expected} regions \
         must be reported. A lower count means a call was never translated at \
         all: `expression_children` gives a refused form no children, so a call \
         written inside a refused call's arguments has no node in the arena, no \
         hole in the result, and nothing anywhere that distinguishes it from a \
         call with a plain argument. Got {} for:\n{source}",
        describe(&result)
    );

    let mut origins: Vec<&str> = calls.iter().map(|hole| hole.origin().as_str()).collect();
    for origin in &origins {
        assert!(
            origin.contains(':'),
            "a region must be placed as well as named, got {origin} for:\n{source}"
        );
    }
    origins.sort_unstable();
    let placed = origins.len();
    origins.dedup();
    assert_eq!(
        origins.len(),
        placed,
        "two call regions share one position, so the inner call was charged \
         where the outer one stands. A reader sent to that column finds one \
         call and is told there are two: {} for:\n{source}",
        describe(&result)
    );
}

/// Run `landav check <fixture> --json --resource queries` over `source`.
fn queries_run(project: &Project, source: &str) -> io::Result<(Run, Value)> {
    let target = project.write("fixture.py", source)?;
    let run = project.check(&target, &["--json", "--resource", "queries"])?;
    let parsed = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not valid JSON ({why}): {}", run.describe()));
    Ok((run, parsed))
}

/// The `resource` block reported for `name`, or [`Value::Null`].
fn resource_of<'a>(parsed: &'a Value, name: &str) -> &'a Value {
    parsed["functions"]
        .as_array()
        .and_then(|functions| functions.iter().find(|entry| entry["name"] == name))
        .map_or(&Value::Null, |entry| &entry["resource"])
}

/// The query count reported for `name`, or `None` if no number was reported.
///
/// Total on purpose, exactly as `resource_registry.rs`'s equivalent is: a
/// missing function, a missing block and a symbolic count all collapse to
/// `None` so that the caller's own message survives, rather than being replaced
/// by a helper's panic.
fn queries(parsed: &Value, name: &str) -> Option<i64> {
    resource_of(parsed, name)["value"].as_i64()
}

/// The `bound_kind` reported for `name`'s query count, if any.
fn queries_kind<'a>(parsed: &'a Value, name: &str) -> Option<&'a str> {
    resource_of(parsed, name)["bound_kind"].as_str()
}

// ---------------------------------------------------------------------------
// 1. the regions, from the library
// ---------------------------------------------------------------------------

/// **`fetch(g(n))` is two call regions, not one.**
///
/// The headline. Today this function reports a single `call` hole, blamed on
/// `fetch`, and `g` appears nowhere: not in the arena, not in the refusal
/// ledger, not in the bound. The report of `fetch(g(n))` is byte-for-byte the
/// report of `fetch(n)` with a different column number.
#[test]
fn a_call_in_a_call_argument_is_a_second_region() {
    assert_names_calls(
        "def outer(n: int) -> int:\n    fetch(g(n))\n    return n\n",
        2,
    );
}

/// **A call nested two containers deep is still named.**
///
/// `fetch(g(h(n)))` evaluates three calls. One arm added at the top of the
/// argument list is not enough; the descent has to be the ordinary recursive
/// one every other container gets, or the second level goes missing and the
/// count lands on two - which is why this asserts three rather than
/// "more than one".
#[test]
fn a_call_nested_two_deep_is_three_regions() {
    assert_names_calls(
        "def outer(n: int) -> int:\n    fetch(g(h(n)))\n    return n\n",
        3,
    );
}

/// **A nested call beside a plain argument is still named.**
///
/// `fetch(g(n), n)` mixes an argument that refuses with one that does not.
/// A translation that stopped at the first argument it could not read, or that
/// looked only at a single-argument call, passes the previous test and fails
/// this one.
#[test]
fn a_nested_call_beside_a_plain_argument_is_still_named() {
    assert_names_calls(
        "def outer(n: int) -> int:\n    fetch(g(n), n)\n    return n\n",
        2,
    );
}

/// **A nested call in a keyword argument is named.**
///
/// `call.args` and `call.keywords` are two lists in the AST, and a descent
/// written against the first alone is the most likely partial fix. `fetch(key=
/// g(n))` runs `g` exactly as `fetch(g(n))` does.
#[test]
fn a_nested_call_in_a_keyword_argument_is_named() {
    assert_names_calls(
        "def outer(n: int) -> int:\n    fetch(key=g(n))\n    return n\n",
        2,
    );
}

/// **A call inside an f-string inside a call argument is named.**
///
/// Two containers, one of which is already right on its own. `f"{g(n)}"`
/// written as a bare statement produces a `call` region today, because the
/// collection lane added the `JoinedStr` arm to `expression_children`. Put the
/// same f-string in a call's arguments and it vanishes - not because of the
/// f-string, but because the refused call above it stopped the descent before
/// the f-string was ever reached. Pinned so that the f-string is not blamed for
/// a hole that belongs to the call.
#[test]
fn a_call_inside_an_f_string_argument_is_named() {
    assert_names_calls(
        "def outer(n: int) -> int:\n    log(f\"{g(n)}\")\n    return n\n",
        2,
    );
}

// ---------------------------------------------------------------------------
// 2. the number a gate reads
// ---------------------------------------------------------------------------

/// **`queries` reports two for `fetch(g(n))`, and the two shapes stop being
/// indistinguishable.**
///
/// Both halves are asserted in one test on purpose. `fetch(n)` reporting `1` is
/// correct and must stay correct, and the defect is precisely that
/// `fetch(g(n))` reports the same thing. A fix that made the nested case
/// symbolic, or partial, or absent would remove the *lie* without producing the
/// *answer*, and the first assertion would go on passing.
#[test]
fn queries_counts_the_call_written_inside_the_argument() -> io::Result<()> {
    let project = Project::new()?;
    let source = "def plain(n: int) -> int:\n    fetch(n)\n    return n\n\n\n\
                  def nested(n: int) -> int:\n    fetch(g(n))\n    return n\n";
    let (run, parsed) = queries_run(&project, source)?;
    run.assert_did_not_crash();

    assert_eq!(
        queries(&parsed, "plain"),
        Some(1),
        "`fetch(n)` issues one call, and this is the control - if it has moved, \
         nothing below is a statement about nesting.\n{}",
        run.describe()
    );
    assert_ne!(
        queries(&parsed, "nested"),
        Some(1),
        "`fetch(g(n))` reported the same number as `fetch(n)`. Two call \
         expressions are evaluated and one is counted. UNDER-COUNTING IS THE \
         ONE DIRECTION A RESOURCE BOUND MAY NEVER GO, and this one is served \
         with `bound_kind: \"exact\"` - a gate reads it as a settled fact and \
         passes a function issuing twice its budget.\n{}",
        run.describe()
    );
    assert_eq!(
        queries(&parsed, "nested"),
        Some(2),
        "`fetch(g(n))` evaluates `g` and then `fetch`. Two calls issued, two \
         queries. Reporting nothing at all would be honest and is not the \
         answer: the count is derivable, the engine already charges each region \
         where it stands, and the corpus measurement in this file's \
         documentation is 185 stdlib functions whose confident number is \
         low.\n{}",
        run.describe()
    );
    Ok(())
}

/// **A nested call in a loop multiplies, like every other call.**
///
/// The assertion the soundness claim rests on, restated for the shape this file
/// is about. `for i in range(10): fetch(g(i))` issues twenty calls: ten `g` and
/// ten `fetch`. Every wrong answer is separated by the number:
///
/// | answer | means |
/// |---|---|
/// | `10` | the inner call was never translated - today's answer |
/// | `11` | the inner call was charged once, outside the loop |
/// | `2`  | the trip count was dropped and sites were counted |
/// | `20` | correct |
///
/// `11` is the one worth naming: it is what an implementation produces if it
/// records the extra call somewhere other than at its position in the control
/// structure, and it is *also* an under-count, just a less obvious one.
#[test]
fn a_nested_call_in_a_loop_is_counted_once_per_iteration() -> io::Result<()> {
    let project = Project::new()?;
    let source =
        "def looped(n: int) -> int:\n    for i in range(10):\n        fetch(g(i))\n    return n\n";
    let (run, parsed) = queries_run(&project, source)?;
    run.assert_did_not_crash();

    let reported = queries(&parsed, "looped");
    assert_ne!(
        reported,
        Some(10),
        "ten iterations were counted and only the outer call was seen. The \
         inner `g` is evaluated on every one of those iterations too, so this \
         is a half-count multiplied by the trip count - the under-count that \
         grows with the loop.\n{}",
        run.describe()
    );
    assert_ne!(
        reported,
        Some(11),
        "the inner call was charged once rather than once per iteration, which \
         means it was not placed inside the loop's region. A call must be \
         charged where it stands; anywhere else understates every loop that \
         contains it.\n{}",
        run.describe()
    );
    assert_eq!(
        reported,
        Some(20),
        "ten iterations, two calls each. The trip count is a literal and the \
         engine derives it exactly, so twenty is both the truth and what this \
         analysis can see.\n{}",
        run.describe()
    );
    Ok(())
}

/// **A returned nested call is counted too.**
///
/// `return fetch(g(n))` is a hoisted position with no value slot, and it
/// reports `exact`, `1` today. It is the single most common way the corpus
/// writes this shape - a wrapper that forwards a transformed argument - so it
/// is asserted separately from the bare statement rather than assumed to follow
/// from it.
#[test]
fn a_returned_nested_call_is_two_queries() -> io::Result<()> {
    let project = Project::new()?;
    let source = "def forward(n: int) -> int:\n    return fetch(g(n))\n";
    let (run, parsed) = queries_run(&project, source)?;
    run.assert_did_not_crash();

    assert_eq!(
        queries(&parsed, "forward"),
        Some(2),
        "`return fetch(g(n))` evaluates `g` and then `fetch`, and the `return` \
         has no value slot for either of them to hide in.\n{}",
        run.describe()
    );
    Ok(())
}

/// **An `if` condition is the hard half, and it is unsound today.**
///
/// `if fetch(g(n)):` reports `bound_kind: "upper"`, `value: 1`. Two calls are
/// issued on every path through the test, because the condition is evaluated
/// whichever branch is taken.
///
/// This one is called out in its own test because it is the position that does
/// **not** go through `Translator::hoisted`. A condition is translated into the
/// real builder, so a refusal found inside a refused call there has no
/// re-emitted statement to attach to and would be the orphan LAN-90 closed. The
/// answer is a design decision - attaching the argument to the refused node, as
/// `unsupported_expr_bounded` already does for `//`'s dividend, is the shape
/// already in the file - and it is a decision, not an oversight. What is not
/// available is leaving it: the number is confident and it is low.
#[test]
fn an_unsound_query_count_in_a_condition_is_still_unsound() -> io::Result<()> {
    let project = Project::new()?;
    let source = "def guarded(n: int) -> int:\n    x = 0\n    if fetch(g(n)):\n        x = 1\n    \
                  return x\n";
    let (run, parsed) = queries_run(&project, source)?;
    run.assert_did_not_crash();

    assert_ne!(
        queries(&parsed, "guarded"),
        Some(1),
        "the condition runs `g` and then `fetch` on every path, and one was \
         counted. The kind reported beside it is `upper`, which tells a \
         consumer this is a ceiling - it is not, it is below the truth, and an \
         upper bound that is low is worse than no bound at all.\n{}",
        run.describe()
    );
    assert_eq!(
        queries(&parsed, "guarded"),
        Some(2),
        "a condition is evaluated whichever branch is taken, so both its calls \
         are issued unconditionally and neither is maximised away.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. the neighbours that are already honest, and must stay that way
// ---------------------------------------------------------------------------

/// **A call in a subscript index reports no number, and must not start.**
///
/// `table[g(n)]` hides `g` exactly as `fetch(g(n))` hides it - but the
/// subscript is itself a region, that region survives into the query bound as
/// an unfilled variable, and the run reports `bound_kind: "partial"` with no
/// value. Weak, and never low: a gate is told it cannot threshold this
/// function, which is the truth.
///
/// It is a regression guard rather than a gap. The failure it catches is a fix
/// that descends into refused *containers generally* and, in tidying up, lets
/// the subscript region drop out of the resource bound - which would replace an
/// honest "I do not know" with a confident number over an index the analysis
/// never read.
#[test]
fn a_call_in_a_subscript_index_never_reports_a_confident_number() -> io::Result<()> {
    let project = Project::new()?;
    let source = "def indexed(n: int) -> int:\n    return table[g(n)]\n";
    let (run, parsed) = queries_run(&project, source)?;
    run.assert_did_not_crash();

    assert_eq!(
        queries(&parsed, "indexed"),
        None,
        "the subscript runs `__getitem__`, which is arbitrary user code that \
         may issue any number of calls. No number is the honest answer and it \
         is the answer today; a number here would be a claim about code the \
         analysis never read.\n{}",
        run.describe()
    );
    assert_eq!(
        queries_kind(&parsed, "indexed"),
        Some("partial"),
        "an unfilled region in the query bound is `partial`. `exact` and \
         `upper` are complete claims a consumer may compare against a \
         budget.\n{}",
        run.describe()
    );
    Ok(())
}

/// **A call in an attribute chain reports no number either.**
///
/// The same argument with a different construct: `g(n).field` runs `g` and then
/// a `property`, and the attribute region carries the second. Held separately
/// because the two constructs are refused in different arms and a change could
/// easily reach one and not the other.
#[test]
fn a_call_in_an_attribute_chain_never_reports_a_confident_number() -> io::Result<()> {
    let project = Project::new()?;
    let source = "def reached(n: int) -> int:\n    return g(n).field\n";
    let (run, parsed) = queries_run(&project, source)?;
    run.assert_did_not_crash();

    assert_eq!(
        queries(&parsed, "reached"),
        None,
        "an attribute access runs a `property`, so the number of calls this \
         function issues is not known. Absent is correct.\n{}",
        run.describe()
    );
    assert_eq!(
        queries_kind(&parsed, "reached"),
        Some("partial"),
        "and the kind must say the bound is incomplete rather than \
         exact.\n{}",
        run.describe()
    );
    Ok(())
}

/// **A call inside a display is already named, and stays named.**
///
/// The collection lane's half of the same rule, kept here so that a change to
/// `expression_children` that fixes the call arm by restructuring the function
/// cannot silently drop the display arm. `[g(n), 2]` evaluated for its effect
/// costs `g` and nothing else.
#[test]
fn a_call_inside_a_display_is_already_named() {
    assert_names_calls(
        "def built(n: int) -> int:\n    [g(n), 2]\n    return n\n",
        1,
    );
}

// ---------------------------------------------------------------------------
// 4. coverage must not move
// ---------------------------------------------------------------------------

/// **Naming the inner call does not make any of these lower.**
///
/// The headline coverage number counts transition systems, and a call still has
/// an unknown effect on the integer state whether it is written inside another
/// call or not - `landav_its::Update` is a total map with no havoc. So every
/// fixture in this file must keep refusing, and must keep naming `Call` when it
/// does. A fix that widened the *fragment* rather than the *traversal* would
/// show up here as a rise in `lowered`, which is the one number nobody is
/// allowed to buy with a translation change.
#[test]
fn naming_the_inner_call_does_not_widen_the_fragment() {
    for source in [
        "def outer(n: int) -> int:\n    fetch(g(n))\n    return n\n",
        "def outer(n: int) -> int:\n    fetch(g(h(n)))\n    return n\n",
        "def outer(n: int) -> int:\n    fetch(g(n), n)\n    return n\n",
        "def outer(n: int) -> int:\n    return fetch(g(n))\n",
        "def outer(n: int) -> int:\n    for i in range(10):\n        fetch(g(i))\n    return n\n",
    ] {
        let function = only_function(source);
        let refused = landav_its::lower(function.program());
        assert!(
            refused.is_err(),
            "a call still stops a program from lowering, however deeply it is \
             written:\n{source}"
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
