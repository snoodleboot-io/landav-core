//! `LAN-103`: **a pack row that cannot bound a callee's cost can still say it
//! rebinds nothing, and that keeps the loop below it counted.**
//!
//! # The gap this closes
//!
//! `LAN-94`'s rows resolve a call only when its cost is a constant; the node
//! then stops being a hole. Every other row - `list`, `sorted`, `split` - was
//! documentary. Measured after `LAN-100`, 13 of the 42 typed-corpus loops over
//! a sized parameter that were still uncounted had exactly one region above
//! them: a call to one of those callees, whose `Construct::Call` answer -
//! "may rebind anything" - cleared the length the loop had read.
//!
//! A refused call now carries `LAN-100`'s write set from its row. The call is
//! still a hole and still denotes omega; the loop below it keeps its trip
//! count. Nothing about the *cost* claim changes, and the tests here assert
//! the hole stays as often as they assert the count survives.
//!
//! # Two matching rules, both borrowed
//!
//! A bare name matches as `LAN-94` said: not bound in this module or function.
//! A method matches as `LAN-99` said for a length relation: the receiver is a
//! parameter annotated with the builtin the row was written for - plus never
//! rebound in the function, since a rebound receiver may hold anything.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_engine::{TripCount, cost};
use landav_python::LoweredFunction;

