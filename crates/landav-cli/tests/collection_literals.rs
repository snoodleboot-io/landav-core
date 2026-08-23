//! `LAN-91` acceptance, the `collection` lane: **a literal is not an unbounded
//! thing.**
//!
//! # The distinction this whole file turns on
//!
//! `Construct::Collection` is refused for two unrelated reasons that today wear
//! one name:
//!
//! * **the value.** The fragment has no representation for a list, so a name
//!   bound to one is not an integer and `landav_its::Update` - a total map with
//!   no havoc - has nothing to write. That reason is permanent.
//! * **the cost.** A collection *display* is an expression, and in this engine's
//!   unit - "one step per statement executed, plus one per loop iteration",
//!   `landav_engine::cost` - an expression costs **nothing of its own**. `x = [1, 2, 3]`
//!   is one statement, exactly as `x = a + b + c` is one statement. That reason
//!   was never true.
//!
//! Holing the second is what erases bounds: `collection` is the sole blocker on
//! 23 stdlib functions and appears 2756 times overall. So this file pins the
//! cost half open and the value half shut, and most of its assertions exist to
//! stop the two from being widened together.
//!
//! # The trap, stated before any test
//!
//! `expression_children` (`landav-python/src/lowering.rs`) returns `Vec::new()`
//! for every collection node. **The elements of a literal are never
//! traversed.** So `[f(), 2]` produces exactly one refusal today - `collection`,
//! at the list's position - and the call inside it is invisible to the whole
//! pipeline.
//!
//! That is harmless while the literal itself refuses, and unsound the moment it
//! stops. Measured against the corpus: of the 23 stdlib functions blocked
//! solely by `collection`, **one** (`cgitb.reset`, a plain string constant) has
//! an element-free literal. The other 22 hide calls (10), attribute accesses
//! (15), subscripts (7) and a generator expression (1) inside the untraversed
//! display - `dataclasses._fields_in_init_order` is `return (tuple(f for f in
//! fields ...), tuple(...))`, two passes over `fields`. An implementation that
//! accepts displays without traversing their elements reports all 23 as
//! *lowered*, with `Theta(1)`, and that is the most confident wrong answer in
//! the corpus. `a_literal_element_that_cannot_be_analysed_is_still_named_and_placed`
//! is the test that stands in front of it.
//!
//! # What is soundly derivable about a literal's *length*
//!
//! Only a **display**, and only some of them:
//!
//! | form | length | why |
//! |---|---|---|
//! | `[a, b, c]`, `(a, b, c)`, `'abc'`, `b'abc'` | exactly 3 | positional, no collapsing |
//! | `{a, b, c}`, `{a: 1, b: 2}` | **at most** 3 | equal elements/keys collapse |
//! | `[*rest, 1]` | not known | `rest` contributes `len(rest)` |
//! | `[0] * n`, `a + b`, a comprehension | not known | not a display at all |
//!
//! An upper bound is enough for a `for`, since the fragment has no `break` - but
//! an `Exact` claim over a set display with duplicate elements is a false
//! tightness claim, not merely a loose one. See
//! `a_set_display_with_duplicates_is_not_exactly_its_element_count`.
//!
//! # Why these tests read the libraries rather than driving the binary
//!
//! The assertions are about the *shape* of a bound - which holes it carries,
//! which variables it mentions - and the process boundary offers only the
//! rendered string, which these tests are forbidden to pin. Same reasoning as
//! `collection_length.rs` and `calls_become_holes.rs`, which this file follows.

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

fn bindings(pairs: &[(&str, u64)]) -> Bindings {
    Bindings(
        pairs
            .iter()
            .map(|(name, value)| (Symbol::from(*name), *value))
            .collect(),
    )
}

/// Translates `source` and returns its single function.
fn only_function(source: &str) -> LoweredFunction {
    let mut functions = landav_python::lower_module(Path::new("literals.py"), source)
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
    let holes: Vec<String> = result
        .holes()
        .iter()
        .map(|hole| {
            format!(
                "{}={}@{}",
                hole.var().symbol(),
                hole.construct(),
                hole.origin()
            )
        })
        .collect();
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

/// The result's value with `pairs` bound and everything else zero.
fn value_at(result: &TripCount, pairs: &[(&str, u64)]) -> u64 {
    let bound = result
        .bound()
        .unwrap_or_else(|| panic!("no bound to evaluate: {}", describe(result)));
    match bound.eval(&bindings(pairs)) {
        Nat::Fin(value) => value,
        Nat::Omega => panic!("a bound that evaluates to omega: {}", describe(result)),
    }
}

/// The first hole blaming `construct`, or a failure naming what was blamed
/// instead.
fn hole_for(result: &TripCount, construct: &str, source: &str) -> Hole {
    result
        .holes()
        .iter()
        .find(|hole| hole.construct() == construct)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "no region was blamed on `{construct}`, got {} for:\n{source}",
                describe(result)
            )
        })
}

