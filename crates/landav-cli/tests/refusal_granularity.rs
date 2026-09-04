//! `LAN-100`: **a refusal forgets what it can change, not the whole frame** -
//! the Python half.
//!
//! # What the frontend claims, per refused node
//!
//! Four tickets added ways to *reach* a loop and count it, and the bounds still
//! did not appear: 75 of the 91 typed-corpus loops that iterate a sized
//! parameter lost their trip count to a refusal earlier in the same function.
//! The engine forgot every readable value at every region, because a region's
//! construct has to answer for its worst member.
//!
//! The frontend saw the node and can do better, and now says so through
//! [`landav_its::Writes`]:
//!
//! * a refused **binding** of one name - `total = 0` when `total` is later
//!   condemned - rebinds that name and touches no object;
//! * a refused **expression** is scanned once for anything that could rebind a
//!   local of this frame: a call, a walrus, an `await`, a `yield`. Finding one
//!   is the widest answer, whatever the construct; finding none leaves at most
//!   the object question, which a name read or a literal answers too;
//! * `global`, `nonlocal`, `del`, a pattern match and an unknown call say
//!   nothing, and a node that says nothing inherits its construct's answer.
//!
//! # Why these tests read the libraries rather than driving the binary
//!
//! Same reason as `tuple_targets.rs` and `collection_length.rs`: the assertions
//! are about the *shape* of a bound - which variables it mentions, whether the
//! loop left a hole - and the process boundary offers only the rendered string.
//! `landav-engine/tests/refusal_granularity.rs` pins what the engine does with
//! a write set; this file pins which write set each Python shape produces.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_engine::{TripCount, cost};
use landav_python::LoweredFunction;

// ---------------------------------------------------------------------------
// harness - lifted from `tuple_targets.rs`
// ---------------------------------------------------------------------------