fn subject(source: &str) -> LoweredFunction {
    let functions = landav_python::lower_module(Path::new("rows.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    functions
        .into_iter()
        .next_back()
        .unwrap_or_else(|| panic!("expected at least one function in:\n{source}"))
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

/// The loop is counted by `name`, and the call above it is still a hole.
fn assert_counted_past_the_call(source: &str, name: &str) {
    let result = cost(subject(source).program());
    let shown = describe(&result);
    assert!(
        mentions(&result, name) && !holes_on(&result, "for"),
        "the loop must keep its trip count in `{name}`: the callee above it \
         cannot rebind a local of this frame. Got {shown} for:\n{source}"
    );
    assert!(
        holes_on(&result, "call"),
        "the call must still be a hole: its row bounds no cost, and the loop \
         keeping its count is not the call becoming free. Got {shown} \
         for:\n{source}"
    );
    assert!(
        !result.is_complete(),
        "a function with an unresolved call makes no complete claim. Got {shown}"
    );
}

/// The loop is **not** counted: the call above it forgot the frame.
fn assert_forgotten_at_the_call(source: &str, name: &str, because: &str) {
    let result = cost(subject(source).program());
    let shown = describe(&result);
    assert!(
        !mentions(&result, name) && holes_on(&result, "for"),
        "the loop must lose its trip count: {because}. Got {shown} for:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// 1 · the ticket's cases
// ---------------------------------------------------------------------------

/// **`event_name.split(".")` above `for k, v in meta.items()`.** The example
/// `LAN-100` named and left: a `str` method on a receiver annotated `str`
/// cannot rebind a local of this frame, and `len(meta)` survives it.
#[test]
fn a_str_method_on_an_annotated_receiver_keeps_the_length_below_it() {
    assert_counted_past_the_call(
        "\
def extract(meta: dict, event_name: str) -> int:
    event_path = event_name.split('.')
    n = 0
    for k, v in meta.items():
        n = n + 1
    return n
",
        "len(meta)",
    );
}

/// **`list(set(map(Path, xs.copy())))`, the corpus's other shape.** Four
/// nested calls, each with a row, each still a hole. An *integer* parameter
/// survives all four: no callee in its own frame can change what `n` denotes.
/// A *length* does not, and must not: `set(...)` hashes every `Path`, and
/// `Path.__hash__` is user code that may reach any object. That is the object
/// half of the answer, and it is the honest limit of this ticket - the loop in
/// the corpus function this was reduced from stays uncounted.
#[test]
fn nested_constructor_calls_keep_an_integer_and_not_a_length() {
    assert_counted_past_the_call(
        "\
def resolve(directories_list: list, n: int) -> int:
    directories = list(set(map(Path, directories_list.copy())))
    total = 0
    for i in range(n):
        total = total + 1
    return total
",
        "n",
    );
    assert_forgotten_at_the_call(
        "\
def resolve(directories_list: list, patterns: list) -> int:
    directories = list(set(map(Path, directories_list.copy())))
    total = 0
    for pattern in patterns:
        total = total + 1
    return total
",
        "len(patterns)",
        "`set` hashes user objects, which may mutate anything reachable",
    );
}

/// **`items.copy()` on an annotated receiver keeps another length.** The
/// builtin copy calls nothing on its elements, so the row says it mutates no
/// object, and `len(other)` survives it.
#[test]
fn a_builtin_copy_on_an_annotated_receiver_keeps_a_length_below_it() {
    assert_counted_past_the_call(
        "\
def copied(items: list, other: list) -> int:
    snapshot = items.copy()
    n = 0
    for x in other:
        n = n + 1
    return n
",
        "len(other)",
    );
}

/// **A bare constructor above an integer loop.** `defaultdict(list)` is the
/// third shape in the measured 13, and `n` is an integer no callee in its own
/// frame can change.
#[test]
fn a_bare_constructor_keeps_an_integer_parameter() {
    assert_counted_past_the_call(
        "\
def group(n: int) -> int:
    by_origin = defaultdict(list)
    total = 0
    for i in range(n):
        total = total + 1
    return total
",
        "n",
    );
}

// ---------------------------------------------------------------------------
// 2 · the cost claim is untouched
// ---------------------------------------------------------------------------

/// **`rows = sorted(records)` is still `Partial`.** The row says `sorted`
/// rebinds nothing and costs `n log n`; only the first half is read here.
#[test]
fn a_length_preserving_call_still_carries_its_cost_hole() {
    let result = cost(
        subject(
            "\
def ordered(records: list) -> int:
    rows = sorted(records)
    n = 0
    for r in rows:
        n = n + 1
    return n
",
        )
        .program(),
    );
    let shown = describe(&result);
    assert!(
        holes_on(&result, "call") && !result.is_complete(),
        "a sort is not free and the function is not complete. Got {shown}"
    );
}

// ---------------------------------------------------------------------------
// 3 · the fences
// ---------------------------------------------------------------------------

/// **An unannotated receiver matches no row.** `obj.split` may be anything.
#[test]
fn an_unannotated_receiver_matches_no_row() {
    assert_forgotten_at_the_call(
        "\
def extract(meta: dict, event_name) -> int:
    event_path = event_name.split('.')
    n = 0
    for k, v in meta.items():
        n = n + 1
    return n
",
        "len(meta)",
        "the receiver's class is unknown, so `split` is not known to be the builtin",
    );
}

/// **A rebound receiver matches no row.** `items` is annotated `list` and
/// then reassigned; whatever it holds now, `items.copy()` is not the builtin's.
#[test]
fn a_rebound_receiver_matches_no_row() {
    assert_forgotten_at_the_call(
        "\
def rebound(items: list, other: list, thing) -> int:
    items = thing
    copied = items.copy()
    n = 0
    for x in other:
        n = n + 1
    return n
",
        "len(other)",
        "`items` was rebound, so its `copy` is user code",
    );
}

/// **A shadowed bare name matches no row.** `LAN-94`'s rule, applied to the
/// narrowing as it is to the resolution.
#[test]
fn a_shadowed_constructor_matches_no_row() {
    assert_forgotten_at_the_call(
        "\
def list(x):
    return x


def shadowed(items: list, other: list) -> int:
    copied = list(items)
    n = 0
    for x in other:
        n = n + 1
    return n
",
        "len(other)",
        "`list` is bound in this module and is not the builtin",
    );
}

/// **A walrus hidden in an untranslated argument widens to the frame.** The
/// row speaks for `list`; it says nothing about `items[(n := 1)]`, which the
/// fragment never translated and which really does rebind `n`.
#[test]
fn a_walrus_in_an_untranslated_argument_still_forgets_the_frame() {
    assert_forgotten_at_the_call(
        "\
def hidden(items: list, n: int) -> int:
    first = list(items[(n := 1)])
    total = 0
    for i in range(n):
        total = total + 1
    return total
",
        "n",
        "the argument rebinds `n` through a walrus the row cannot see",
    );
}

/// **A row keeps the object question open.** `list(items)` runs
/// `items.__iter__`, so a length read on entry does not survive it - only an
/// integer does. The ticket hands this half to `LAN-101`.
#[test]
fn a_row_does_not_keep_the_length_of_its_own_argument() {
    assert_forgotten_at_the_call(
        "\
def iterated(items: list) -> int:
    copied = list(items)
    n = 0
    for x in items:
        n = n + 1
    return n
",
        "len(items)",
        "`mutates_arguments` is true for every row, and `len(items)` is volatile",
    );
}