/// No region anywhere in the result blames `collection`.
///
/// The single assertion this ticket exists for. Spelled as a helper because the
/// failure message has to carry the whole result: a reader seeing only
/// "expected no collection hole" cannot tell a literal that is still refused
/// from one that moved to a different construct.
fn assert_no_collection_region(result: &TripCount, source: &str) {
    let blamed: Vec<&str> = result
        .holes()
        .iter()
        .filter(|hole| hole.construct() == "collection")
        .map(|hole| hole.origin().as_str())
        .collect();
    assert!(
        blamed.is_empty(),
        "a collection display is an expression, and an expression costs no \
         source step of its own - so charging one an unknown region overstates \
         nothing but erases the bound around it. Regions blamed on `collection` \
         at {blamed:?}, full result {} for:\n{source}",
        describe(result)
    );
}

/// Every name a complete bound mentions must be something the caller supplies.
///
/// Copied in spirit from `collection_length.rs`: a hole variable is exempt only
/// while the result is partial, because an unfilled hole denotes `omega` and a
/// partial makes no finite claim. A *complete* result carrying one is a
/// finite-looking claim that every caller reads as zero.
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
             function - the caller has nothing to supply for it, so \
             `Bound::eval` reads it as zero. Parameters are {params:?}, got {} \
             for:\n{source}",
            describe(result),
        );
    }
}

// ---------------------------------------------------------------------------
// the cost half: a literal is bounded work
// ---------------------------------------------------------------------------

/// **A literal on the right of an assignment leaves no region.**
///
/// `def f(n): x = [1, 2, 3]; return n` is two statements, so it costs two source
/// steps, exactly. Today it is `Partial(2 + #hole0 + #hole1)` - `#hole0` blaming
/// `collection` at the list, `#hole1` blaming `non-integer-value` at the binding
/// - and the reported bound is therefore no finite claim at all.
///
/// # Only the `collection` region is asserted away
///
/// The `non-integer-value` region on the binding is a *different* refusal with a
/// different lane and a different justification: `x` really does hold something
/// this fragment cannot represent, and every later read of it must keep
/// refusing. Asserting it away here would be asking this lane to widen the value
/// model, which is exactly what
/// `the_value_of_a_literal_is_still_not_an_integer` forbids.
///
/// FAILS today: `#hole0` blames `collection`.
#[test]
fn a_literal_in_an_assignment_leaves_no_collection_region() {
    let source = "\
def f(n: int) -> int:
    x = [1, 2, 3]
    return n
";
    let function = only_function(source);
    let result = cost(function.program());

    assert!(
        !matches!(result, TripCount::Unknown),
        "a literal must not erase the enclosing function's bound: {}",
        describe(&result)
    );
    assert_no_collection_region(&result, source);
    assert_only_supplied(&function, &result, source);
}

/// **A literal inside a loop does not cost the loop its shape.**
///
/// The reason the previous test matters at scale. The loop is counted by
/// `len(items)` and the body is one statement, so building a literal per
/// iteration must not turn a linear bound into an unfilled region multiplied by
/// the length.
///
/// FAILS today: the body carries a `collection` region.
#[test]
fn a_literal_in_a_loop_body_does_not_hole_the_loop() {
    let source = "\
def f(items: list) -> int:
    total = 0
    for x in items:
        row = [1, 2, 3]
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let names = mentioned(&result);

    assert!(
        names.iter().any(|var| var == "len(items)"),
        "the walk over `items` is still counted by its length, got {} \
         for:\n{source}",
        describe(&result)
    );
    assert_no_collection_region(&result, source);
    assert_only_supplied(&function, &result, source);
}

