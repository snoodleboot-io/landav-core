//! `LAN-97`: **a confident `queries` count has descended into every refused
//! container.**
//!
//! # The defect
//!
//! `queries` is projected out of the cost bound by reading the coefficient of
//! the `call` holes. That works exactly as far as the arena does. A refused
//! form is one node whose interior the frontend need not translate - which is
//! what keeps a refused comprehension from producing a refusal per node inside
//! it - so a call written in such an interior has no node, no hole, and no
//! coefficient. The projection cannot notice it is missing, and reports the
//! calls it *can* see as a complete claim.
//!
//! ```python
//! def callee_pos(n: int) -> int:
//!     lookup(n).method()   # one `call` hole: reported `exact 1`, for two calls
//!     return 0
//! ```
//!
//! Measured before this change, 30.1% of confident stdlib results and 11.3% of
//! a typed corpus's were below an `ast`-derived truth and labelled `exact`.
//!
//! # Why the fix is not "keep descending"
//!
//! `LAN-96` closed the largest container - a refused call's arguments - and
//! found that descending *indiscriminately* is actively harmful: descending
//! into every argument moved 488 stdlib functions out of "blocked solely by a
//! call", 440 of them onto `non-integer-value` for a value nothing reads, which
//! renames the obstacle on a third of the corpus. So each remaining container -
//! a callee, a refused binary operator's operands, a subscript index - is its
//! own widening with its own blame cost, and the count is wrong while they
//! queue.
//!
//! So the *label* is fixed first and independently: the frontend records that
//! it left a call unaccounted for, and the projection fails closed on it. An
//! unhelpful `partial` costs a reader nothing; a confident 10 for a function
//! issuing 78 costs them an incident. Closing each container afterwards is an
//! ordinary precision improvement that moves functions from `partial` back to a
//! number.
//!
//! # What these tests hold in place
//!
//! Both directions, because only having both makes the change a fix rather than
//! a way of reporting less. Every shape `LAN-96` made exact still reports its
//! exact number, and every shape that hides a call reports none.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use serde_json::Value;

use common::{Project, Run};

// ---------------------------------------------------------------------------
// harness - lifted from `nested_calls.rs`, which is `LAN-96`'s, because these
// tests assert that its results are unchanged and asserting them in weaker
// words would let a regression through
// ---------------------------------------------------------------------------

/// Run `landav check <fixture> --json --resource queries` over `source`.
fn queries_run(project: &Project, source: &str) -> io::Result<(Run, Value)> {
    let target = project.write("fixture.py", source)?;
    let run = project.check(&target, &["--json", "--resource", "queries"])?;
    let parsed = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not valid JSON ({why}): {}", run.describe()));
    Ok((run, parsed))
}

fn resource_of<'a>(parsed: &'a Value, name: &str) -> &'a Value {
    parsed["functions"]
        .as_array()
        .and_then(|functions| functions.iter().find(|entry| entry["name"] == name))
        .map_or(&Value::Null, |entry| &entry["resource"])
}

fn queries(parsed: &Value, name: &str) -> Option<i64> {
    resource_of(parsed, name)["value"].as_i64()
}

fn queries_kind<'a>(parsed: &'a Value, name: &str) -> Option<&'a str> {
    resource_of(parsed, name)["bound_kind"].as_str()
}

/// `source` names `expected` queries, as a complete claim a gate may read.
fn assert_counts(source: &str, name: &str, expected: i64) -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = queries_run(&project, source)?;
    assert_eq!(
        queries(&parsed, name),
        Some(expected),
        "every call here is accounted for, so the count is a complete claim \
         and must survive `LAN-97`'s fail-closed rule. {}",
        run.describe()
    );
    assert_ne!(
        queries_kind(&parsed, name),
        Some("partial"),
        "nothing is concealed in this function. {}",
        run.describe()
    );
    Ok(())
}