fn subject(source: &str) -> LoweredFunction {
    let functions = landav_python::lower_module(Path::new("granularity.py"), source)
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

fn mentioned(result: &TripCount) -> Vec<String> {
    result.bound().map_or_else(Vec::new, |bound| {
        bound
            .vars()
            .iter()
            .map(|var| var.symbol().as_str().to_owned())
            .collect()
    })
}

fn holes_on(result: &TripCount, construct: &str) -> bool {
    result
        .holes()
        .iter()
        .any(|hole| hole.construct() == construct)
}

/// Whether the loop kept its trip count. A `complex-assignment-target` hole is
/// *not* evidence against it here, unlike in `tuple_targets.rs`: in this file
/// that hole is an attribute store or a chained assignment above the loop,
/// which is exactly the region the loop must survive.
fn loop_is_counted(result: &TripCount) -> bool {
    !holes_on(result, "unbounded-iteration") && !holes_on(result, "for")
}

fn assert_only_supplied(function: &LoweredFunction, result: &TripCount, source: &str) {
    let params: Vec<String> = function
        .program()
        .params()
        .iter()
        .map(|param| param.symbol().as_str().to_owned())
        .collect();
    let partial = !result.is_complete();
    for name in mentioned(result) {
        assert!(
            params.contains(&name) || (partial && name.starts_with("#hole")),
            "the bound mentions `{name}`, which is not a parameter of the \
             function - the caller has nothing to supply for it. Parameters are \
             {params:?}, got {} for:\n{source}",
            describe(result),
        );
    }
}

/// The loop is counted, and the count is a function of `name`.
fn assert_counted_by(source: &str, name: &str) -> TripCount {
    let function = subject(source);
    let result = cost(function.program());
    let shown = describe(&result);
    assert!(
        loop_is_counted(&result),
        "the loop must be counted: nothing above it can have changed `{name}`. \
         Got {shown} for:\n{source}"
    );
    assert!(
        mentioned(&result).iter().any(|var| var == name),
        "the bound must be a function of `{name}` but mentions {:?}. Got {shown} \
         for:\n{source}",
        mentioned(&result)
    );
    assert_only_supplied(&function, &result, source);
    result
}

/// The loop is **not** counted, and the bound does not mention `name`.
fn assert_forgotten(source: &str, name: &str, because: &str) -> TripCount {
    let function = subject(source);
    let result = cost(function.program());
    let shown = describe(&result);
    assert!(
        !mentioned(&result).iter().any(|var| var == name),
        "the bound must not mention `{name}`: {because}. Got {shown} for:\n{source}"
    );
    assert!(
        !loop_is_counted(&result),
        "the loop must lose its endpoint: {because}. Got {shown} for:\n{source}"
    );
    assert_only_supplied(&function, &result, source);
    result
}

// ---------------------------------------------------------------------------
// 1 · a refused binding forgets the name it binds
// ---------------------------------------------------------------------------

/// **The reduction from the ticket.** `total = 0` is refused because a later
/// `total = total + a` condemns `total`; it rebinds `total` and nothing else,
/// so `len(pairs)` survives it and the loop is counted.
#[test]
fn the_reduction_from_the_ticket_counts_by_the_length() {
    let source = "\
def two(pairs: list) -> int:
    total = 0
    for a, b in pairs:
        total = total + a
    return total
";
    let result = assert_counted_by(source, "len(pairs)");
    assert!(
        result
            .holes()
            .iter()
            .all(|hole| hole.construct() == "non-integer-value"),
        "every hole is a refused read or binding of `total`, `a` or `b`; the loop \
         itself is not one. Got {}",
        describe(&result)
    );
}

/// **A chained assignment binds its names and nothing else.** `a = b = None`
/// is a `complex-assignment-target` - the fragment binds one name per
/// statement - and it still rebinds exactly `a` and `b`.
#[test]
fn a_chained_assignment_binds_its_names_and_nothing_else() {
    let source = "\
def chained(items: list) -> int:
    first = last = None
    count = 0
    for item in items:
        count += 1
    return count
";
    assert_counted_by(source, "len(items)");
}

/// **`except ... as name` binds one name.** Inside the handler, the loop over
/// the parameter is counted; before `LAN-100` the binding cleared the frame.
#[test]
fn an_except_clause_binds_its_name_and_nothing_else() {
    let source = "\
def guarded(items: list, n: int) -> int:
    count = 0
    try:
        count = n
    except ValueError as n:
        for item in items:
            count += 1
    return count
";
    assert_counted_by(source, "len(items)");
}

/// **Rebinding a collection parameter forgets its length.** `items = []` binds
/// `items`, and `len(items)` is the same object's length on entry, so both go.
/// The fence on the previous tests: naming the variable bound is only sound if
/// every name standing for it is named too.
#[test]
fn rebinding_a_collection_parameter_forgets_its_length() {
    let source = "\
def rebound(items: list) -> int:
    items = []
    count = 0
    for item in items:
        count += 1
    return count
";
    assert_forgotten(
        source,
        "len(items)",
        "`items` was rebound, so its length on entry is not the length the loop \
         runs over",
    );
}

// ---------------------------------------------------------------------------
// 2 · a refused expression that binds nothing forgets nothing
// ---------------------------------------------------------------------------

/// **A refused comparison over names and literals binds nothing.** `flag is
/// None` is refused - identity is not an operator of the fragment - and its
/// operands are a name and a literal, so nothing in this frame changes.
#[test]
fn a_refused_comparison_over_names_binds_nothing() {
    let source = "\
def compared(items: list, flag) -> int:
    count = 0
    if flag is None:
        count = 1
    for item in items:
        count += 1
    return count
";
    assert_counted_by(source, "len(items)");
}

/// **An attribute store before a counted loop keeps an integer parameter.**
/// `obj.flag = 1` is a `complex-assignment-target`; it rebinds no local and
/// runs a setter, which is foreign code in its own frame. `n` is an integer
/// the caller passed and no setter can change what it denotes here.
#[test]
fn an_attribute_store_keeps_an_integer_parameter() {
    let source = "\
def stored(obj, n: int) -> int:
    obj.flag = 1
    total = 0
    for i in range(n):
        total = total + i
    return total
";
    let result = assert_counted_by(source, "n");
    assert!(
        holes_on(&result, "complex-assignment-target"),
        "the store is still refused; only what it forgets is narrowed. Got {}",
        describe(&result)
    );
}

/// **An attribute store still forgets a collection's length.** The honest
/// limit of the same narrowing: a setter may call `items.append(...)`, so the
/// length read on entry is not the length the loop below runs over. Recorded
/// so that a future change to it is a decision rather than an accident.
#[test]
fn an_attribute_store_still_forgets_a_length() {
    let source = "\
def stored(self, items: list) -> int:
    self.items = items
    count = 0
    for item in items:
        count += 1
    return count
";
    assert_forgotten(
        source,
        "len(items)",
        "a setter is user code and may mutate `items`",
    );
}

// ---------------------------------------------------------------------------
// 3 · what still forgets the frame, pinned so the narrowing cannot widen
// ---------------------------------------------------------------------------

/// **An unknown call still forgets the frame.** `event_name.split(".")` is the
/// case from the ticket, and it is deliberately *not* fixed here: a callee the
/// signature pack does not account for may be a closure over this frame, and
/// the pack is the place to say otherwise, per callee.
#[test]
fn an_unknown_call_still_forgets_the_frame() {
    let source = "\
def split_first(meta: dict, event_name: str) -> int:
    event_path = event_name.split('.')
    n = 0
    for k, v in meta.items():
        n = n + 1
    return n
";
    let result = assert_forgotten(
        source,
        "len(meta)",
        "an unknown callee may rebind any local of this frame",
    );
    assert!(
        holes_on(&result, "call"),
        "the call is the region that forgets. Got {}",
        describe(&result)
    );
}

/// **`global`, `nonlocal` and `del` still forget the frame.** Each says
/// nothing about what it binds, and a node that says nothing inherits its
/// construct's answer, which for a `binding-form` is "anything".
#[test]
fn global_nonlocal_and_del_still_forget_the_frame() {
    let sources = [
        "\
def declared(items: list) -> int:
    global counter
    count = 0
    for item in items:
        count += 1
    return count
",
        "\
def outer():
    counter = 0
    def declared(items: list) -> int:
        nonlocal counter
        count = 0
        for item in items:
            count += 1
        return count
    return declared
",
        "\
def deleted(items: list, other) -> int:
    del other
    count = 0
    for item in items:
        count += 1
    return count
",
    ];
    for source in sources {
        let function = subject(source);
        let sees_items = function
            .program()
            .params()
            .iter()
            .any(|param| param.symbol().as_str() == "len(items)");
        if !sees_items {
            // The module's last function is the enclosing one and this
            // fragment does not descend into nested definitions; the
            // `nonlocal` case is then covered by the construct's answer alone,
            // which `an_unstated_refusal_still_forgets_the_frame` pins.
            continue;
        }
        let result = assert_forgotten(
            source,
            "len(items)",
            "a binding form may reach any name in this frame",
        );
        assert!(
            holes_on(&result, "binding-form"),
            "the binding form is the region that forgets. Got {}",
            describe(&result)
        );
    }
}

/// **A walrus hidden inside a refused expression forgets the frame.** The
/// f-string is a `collection` refusal whose interior the fragment never
/// translates; the scan finds the `:=` inside it and answers with the widest
/// claim. `items` really was rebound, and a loop counted by its length on entry
/// would be exceeded.
#[test]
fn a_walrus_hidden_in_a_refused_expression_forgets_the_frame() {
    let source = "\
def hidden(items: list) -> int:
    label = f'{(items := [1])}'
    count = 0
    for item in items:
        count += 1
    return count
";
    assert_forgotten(
        source,
        "len(items)",
        "the refused expression rebinds `items` through a walrus the fragment \
         did not translate",
    );
}

/// **A call hidden inside a refused expression forgets the frame.** Same scan,
/// other trigger: `[f() for x in xs]` is one `comprehension` node, and the call
/// inside it may be a closure over this frame.
#[test]
fn a_call_hidden_in_a_refused_expression_forgets_the_frame() {
    let source = "\
def hidden(items: list, xs) -> int:
    built = [len(x) for x in xs]
    count = 0
    for item in items:
        count += 1
    return count
";
    assert_forgotten(
        source,
        "len(items)",
        "a call inside the refused comprehension may rebind any local",
    );
}