/// **A returned constant costs one step and nothing else.**
///
/// `return 'gif'` is `imghdr.test_gif`'s last line and `cgitb.reset`'s only one.
/// The value is dropped - this fragment models runtime, not results - so the
/// constant contributes no step, and the function costs exactly the `return`.
/// Hand arithmetic: one statement executed, so `Theta(1)`.
///
/// FAILS today: `Partial(1 + #hole0)`, `#hole0` blaming `collection`.
#[test]
fn a_returned_constant_costs_exactly_the_return() {
    for source in [
        "def f(n: int) -> int:\n    return 'gif'\n",
        "def f(n: int) -> int:\n    return (1, 2)\n",
        "def f(n: int) -> int:\n    return n, n\n",
        "def f(n: int) -> int:\n    return b'MM'\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());

        assert_no_collection_region(&result, source);
        assert!(
            result.is_complete(),
            "the returned value is discarded and the constant costs no step, so \
             the whole function is one `return`: {} for:\n{source}",
            describe(&result)
        );
        assert_eq!(
            value_at(&result, &[("n", 9)]),
            1,
            "one statement executed is one source step: {} for:\n{source}",
            describe(&result)
        );
        assert_only_supplied(&function, &result, source);
    }
}

/// **An element-free literal stops blocking the transition system.**
///
/// This is the one assertion in the file that moves `Coverage::lowered()`, and
/// it is the ticket's headline claim, so it is stated on its own where it can be
/// argued with rather than buried.
///
/// The argument that it is sound: `landav_its` refuses a call because a call has
/// an unknown *effect* on the integer state and `Update` is a total map with no
/// havoc. A constant in a discarded position has **no** effect on the integer
/// state - nothing is written, no name changes meaning - so there is nothing for
/// the transition system to be wrong about. That is the difference between this
/// construct and `LAN-87`'s, and it is why `collection` is a cheap one.
///
/// The `for` case is the same argument through a different door: the walk
/// becomes `for #walk in range(0, 3, 1)`, which is entirely integer.
///
/// # The honest size of this win
///
/// One of the 23 sole-`collection` stdlib functions - `cgitb.reset` - has an
/// element-free literal. The other 22 will re-refuse with `call`, `attribute`,
/// `subscript` or `comprehension` once their elements are traversed, which is
/// the correct outcome and not a regression. Anyone reporting "+23 lowered" from
/// this lane has skipped the traversal.
///
/// FAILS today: every one of these refuses with `collection`.
#[test]
fn an_element_free_literal_no_longer_stops_a_program_from_lowering() {
    for source in [
        "def f(n: int) -> int:\n    return 'gif'\n",
        "def f(n: int) -> int:\n    return (1, 2)\n",
        "def f() -> int:\n    total = 0\n    for x in [1, 2, 3]:\n        total = total + 1\n    return total\n",
    ] {
        let function = only_function(source);
        let lowered = landav_its::lower(function.program());
        let constructs = lowered
            .as_ref()
            .err()
            .and_then(landav_its::LoweringError::refusals)
            .map(landav_its::Refusals::constructs)
            .unwrap_or_default();
        assert!(
            lowered.is_ok(),
            "a literal whose elements are all constants writes nothing to the \
             integer state, so there is nothing for the transition system to \
             misrepresent - it refused with {constructs:?} for:\n{source}"
        );
    }
}