/// `source` hides a call, so no number is offered for it.
fn assert_fails_closed(source: &str, name: &str, hidden: &str) -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = queries_run(&project, source)?;
    assert_eq!(
        queries_kind(&parsed, name),
        Some("partial"),
        "the call to `{hidden}` is inside a container the frontend did not \
         descend into, so it is in no ledger and no bound and the count cannot \
         see it. A complete label here is a promise the run cannot keep. {}",
        run.describe()
    );
    assert_eq!(
        queries(&parsed, name),
        None,
        "`value` is the field a budget gate thresholds on, and a low number \
         here is what waves a function past its budget. {}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 1 · the shapes that hide a call
// ---------------------------------------------------------------------------

/// **The reproduction from the ticket: a call in the callee.**
///
/// `lookup(n).method()` issues two calls and reports one `call` region, for
/// the `.method()`. `lookup` is inside the attribute the callee is read from,
/// which the frontend does not translate.
#[test]
fn a_call_in_the_callee_offers_no_number() -> io::Result<()> {
    assert_fails_closed(
        "def callee_pos(n: int) -> int:\n    lookup(n).method()\n    return 0\n",
        "callee_pos",
        "lookup",
    )
}

/// **A callee built by arithmetic.** `(a(p) / b(r)).read_bytes()` is three
/// calls and one region, and the two that produced the object are inside a
/// refused binary operator inside the callee - two uncrossed containers at
/// once.
#[test]
fn a_call_in_an_arithmetic_callee_offers_no_number() -> io::Result<()> {
    assert_fails_closed(
        "def joined(p: int, r: int) -> int:\n    (a(p) / b(r)).read_bytes()\n    return 0\n",
        "joined",
        "a",
    )
}

/// **A call among a refused binary operator's operands.** `"%s.%s" % (host(),
/// pid())` is a string format: the operator is refused, its operands are not
/// translated, and both calls vanish. The outer `_create` is counted alone.
#[test]
fn a_call_in_a_refused_operand_offers_no_number() -> io::Result<()> {
    assert_fails_closed(
        "def named(n: int) -> int:\n    _create(\"%s.%s\" % (host(), pid()))\n    return 0\n",
        "named",
        "host",
    )
}

/// **A call in a subscript index, and one behind an attribute.** Both already
/// reported `partial` before this change, because the enclosing region is not a
/// `call` hole and survives into the query bound unfilled. They are here so
/// that the new rule is shown to leave them exactly as they were, rather than
/// reaching the same answer by a different route nobody checked.
#[test]
fn an_index_and_an_attribute_are_unchanged() -> io::Result<()> {
    assert_fails_closed(
        "def indexed(n: int) -> int:\n    table[g(n)]\n    return 0\n",
        "indexed",
        "g",
    )?;
    assert_fails_closed(
        "def attributed(n: int) -> int:\n    g(n).field\n    return 0\n",
        "attributed",
        "g",
    )
}

// ---------------------------------------------------------------------------
// 2 · the shapes that must keep their number
// ---------------------------------------------------------------------------

/// **`LAN-96`'s shapes still report their exact number.**
///
/// The fence that stops this ticket being "won" by reporting `partial` for
/// everything. A refused call's arguments *are* translated where one of them
/// reaches a call, so nothing in these three is concealed and each keeps the
/// count `LAN-96` earned.
#[test]
fn the_shapes_lan_96_fixed_keep_their_counts() -> io::Result<()> {
    assert_counts(
        "def outer(n: int) -> int:\n    fetch(g(n))\n    return n\n",
        "outer",
        2,
    )?;
    assert_counts(
        "def deeper(n: int) -> int:\n    fetch(g(h(n)))\n    return n\n",
        "deeper",
        3,
    )?;
    assert_counts(
        "def looped(n: int) -> int:\n    for i in range(10):\n        fetch(g(i))\n    return n\n",
        "looped",
        20,
    )
}

/// **A function with no call at all still reports a confident zero.**
///
/// A known zero withheld is as wrong as an unknown one printed, and a rule that
/// failed closed on every function would produce exactly that.
#[test]
fn a_function_with_no_call_still_reports_zero() -> io::Result<()> {
    assert_counts(
        "def quiet(n: int) -> int:\n    total = 0\n    for i in range(n):\n        total = total + i\n    return total\n",
        "quiet",
        0,
    )
}

/// **A plain call keeps its count of one.** The simplest shape there is, and
/// the one a regression here would break first.
#[test]
fn a_plain_call_still_reports_one() -> io::Result<()> {
    assert_counts(
        "def once(n: int) -> int:\n    fetch(n)\n    return n\n",
        "once",
        1,
    )
}

// ---------------------------------------------------------------------------
// 3 · the flag itself, read from the library
// ---------------------------------------------------------------------------
//
// The tests above read the contract through the JSON, which is where a
// consumer meets it. These read `SourceProgram::conceals_a_call` directly,
// because a shape can be `partial` for more than one reason - a lambda is its
// own unanalysed region, and would report `partial` whatever this ticket did -
// and a test that could not tell those apart would pass on a rule that never
// fired.

/// Whether translating `source` leaves a call unaccounted for.
fn conceals(source: &str) -> bool {
    let functions = landav_python::lower_module(std::path::Path::new("q.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    functions
        .iter()
        .any(|function| function.program().conceals_a_call())
}

/// **A call in a lambda body does not conceal anything.**
///
/// The body runs when the lambda is *called*, which is not here, so it is not
/// a call this statement issues. A scan that walked it would fail closed on
/// every function that defines a callback. The lambda's **defaults** are
/// evaluated where it is written and are walked, which is the other half of
/// the same rule.
#[test]
fn a_call_in_a_lambda_body_does_not_conceal() {
    assert!(
        !conceals("def defines(n: int) -> int:\n    handler = lambda: fetch(n)\n    return n\n"),
        "a call in a lambda body is not issued by the statement that defines it"
    );
    assert!(
        conceals(
            "def defaulted(n: int) -> int:\n    handler = lambda x=fetch(n): x\n    return n\n"
        ),
        "a lambda's default argument is evaluated where the lambda is written, \
         so a call there is one this statement issues"
    );
}

/// **The flag fires on the concealing shapes and not on the accounted ones.**
///
/// The mechanism, stated once against the two populations the JSON tests
/// above check through the contract.
#[test]
fn the_flag_separates_the_two_populations() {
    for source in [
        "def a(n: int) -> int:\n    lookup(n).method()\n    return 0\n",
        "def b(n: int) -> int:\n    _create(\"%s\" % host())\n    return 0\n",
        "def c(n: int) -> int:\n    (x(n) / y(n)).read()\n    return 0\n",
    ] {
        assert!(conceals(source), "a call is hidden here:\n{source}");
    }
    for source in [
        "def d(n: int) -> int:\n    fetch(g(n))\n    return n\n",
        "def e(n: int) -> int:\n    fetch(g(h(n)))\n    return n\n",
        "def f(n: int) -> int:\n    fetch(n)\n    return n\n",
        "def g(n: int) -> int:\n    total = 0\n    for i in range(n):\n        total = total + i\n    return total\n",
    ] {
        assert!(
            !conceals(source),
            "every call here has a node in the arena:\n{source}"
        );
    }
}