/// **Walking a literal runs exactly once per element.**
///
/// The trip count is not merely bounded, it is a number in the source. For
/// `for x in [1, 2, 3]` the arithmetic is the engine's declared unit - one step
/// per statement executed, one per loop iteration:
///
/// ```text
/// total = 0                 1
/// loop, 3 iterations        3 * (1 iteration + 1 statement) = 6
/// return total              1
///                          --
///                           8
/// ```
///
/// Measured against `for x in range(3)` on this engine today: `Theta(8)`. The
/// table below carries the same arithmetic for the other displays, `[]`
/// included, because a zero-length literal is where an off-by-one in the length
/// computation shows up as `Theta(4)` rather than `Theta(2)`.
///
/// `Exact` rather than `AtMost` is the claim for a list, tuple, `str` or `bytes`
/// display: the elements are positional, nothing collapses, and the fragment has
/// no `break`.
///
/// FAILS today: `Partial(3 + #hole0)`, `#hole0` blaming `unbounded-iteration`.
#[test]
fn walking_a_literal_runs_once_per_element() {
    for (iterable, elements, steps) in [
        ("[1, 2, 3]", 3_u64, 8_u64),
        ("(1, 2, 3)", 3, 8),
        ("[7]", 1, 4),
        ("[]", 0, 2),
        ("'abc'", 3, 8),
        ("[[1, 2], [3, 4]]", 2, 6),
    ] {
        let source = format!(
            "\
def f() -> int:
    total = 0
    for x in {iterable}:
        total = total + 1
    return total
"
        );
        let function = only_function(&source);
        let result = cost(function.program());

        assert!(
            result.holes().is_empty(),
            "a display of {elements} elements is walked {elements} times - a \
             literal is not unbounded iteration: {} for:\n{source}",
            describe(&result)
        );
        assert!(
            matches!(result, TripCount::Exact(_)),
            "the element count is positional and the fragment has no `break`, \
             so the trip count is exact rather than an upper bound: {} \
             for:\n{source}",
            describe(&result)
        );
        assert_eq!(
            value_at(&result, &[]),
            steps,
            "1 for `total = 0`, {elements} * (1 iteration + 1 statement), 1 for \
             `return` = {steps}: {} for:\n{source}",
            describe(&result)
        );
        assert_only_supplied(&function, &result, &source);
    }
}

/// **The literal and the `range` idioms derive the same cost.**
///
/// `for x in [1, 2, 3]` and `for i in range(3)` run the same number of times, so
/// a difference between them is a bug in one of them - and comparing the numbers
/// catches an off-by-one that comparing shapes would not. The same pairing
/// `collection_length.rs` uses for the two collection-parameter idioms.
///
/// FAILS today: the literal side is `Partial`, the `range` side is `Theta(8)`.
#[test]
fn the_literal_and_range_idioms_derive_the_same_cost() {
    let body = |iterable: &str| {
        format!(
            "\
def f() -> int:
    total = 0
    for x in {iterable}:
        total = total + 1
    return total
"
        )
    };
    let literal_source = body("[1, 2, 3]");
    let range_source = body("range(3)");

    let literal = cost(only_function(&literal_source).program());
    let ranged = cost(only_function(&range_source).program());

    assert_eq!(
        literal.bound().map(ToString::to_string),
        ranged.bound().map(ToString::to_string),
        "`for x in [1, 2, 3]` and `for i in range(3)` are the same loop written \
         two ways: {} versus {}",
        describe(&literal),
        describe(&ranged)
    );
    assert_eq!(
        literal.holes().len(),
        ranged.holes().len(),
        "one of the two idioms left a region the other did not: {} versus {}",
        describe(&literal),
        describe(&ranged)
    );
}

/// **A nested walk over literals is the product of their lengths.**
///
/// `[1, 2]` outside and `[1, 2, 3]` inside:
///
/// ```text
/// total = 0                              1
/// outer, 2 iterations, each costing
///     1 iteration + inner loop
///     inner = 3 * (1 + 1) = 6            2 * (1 + 6) = 14
/// return total                           1
///                                       --
///                                       16
/// ```
///
/// Measured against `for x in range(2): for y in range(3)` today: `Theta(16)`.
/// A bound that stayed at 8 here - the outer length forgotten, or the inner
/// charged once - would be exceeded by the program, which is the direction a
/// resource bound must never move.
///
/// FAILS today: both loops are `unbounded-iteration` regions.
#[test]
fn a_nested_literal_walk_multiplies_the_lengths() {
    let source = "\
def f() -> int:
    total = 0
    for x in [1, 2]:
        for y in [1, 2, 3]:
            total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());

    assert!(
        result.holes().is_empty(),
        "both walks have literal lengths, so neither is a region: {} \
         for:\n{source}",
        describe(&result)
    );
    assert_eq!(
        value_at(&result, &[]),
        16,
        "1 + 2 * (1 + 3 * (1 + 1)) + 1 = 16: {} for:\n{source}",
        describe(&result)
    );
    assert_only_supplied(&function, &result, source);
}

// ---------------------------------------------------------------------------
// the trap: elements are not traversed today
// ---------------------------------------------------------------------------

/// **An element a literal hides is still named, and still placed.**
///
/// `expression_children` returns nothing for every collection node, so `[g(n),
/// 2]` produces exactly one refusal today - `collection`, at the list - and the
/// call is invisible. Accepting the display without traversing its elements
/// therefore does not *lose precision*; it publishes a bound for a function that
/// calls something, with no hole and no footnote.
///
/// This is the assertion that separates a sound widening from an unsound one,
/// and it is stated over every shape in the same match arm - list, tuple, set,
/// dict and the f-string, which shares it and whose `{...}` parts are arbitrary
/// expressions.
///
/// FAILS today: the region is blamed on `collection`, and there is no `call`
/// hole to find.
#[test]
fn a_literal_element_that_cannot_be_analysed_is_still_named_and_placed() {
    for source in [
        "def f(n: int) -> int:\n    x = [g(n), 2]\n    return n\n",
        "def f(n: int) -> int:\n    x = (g(n), 2)\n    return n\n",
        "def f(n: int) -> int:\n    x = {g(n), 2}\n    return n\n",
        "def f(n: int) -> int:\n    x = {1: g(n)}\n    return n\n",
        "def f(n: int) -> int:\n    x = f'{g(n)}'\n    return n\n",
        "def f(n: int) -> int:\n    [g(n), 2]\n    return n\n",
        "def f(n: int) -> int:\n    return (g(n), 2)\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        let hole = hole_for(&result, "call", source);

        assert!(
            hole.origin().as_str().contains(':'),
            "a region must be placed as well as named - the user acts on the \
             line, not the construct - got {} for:\n{source}",
            hole.origin()
        );
        let bound = result.bound().expect("a partial result carries a bound");
        assert!(
            bound.vars().contains(&hole.var()),
            "the hole {} does not occur in {bound}, so the bound reads as a \
             complete cost with a footnote and `Bound::subst` has nothing to \
             fill for:\n{source}",
            hole.var().symbol()
        );
    }
}

/// **A call hidden in a literal inside a loop is paid once per iteration.**
///
/// Naming the region is half of it; placing it is the other half. A call lifted
/// out of the loop and charged once understates every iteration but the first,
/// which is the shape this tool exists to report.
///
/// Asserted as a *rate* rather than a literal total, because the binding of `x`
/// carries a second region of its own whose fate belongs to the
/// `non-integer-value` lane. Raising the call's cost by 1 must raise the
/// function's cost by at least `n`, and no charge outside the loop can do that.
///
/// FAILS today: there is no `call` hole - the call is inside an untraversed
/// literal.
#[test]
fn a_call_hidden_in_a_literal_is_paid_once_per_iteration() {
    let source = "\
def f(n: int) -> int:
    for i in range(n):
        x = [g(i), 2]
    return n
";
    let function = only_function(source);
    let result = cost(function.program());
    let hole = hole_for(&result, "call", source);
    let name = hole.var().symbol().as_str().to_owned();

    let cheap = value_at(&result, &[("n", 4), (&name, 0)]);
    let dearer = value_at(&result, &[("n", 4), (&name, 1)]);
    assert!(
        dearer >= cheap + 4,
        "the call runs on every one of the 4 iterations, so a call costing 1 \
         more must cost the function at least 4 more - {cheap} then {dearer} is \
         a call charged once, outside the loop: {} for:\n{source}",
        describe(&result)
    );
}

// ---------------------------------------------------------------------------
// the value half, which must not move
// ---------------------------------------------------------------------------

/// **A docstring is still not a value.**
///
/// PASSES today, and is here as a regression guard. `bare_expression` skips a
/// bare `Constant::Str`, which is why the corpus's documented functions are
/// analysable at all. This lane makes a string constant *cost* something
/// derivable, and the obvious way to do that is to stop treating
/// `Constant::Str` specially - at which point a docstring becomes a statement
/// and every documented function costs one step more than it does.
///
/// Hand arithmetic: the docstring is not a statement, so `def f(n): """d"""
/// return n` is one `return`, `Theta(1)`.
#[test]
fn a_docstring_is_still_not_charged_as_a_statement() {
    let documented = "\
def f(n: int) -> int:
    \"\"\"Return n.\"\"\"
    return n
";
    let bare = "\
def f(n: int) -> int:
    return n
";
    let with_doc = cost(only_function(documented).program());
    let without = cost(only_function(bare).program());

    assert_eq!(
        value_at(&with_doc, &[("n", 3)]),
        1,
        "a docstring is documentation, not a statement, so the function is one \
         `return`: {}",
        describe(&with_doc)
    );
    assert_eq!(
        value_at(&with_doc, &[("n", 3)]),
        value_at(&without, &[("n", 3)]),
        "documenting a function must not change what it costs: {} versus {}",
        describe(&with_doc),
        describe(&without)
    );
    assert!(
        with_doc.holes().is_empty(),
        "a docstring names no region: {}",
        describe(&with_doc)
    );
}

/// **The value of a literal is still not an integer.**
///
/// PASSES today, and is the fence this lane must not climb. The cost of
/// `x = [1, 2, 3]` is derivable; the *value* of `x` is not, because the fragment
/// has no list. So `range(x)` and `range(len(x))` must keep refusing, and no
/// bound may mention `x`.
///
/// `len(x)` deserves its own line. It is *arithmetically* knowable here - the
/// display has three elements - but knowing it needs an assignment environment,
/// and `landav_engine` deliberately keeps none: "a variable read is turned into
/// a bound only while the name still holds the value the caller supplied". A
/// length read out of a local would be the first thing in this engine that
/// depended on one, and the version of it that is wrong - reading the length
/// after `x.append(...)` - is the exact shape `LAN-89` already fenced off for
/// parameters.
#[test]
fn the_value_of_a_literal_is_still_not_an_integer() {
    for source in [
        "def f() -> int:\n    x = [1, 2, 3]\n    total = 0\n    for i in range(x):\n        total = total + 1\n    return total\n",
        "def f() -> int:\n    x = [1, 2, 3]\n    total = 0\n    for i in range(len(x)):\n        total = total + 1\n    return total\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        let names = mentioned(&result);

        assert!(
            !names.iter().any(|var| var == "x" || var == "len(x)"),
            "`x` holds a list, which is not a value the caller supplied and not \
             a number - a bound mentioning it is read as zero by `Bound::eval`: \
             {} for:\n{source}",
            describe(&result)
        );
        assert!(
            !result.is_complete(),
            "the loop's endpoint is a local this engine keeps no environment \
             for, so it cannot be counted: {} for:\n{source}",
            describe(&result)
        );
        assert_only_supplied(&function, &result, source);
    }
}

/// **A name bound to a literal still stops the program from lowering.**
///
/// PASSES today. The mirror of `an_element_free_literal_no_longer_stops_a_program_from_lowering`,
/// and the pair is the whole of this ticket's coverage claim: a literal in a
/// *discarded* position writes nothing to the integer state and may lower; a
/// literal *bound to a name* leaves that name holding something `Update` cannot
/// write, and may not.
#[test]
fn a_name_bound_to_a_literal_still_stops_the_program_from_lowering() {
    for source in [
        "def f(n: int) -> int:\n    x = [1, 2, 3]\n    return n\n",
        "def f(n: int) -> int:\n    d = {1: 2}\n    return n\n",
        "def f(n: int) -> int:\n    s = 'text'\n    return n\n",
    ] {
        let function = only_function(source);
        assert!(
            landav_its::lower(function.program()).is_err(),
            "`Update` is a total map with no havoc, so a name holding a list is \
             a name the transition system would have to claim was unchanged \
             for:\n{source}"
        );
    }
}

/// **The element a walk binds is not readable as an integer.**
///
/// PASSES today. `for x in [1, 2, 3]` binds `x` to an element. Even here, where
/// every element happens to be an integer literal, the counter the engine may
/// read must stay separate from the element the source bound - `LAN-89` uses a
/// synthetic `#walk` name for exactly this, and a literal walk must use the same
/// machinery rather than promoting the target.
///
/// The reason is `for x in [1, 'a']`, which is legal Python: a target promoted
/// on the strength of the elements it happened to have would need element type
/// inference to stay sound, and that is not this lane.
#[test]
fn the_element_of_a_literal_walk_is_not_readable_as_an_integer() {
    let source = "\
def f() -> int:
    total = 0
    for x in [1, 2, 3]:
        total = total + x
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let names = mentioned(&result);

    assert!(
        !names.iter().any(|var| var == "x"),
        "`x` is an element of the literal, not a value the caller supplied: {} \
         for:\n{source}",
        describe(&result)
    );
    assert_only_supplied(&function, &result, source);
}

/// **Membership in a literal is still refused.**
///
/// PASSES today. `if h in ('a', 'b')` is `wsgiref.util.guess_scheme` and both
/// `imghdr` tests. It is refused as `collection` and it is not a literal
/// problem: `in` needs an equality model over values this fragment does not
/// have, and the comparison is a *condition*, which decides which branch is
/// taken. Widening the literal must not sweep this up with it.
#[test]
fn membership_in_a_literal_is_still_refused() {
    for source in [
        "def f(n: int) -> int:\n    if n in (1, 2):\n        return 1\n    return 0\n",
        "def f(n: int) -> int:\n    if n not in [1, 2]:\n        return 1\n    return 0\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());

        assert!(
            !result.holes().is_empty(),
            "`in` is membership, not construction - it needs an equality model \
             this fragment has none of, and it decides a branch: {} \
             for:\n{source}",
            describe(&result)
        );
        assert!(
            landav_its::lower(function.program()).is_err(),
            "a membership test still stops the program from lowering \
             for:\n{source}"
        );
    }
}

// ---------------------------------------------------------------------------
// lengths that are not static, and must not be treated as if they were
// ---------------------------------------------------------------------------

/// **A display with a starred element has no static length.**
///
/// PASSES today, by refusing the whole loop as `unbounded-iteration`, and it is
/// the sharpest over-widening guard in the file: `[*rest, 1]` is an ordinary
/// `Expr::List` with two elements, so the natural implementation - "the trip
/// count is `elts.len()`" - reports 2 while the truth is `len(rest) + 1`. A
/// caller passing a thousand-element `rest` is handed a bound of 2.
///
/// Either answer is acceptable: refuse the walk, or count it as
/// `len(rest) + 1`. What is not acceptable is a constant.
#[test]
fn a_starred_element_has_no_static_length() {
    let source = "\
def f(rest: list) -> int:
    total = 0
    for x in [*rest, 1]:
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());

    if result.is_complete() {
        let names = mentioned(&result);
        assert!(
            names.iter().any(|var| var == "len(rest)"),
            "`[*rest, 1]` walks `len(rest) + 1` elements, so a complete bound \
             that does not mention `len(rest)` is a constant claim about an \
             unbounded loop: {} for:\n{source}",
            describe(&result)
        );
        assert!(
            value_at(&result, &[("len(rest)", 1000)]) >= 1000,
            "with a thousand elements spread in, the loop runs at least a \
             thousand times: {} for:\n{source}",
            describe(&result)
        );
    }
    assert_only_supplied(&function, &result, source);
}

/// **A repeated literal has no static length either.**
///
/// PASSES today. `[0] * n` is a `BinOp`, not a display, so it falls through to
/// `unbounded-iteration` - but its *left operand* is a display, and an
/// implementation that starts folding displays into lengths has to stop at the
/// operator rather than reading through it. The truth is `n` iterations; a
/// bound of 1 is what reading the display alone would give.
#[test]
fn a_repeated_literal_has_no_static_length() {
    let source = "\
def f(n: int) -> int:
    total = 0
    for x in [0] * n:
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());

    if result.is_complete() {
        let names = mentioned(&result);
        assert!(
            names.iter().any(|var| var == "n"),
            "`[0] * n` walks `n` elements, not the one the display has: {} \
             for:\n{source}",
            describe(&result)
        );
        assert!(
            value_at(&result, &[("n", 1000)]) >= 1000,
            "the loop runs `n` times: {} for:\n{source}",
            describe(&result)
        );
    }
    assert_only_supplied(&function, &result, source);
}

/// **A set display with duplicates is not exactly its element count.**
///
/// PASSES today, by refusing. `{1, 1, 2}` has three elements written and two
/// members, and `for x in {1, 1, 2}` runs twice:
///
/// ```text
/// total = 0                 1
/// loop, 2 iterations        2 * (1 + 1) = 4
/// return total              1
///                          --
///                           6
/// ```
///
/// Measured against `for x in range(2)` today: `Theta(6)`.
///
/// Counting three is *sound* - an upper bound may overstate - but claiming it
/// `Exact` is a false tightness claim, and `Theta` against `O` is a distinction
/// this codebase tracks deliberately. So a set or dict display is at most its
/// written element count, never exactly it, unless the implementation actually
/// deduplicates.
#[test]
fn a_set_display_with_duplicates_is_not_exactly_its_element_count() {
    for iterable in ["{1, 1, 2}", "{1: 'a', 1: 'b', 2: 'c'}"] {
        let source = format!(
            "\
def f() -> int:
    total = 0
    for x in {iterable}:
        total = total + 1
    return total
"
        );
        let function = only_function(&source);
        let result = cost(function.program());

        match &result {
            TripCount::Exact(_) => assert_eq!(
                value_at(&result, &[]),
                6,
                "`{iterable}` has two members, so an exact claim must be the \
                 cost of two iterations - counting the three written elements \
                 is a sound upper bound but a false `Theta`: {} for:\n{source}",
                describe(&result)
            ),
            TripCount::AtMost(_) => assert!(
                value_at(&result, &[]) >= 6,
                "an upper bound may overstate but never understate two \
                 iterations: {} for:\n{source}",
                describe(&result)
            ),
            TripCount::Partial { .. } | TripCount::Unknown => {}
        }
        assert_only_supplied(&function, &result, &source);
    }
}

// ---------------------------------------------------------------------------
// literals and LAN-89's lengths must not be confused
// ---------------------------------------------------------------------------

/// **A literal walk does not borrow a collection parameter's length.**
///
/// PASSES today. `LAN-89` gave a collection *parameter* a length variable, and
/// this function has one - but the loop walks a three-element display, not the
/// parameter. A bound over `len(items)` here would say 0 for a caller passing an
/// empty list, and the loop would still run three times.
///
/// The cost must be the same whatever `items` is.
#[test]
fn a_literal_walk_does_not_borrow_a_parameters_length() {
    let source = "\
def f(items: list) -> int:
    total = 0
    for x in [1, 2, 3]:
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let names = mentioned(&result);

    assert!(
        !names.iter().any(|var| var == "len(items)"),
        "the loop walks a literal; `len(items)` is the length of a collection \
         it never touches: {} for:\n{source}",
        describe(&result)
    );
    assert_eq!(
        value_at(&result, &[("len(items)", 0)]),
        value_at(&result, &[("len(items)", 1000)]),
        "what the caller passes cannot change how many times a literal is \
         walked: {} for:\n{source}",
        describe(&result)
    );
    assert_only_supplied(&function, &result, source);
}

/// **Rebinding a collection parameter to a literal forgets the caller's
/// length.**
///
/// PASSES today, by holing the loop, and it is `LAN-88`'s discipline in this
/// lane's vocabulary. After `items = [1, 2, 3]` the loop walks three elements
/// whatever the caller passed, so a bound still written over `len(items)` is
/// exceeded by the program for every caller with fewer than three - the one
/// direction a resource bound must never move.
///
/// Two answers are honest: hole the loop, or count it as three. Continuing to
/// claim `len(items)` is not one of them.
#[test]
fn rebinding_a_collection_parameter_to_a_literal_forgets_the_callers_length() {
    let source = "\
def f(items: list) -> int:
    total = 0
    items = [1, 2, 3]
    for x in items:
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());

    if result.is_complete() {
        let names = mentioned(&result);
        assert!(
            !names.iter().any(|var| var == "len(items)"),
            "after `items = [1, 2, 3]` the loop runs three times however long \
             the caller's list was - a complete bound over `len(items)` is \
             exceeded by every caller passing fewer than three: {} \
             for:\n{source}",
            describe(&result)
        );
        assert!(
            value_at(&result, &[("len(items)", 0)]) >= 6,
            "three iterations cost 1 + 3 * 2 + 1 = 8, and a caller passing an \
             empty list still pays them: {} for:\n{source}",
            describe(&result)
        );
    }
    assert_only_supplied(&function, &result, source);
}

/// **A collection parameter's walk is still counted by its length.**
///
/// PASSES today, and guards the other direction: `LAN-89`'s machinery must
/// survive this lane. A literal-length path that shadowed the parameter path -
/// or a `walked_collection` rewritten to look for displays first - would turn a
/// scale answer back into a constant, which is the regression `LAN-89` exists to
/// prevent.
#[test]
fn a_collection_parameter_is_still_counted_by_its_length() {
    let source = "\
def f(items: list) -> int:
    total = 0
    for x in items:
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let names = mentioned(&result);

    assert!(
        names.iter().any(|var| var == "len(items)"),
        "the bound must still be a function of the parameter's length: {} \
         for:\n{source}",
        describe(&result)
    );
    assert!(
        result.holes().is_empty(),
        "the walk over a collection parameter must still be counted, not \
         holed: {} for:\n{source}",
        describe(&result)
    );
}
