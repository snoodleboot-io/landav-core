//! `LAN-99` acceptance, second half: **a tuple target on a `for` statement.**
//!
//! # The construct, and why it is the dominant blocker
//!
//! `for k, v in mapping.items()` is refused as
//! [`landav_its::Construct::ComplexAssignmentTarget`] in `Translator::for_loop`,
//! by a `let Expr::Name(target) = loop_stmt.target else { ... }` that runs
//! **before the iterable is consulted at all**. The refusal is about the target;
//! the trip count is a property of the iterable; and today the first of those
//! stops the second ever being asked.
//!
//! Measured over the standard library (see the table below), 96 of the 150 loops
//! that iterate a length-preserving call are behind this refusal. The sibling
//! file `length_relations.rs` pins the other half of the ticket and records the
//! same fact from its side, in
//! `a_tuple_target_is_refused_by_something_this_ticket_does_not_fix` - a fence
//! written to fail the moment this lane lands, which is the moment to delete it
//! and widen `a_walk_over_items_counts_by_the_mappings_length`.
//!
//! # The treatment already exists one construct over
//!
//! `LAN-89`'s `Translator::walk_collection` counts `for x in items` over a
//! collection parameter by the parameter's length variable, and binds the target
//! through a **synthetic** `#walk` counter rather than through `x` itself. The
//! reason is written there: the target holds an **element**, an element of a
//! `list` may be a string, and [`landav_its::VarName`] promises a mathematical
//! integer - so the element's name must stay non-integer and a read of it must
//! refuse. `integer_names` dooms it, which is what makes the read refuse.
//!
//! A tuple target is that same argument with more names. `for a, b in pairs`
//! yields one value per element of `pairs` exactly as `for x in pairs` does; it
//! then takes that element apart. `a` and `b` hold pieces of an element, which is
//! no more an integer than the element was. So: **count it exactly as the plain
//! walk is counted, bind nothing, and let every read refuse.** `written_names`
//! already recurses through `Tuple`, `List` and `Starred`, so the machinery for
//! condemning all of the names is in the tree already.
//!
//! # What this lane is worth on its own - measured, and smaller than it looks
//!
//! Corpus: every `.py` under `/usr/lib/python3.12` (570 files), module-level
//! `def`/`async def` only, which is what `lower_module` iterates - 3123
//! functions against the 3072 the CLI reports analysed. 868 `for` loops.
//!
//! | target shape | loops |
//! |---|---|
//! | single `Name` | 663 (76.4%) |
//! | flat tuple | 201 |
//! | nested tuple | 4 |
//! | starred | 0 |
//!
//! Of the 205 tuple-target loops, split by what their iterable would need:
//!
//! | needs | loops |
//! |---|---|
//! | **this lane alone** (collection parameter or literal display) | **4** |
//! | + the length relation, argument length already known | 2 |
//! | + assignment-level size flow (`LAN-11`), or an annotation | 147 |
//! | unreachable by all three | 52 |
//!
//! All four of the reachable-today loops are **literal displays** -
//! `dataclasses.py:1101`, `plistlib.py:428` and two in `test/libregrtest`. Not
//! one is a collection parameter, because the whole standard library contains
//! exactly **one** loop over a collection-annotated parameter
//! (`test/libregrtest/run_workers.py:440`) and its target is a single name.
//!
//! So the honest verdict is: this lane ships **4 loops** on its own, and the
//! premise it was commissioned under holds anyway. Tuple unpacking *is* the
//! dominant blocker of the length-preserving population - 96 of 150 - and
//! removing it is necessary. It is not sufficient, and neither is the relation:
//! 82 of the 205 tuple-target loops bottom out in a **local**, which is
//! `LAN-98`'s step 2 and lives in `LAN-11`. Predicting a coverage jump from
//! either half of `LAN-99` alone would be the fourth prediction this month that
//! measurement contradicts.
//!
//! One figure in the brief this file could not reproduce, recorded rather than
//! quietly adjusted: "4 reachable by a length relation alone". Under the
//! definition used here - a single-`Name` target over a length-preserving call
//! whose argument is a collection parameter or a literal display - the count is
//! **0**. The 54 single-`Name` length-preserving loops take their argument from
//! a local (20), an attribute (14), an unannotated parameter (9) or another call;
//! none from something whose length is a value the caller supplies.
//!
//! # Why these tests read the libraries rather than driving the binary
//!
//! Same reason as `collection_length.rs`, `signature_pack.rs` and
//! `length_relations.rs`: the assertions are about the *shape* of a bound -
//! which variables it mentions, whether the loop left a hole, whether the
//! program lowers - and the process boundary offers only the rendered string,
//! which these tests are forbidden to pin.
//!
//! [`landav_its::Construct::ComplexAssignmentTarget`]: landav_its

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, path::Path};

use landav_bound::{Nat, Symbol, Valuation, VarId};
use landav_engine::{TripCount, cost};
use landav_python::LoweredFunction;

// ---------------------------------------------------------------------------
// harness - lifted from `collection_length.rs` and `length_relations.rs`,
// because several tests below assert those files' outcomes are unchanged and
// asserting them in weaker words would let a regression through
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

/// Translates `source` and returns its **last** function.
///
/// Last rather than only, so a source may define a helper above the function
/// under test without the helper becoming the subject.
fn subject(source: &str) -> LoweredFunction {
    let functions = landav_python::lower_module(Path::new("targets.py"), source)
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

/// Whether any hole blames `construct`.
fn holes_on(result: &TripCount, construct: &str) -> bool {
    result
        .holes()
        .iter()
        .any(|hole| hole.construct() == construct)
}

/// Whether the loop itself was counted rather than given up on.
///
/// Three tags, because the frontend has three ways of not counting a `for`, and
/// this lane is the one that adds the third: `unbounded-iteration` when it
/// cannot read the iterable, `for` when it built the loop but could not read the
/// endpoint, and `complex-assignment-target` when it refused the statement for
/// its target before looking at the iterable at all. A helper that checked only
/// the first two would report every test in this file as already counted.
fn loop_is_counted(result: &TripCount) -> bool {
    !holes_on(result, "unbounded-iteration")
        && !holes_on(result, "for")
        && !holes_on(result, "complex-assignment-target")
}

/// Every `(construct, callee)` pair the lowering refused, callee where known.
///
/// The lowering's ledger is where a callee's *name* survives; a hole carries
/// only the construct tag, so "which callee" can be asked here and nowhere else.
fn refusals(function: &LoweredFunction) -> Vec<(String, String)> {
    match landav_its::lower(function.program()) {
        Ok(_) => Vec::new(),
        Err(error) => error
            .refusals()
            .map(|ledger| {
                ledger
                    .as_slice()
                    .iter()
                    .map(|record| {
                        (
                            record.construct().tag().to_owned(),
                            record
                                .detail()
                                .map_or_else(String::new, |name| name.as_str().to_owned()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Whether the lowering refused `construct`, with `detail` where it carries one.
fn refuses(function: &LoweredFunction, construct: &str, detail: &str) -> bool {
    refusals(function)
        .iter()
        .any(|(tag, name)| tag == construct && name == detail)
}

/// **The** assertion this lane exists for: the loop is counted, and counted by
/// the length of the thing it walks - whatever shape its target has.
///
/// Weaker than `collection_length.rs::assert_scales_with` in one respect: that
/// helper also requires `holes().is_empty()`, and here some subjects walk an
/// iterable whose construction is genuinely unknown work. Completeness is
/// asserted per test, where it is a claim, rather than smuggled in here.
fn assert_counted_by(result: &TripCount, name: &str, source: &str) {
    let shown = describe(result);
    let names = mentioned(result);
    assert!(
        loop_is_counted(result),
        "the loop must be counted: the number of times it runs is a property of \
         the **iterable**, and taking the value it yields apart into several \
         names does not change how many values there are. Refusing the statement \
         for its target gives up on the loop before the question is even asked. \
         Got {shown} for:\n{source}"
    );
    assert!(
        names.iter().any(|var| var == name),
        "the bound must be a function of `{name}` - that is how many values this \
         iterable yields and therefore how many times the loop runs - but it \
         mentions {names:?}. Got {shown} for:\n{source}"
    );
}

/// No name a tuple target binds may appear in the bound.
///
/// The discipline `LAN-89` established for the single-name case, restated for
/// several names. Each of these holds a piece of an element, an element of a
/// `list` may be a string, and a bound mentioning one would be arithmetic on a
/// value that is not a number.
fn assert_elements_are_not_integers(
    function: &LoweredFunction,
    result: &TripCount,
    elements: &[&str],
    source: &str,
) {
    let shown = describe(result);
    let names = mentioned(result);
    let params: Vec<String> = function
        .program()
        .params()
        .iter()
        .map(|param| param.symbol().as_str().to_owned())
        .collect();
    for element in elements {
        assert!(
            !names.iter().any(|var| var == element),
            "`{element}` holds a piece of an element of the collection, not an \
             integer the caller supplied - a bound mentioning it would be \
             arithmetic on a value that may be a string. This is exactly why \
             `walk_collection` counts through a synthetic `#walk` counter \
             instead of through the target. Got {shown} for:\n{source}"
        );
        let length = format!("len({element})");
        assert!(
            !params.contains(&length) && !names.contains(&length),
            "`{length}` is not a value the caller supplies - `{element}` was \
             bound by the loop, not passed in - so it may not become a bound \
             variable. `Bound::eval` reads a name the caller supplies nothing \
             for as **zero**. Parameters are {params:?}, got {shown} \
             for:\n{source}"
        );
    }
}

/// Every name a bound mentions must be something the caller can supply.
///
/// A hole variable is exempt only while the result is **partial**: an unfilled
/// hole denotes `omega`, so a partial makes no finite claim. A *complete* result
/// carrying one would be a finite-looking claim with an infinite term in it,
/// which every caller supplying the parameters and nothing else reads as zero.
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

/// The bound's value with every length this file uses bound to `n`.
///
/// Zero is the right default *only* for a complete result; a hole variable read
/// as zero is exactly the under-report these tests police, so every caller
/// checks completeness first.
fn at(result: &TripCount, n: u64) -> u64 {
    match result
        .bound()
        .expect("a result under test carries a bound")
        .eval(&bindings(&[
            ("n", n),
            ("len(pairs)", n),
            ("len(other)", n),
            ("len(records)", n),
            ("len(mapping)", n),
        ])) {
        Nat::Fin(value) => value,
        Nat::Omega => panic!("a bound under test must evaluate finitely at n = {n}"),
    }
}

// ---------------------------------------------------------------------------
// sources
// ---------------------------------------------------------------------------

/// `for <target> in <iterable>:` inside a function taking `pairs: list`.
///
/// Every subject in sections 1 and 2 is this shape, so the arithmetic below is
/// one hand computation rather than one per test. Measured for the single-name
/// baseline `for x in pairs`, which is what a tuple target must match:
///
/// ```text
/// Theta(2 + 2 * len(pairs))
/// ```
///
/// Two outside the loop - `total = 0` and `return total` - and two per
/// iteration, the loop's own step plus the one body statement. At
/// `len(pairs) = 10` that is 22 and at 20 it is 42.
fn walking(target: &str, iterable: &str) -> String {
    format!(
        "\
def g(pairs: list) -> int:
    total = 0
    for {target} in {iterable}:
        total = total + 1
    return total
"
    )
}

// ---------------------------------------------------------------------------
// 1 · the plain case - the one that must work first
// ---------------------------------------------------------------------------

/// **`for a, b in pairs` counts as `len(pairs)`.**
///
/// The floor everything else in this file stands on, and the one shape with no
/// second dependency: `pairs` is a collection parameter, so its length is
/// already a bound variable and already spelled `len(pairs)`. Nothing about the
/// iterable is new. The only thing standing between this loop and
/// `walk_collection` is the target check above it.
///
/// Today: `Partial(3 + #hole0)`, `#hole0` blamed on
/// `complex-assignment-target`, and the function has `len(pairs)` as a parameter
/// and never mentions it.
///
/// Completeness is asserted because it is a claim and not a formality: nothing
/// in this function is unknown once the loop is counted. `pairs` is supplied,
/// the body is arithmetic on a proven integer, and taking an element apart is
/// not work this analysis needs a hole for.
#[test]
fn a_tuple_target_over_a_collection_parameter_counts_by_its_length() {
    let source = walking("a, b", "pairs");
    let function = subject(&source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert_counted_by(&result, "len(pairs)", &source);
    assert!(
        result.holes().is_empty(),
        "nothing in this function is unknown - a collection the caller supplied, \
         a body that adds one to a proven integer - so it must be a complete \
         claim with no holes at all. A surviving hole means the target was \
         charged for rather than simply not bound. Got {shown} for:\n{source}"
    );
    assert_elements_are_not_integers(&function, &result, &["a", "b"], &source);
    assert_only_supplied(&function, &result, &source);
}

/// **A nested tuple target counts by the same length.**
///
/// `for a, (b, c) in pairs` - four such loops in the standard library. Counted
/// rather than refused, and the reason is the same sentence as for the flat
/// case: **the trip count is a property of the iterable**, and nesting adds
/// names, not iterations. `pairs` yields `len(pairs)` values whether each one is
/// taken apart into two names, three, or none.
///
/// This is the honest answer rather than the convenient one, and it is worth
/// saying which alternative was rejected. Refusing the nested form while
/// counting the flat one would be a rule about the *shape of the target*, and no
/// property of the target bears on how many values the iterable has. It would
/// also be inconsistent with the code already in the tree: `written_names`
/// recurses through `Tuple`, `List` and `Starred` precisely because a target is
/// a tree, and `collect_non_integer_bindings` already condemns every name it
/// finds there - so `b` and `c` are already non-integers today, with no further
/// work.
///
/// Today: `Partial(3 + #hole0)` on `complex-assignment-target`, identical to the
/// flat case - measured, the frontend does not distinguish them at all.
#[test]
fn a_nested_tuple_target_counts_by_the_same_length() {
    let source = walking("a, (b, c)", "pairs");
    let function = subject(&source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert_counted_by(&result, "len(pairs)", &source);
    assert!(
        result.holes().is_empty(),
        "a nested target destructures each element further; it does not make the \
         collection longer, shorter or unknown. Whatever is true of `for a, b in \
         pairs` is true here, and that one is complete. Got {shown} for:\n{source}"
    );
    assert_elements_are_not_integers(&function, &result, &["a", "b", "c"], &source);
    assert_only_supplied(&function, &result, &source);
}

/// **A starred target counts, and `rest` acquires no length.**
///
/// `for a, *rest in pairs` is counted for the same reason as the nested form:
/// `pairs` has `len(pairs)` elements however each is taken apart.
///
/// The sharp half is the second assertion. `rest` is a **list**, built fresh on
/// every iteration, and its length is `len(element) - 1` for an element size
/// nothing here knows. It is exactly the kind of name a length-aware change
/// reaches for by accident, and it must not become one: `len(rest)` is not a
/// value the caller supplies, so `Bound::eval` reads it as **zero** and a loop
/// over it would report as costing nothing, completely, with no hole to warn
/// anybody. The matched precedent is
/// `collection_length.rs::a_length_of_something_that_is_not_a_parameter_is_not_a_bound_variable`.
///
/// # Corpus footprint: zero, and it is still worth pinning
///
/// Measured, the standard library contains **no** starred `for` target at all -
/// 201 flat and 4 nested out of 205. So this test buys no coverage and is not a
/// driver of the ticket's number. It is here because a starred element is the
/// one target shape that binds something with a length, and the change that
/// teaches the frontend to walk a target tree is the change that could give it
/// one.
#[test]
fn a_starred_target_counts_and_confers_no_length_on_the_rest() {
    let source = walking("a, *rest", "pairs");
    let function = subject(&source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert_counted_by(&result, "len(pairs)", &source);
    assert!(
        result.holes().is_empty(),
        "`*rest` collects the leftovers of each element; how many elements there \
         are is still `len(pairs)`. Got {shown} for:\n{source}"
    );
    assert_elements_are_not_integers(&function, &result, &["a", "rest"], &source);
    assert_only_supplied(&function, &result, &source);
}

/// **A tuple target over a literal display counts what the display holds.**
///
/// The only shape this lane unblocks on its own, and all four instances of it in
/// the standard library are this exact spelling:
///
/// ```text
/// dataclasses.py:1101   for name, op in [('__lt__', '<'), ...]
/// plistlib.py:428       for bom, encoding in (...)
/// test/libregrtest/refleak.py:165, test/libregrtest/utils.py:667
/// ```
///
/// `LAN-91`'s `walk_display` already counts a display by the number of values
/// written in the source, and already builds the display's own elements as
/// statements in front of the loop so that `for x in [g(n), 2]` still pays for
/// `g`. Nothing about that needs to change; it simply has to be reachable from a
/// tuple target.
///
/// # The arithmetic, hand computed
///
/// Two elements, so the loop runs twice. Measured for the single-name spelling
/// `for x in [(1, 2), (3, 4)]`, which is the same loop: `Theta(6)` - two
/// statements outside plus `2 * (1 loop step + 1 body statement)`. The tuple
/// spelling unpacks each element and so may cost more per iteration; it may
/// never cost less, and may never run a different number of times. Six is
/// asserted as a floor rather than as an equality so the test survives a change
/// to what a statement costs.
///
/// Today: `Partial(3 + #hole0)` on `complex-assignment-target`.
#[test]
fn a_tuple_target_over_a_literal_display_counts_what_the_display_holds() {
    let source = "\
def g() -> int:
    total = 0
    for a, b in [(1, 2), (3, 4)]:
        total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        loop_is_counted(&result),
        "the display holds two values, written in the source, so the loop runs \
         twice - and `walk_display` has counted exactly this since `LAN-91`. \
         Only the target check above it stands in the way. Got {shown} \
         for:\n{source}"
    );
    assert!(
        result.is_complete(),
        "a two-element display of integer literals contains nothing unknown, so \
         this function is a finished number. Got {shown} for:\n{source}"
    );
    assert!(
        at(&result, 0) >= 6,
        "the loop runs twice and each iteration costs at least a step and a body \
         statement, on top of `total = 0` and `return total`: at least 2 + 2 * 2 \
         = 6. Got {} from {shown} for:\n{source}",
        at(&result, 0)
    );
    assert_elements_are_not_integers(&function, &result, &["a", "b"], source);
    assert_only_supplied(&function, &result, source);
}

// ---------------------------------------------------------------------------
// 2 · the names the target binds are not integers
// ---------------------------------------------------------------------------

/// **Neither name of a tuple target is readable as an integer.**
///
/// The half of this lane that is a *refusal*, and the reason the counter has to
/// stay synthetic. `a` and `b` hold pieces of an element of a `list`, and
/// `for a, b in [(1, 'x'), (2, 'y')]` is legal Python - so `b` may be a string.
/// [`landav_its::VarName`] promises a mathematical integer, so neither name may
/// become one, and `total = total + a` must keep refusing as `non-integer-value`
/// while the loop around it is still counted.
///
/// Both spellings are exercised because an implementation that walked only the
/// first element of the target would get `a` right and `b` wrong, and a single
/// test would not say which.
///
/// # Two claims, one test, and the tension between them is the point
///
/// Today the refusal half already holds - measured, `total = total + a` yields
/// `non-integer-value` at the read and the bound mentions neither name - and the
/// counting half does not. That is the failure to read in the message: the loop
/// must gain a trip count **without** the names gaining values. A change that
/// bound `a` to the `#walk` counter to make the read succeed would pass the
/// counting assertion and fail these, which is the whole reason they are stated
/// together.
///
/// **Deferred, and the evidence says it is not a tuple-target property.** The
/// refusal half already holds today; what fails is the counting half, for a
/// reason this file does not own. The body is `total = total + a`, which
/// condemns `total`, which makes the *preceding* `total = 0` a
/// `non-integer-value` refusal - and `Construct::NonIntegerValue`
/// `::may_rebind_locals()` is `true`, so that statement is a frame-wide region
/// that clears `len(pairs)` before the loop is ever reached.
///
/// Measured, and this is what makes it not ours: the identical function with a
/// *single-name* target reports exactly the same holes, and did so before tuple
/// targets existed. Replace the body with one that does not accumulate and the
/// tuple loop counts correctly at `2 + len(pairs) * (3 + ...)`.
///
/// The fix is to teach a refusal to forget the one local it can actually change
/// rather than the whole frame, which is new `landav-its` and `landav-engine`
/// surface with corpus-wide effect - and is the same bottleneck that leaves 75
/// of 91 reachable loops without an endpoint. Doing it inside `LAN-99` would
/// have moved a great many numbers under cover of this ticket.
#[test]
#[ignore = "blocked by region granularity, not by tuple targets: a refusal \
            forgets the whole frame, so an earlier `non-integer-value` clears \
            the length. A single-name target behaves identically."]
fn neither_name_of_a_tuple_target_is_readable_as_an_integer() {
    for element in ["a", "b"] {
        let source = format!(
            "\
def g(pairs: list) -> int:
    total = 0
    for a, b in pairs:
        total = total + {element}
    return total
"
        );
        let function = subject(&source);
        let result = cost(function.program());
        let shown = describe(&result);

        assert_counted_by(&result, "len(pairs)", &source);
        assert_elements_are_not_integers(&function, &result, &["a", "b"], &source);
        assert!(
            refuses(&function, "non-integer-value", element)
                || refuses(&function, "non-integer-value", "total"),
            "reading `{element}` must still refuse: it holds a piece of an \
             element and this fragment cannot do arithmetic on it. The ledger \
             holds {:?} for:\n{source}",
            refusals(&function)
        );
        assert!(
            !result.is_complete(),
            "the body adds a value this analysis cannot read, so the function \
             makes no complete claim even though the loop is counted. A finished \
             number here means `{element}` acquired a value it must not have. \
             Got {shown} for:\n{source}"
        );
        assert_only_supplied(&function, &result, &source);
    }
}

// ---------------------------------------------------------------------------
// 3 · the counting is conditional on the ITERABLE, never on the target
// ---------------------------------------------------------------------------

/// **A tuple target over something with no known length still holes.**
///
/// The fence that stops this lane being "won" by counting every tuple-target
/// loop. `mystery()` is user code: it may yield three values or three million,
/// and passing nothing to it is evidence about nothing. Removing the target
/// refusal must leave this loop refused - by `unbounded-iteration`, one construct
/// further in, where the actual problem is.
///
/// # This is a driver, and the assertion that drives it is the tag
///
/// Today this reports `Partial(3 + #hole0)` on **`complex-assignment-target`**,
/// so it is already incomplete and already uncounted - a test that asked only
/// for incompleteness would be green today and would stay green if the tag never
/// moved. What must change is *which* construct is blamed: today the report tells
/// a user to rewrite their target, when rewriting the target would not help,
/// because the iterable is the thing nothing is known about. A per-construct
/// ledger that files 205 loops under the wrong construct is a ledger that
/// mis-directs whoever reads it.
#[test]
fn a_tuple_target_over_an_unknown_iterable_still_holes() {
    let source = "\
def g(n: int) -> int:
    total = 0
    for a, b in mystery():
        total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        holes_on(&result, "unbounded-iteration"),
        "the target is not what stops this loop being counted - `mystery()` is. \
         The refusal must move to the iterable, or the report keeps telling users \
         to rewrite a `for` target that was never the problem, and the ledger \
         keeps filing the wrong construct. Got {shown} for:\n{source}"
    );
    assert!(
        !holes_on(&result, "complex-assignment-target"),
        "once a tuple target is a shape this frontend handles, it may not also be \
         charged as a refusal: the statement would be paid for twice and named \
         for a construct that is no longer a blocker. Got {shown} for:\n{source}"
    );
    assert!(
        !result.is_complete(),
        "nothing is known about `mystery`, so this function makes no complete \
         claim: {shown} for:\n{source}"
    );
    assert_only_supplied(&function, &result, source);
}

/// **`for k, v in mapping.items()` counts as `len(mapping)`.**
///
/// The headline shape of the whole ticket: 54 stdlib loops iterate `.items()`,
/// the largest length-preserving family in the corpus, and this is what every
/// one of them looks like.
///
/// # This test needs BOTH halves of `LAN-99` and will stay red after either alone
///
/// It is behind two independent refusals. This lane removes the first - the
/// target - and `length_relations.rs` is where the second lives: `.items()` is a
/// call the frontend cannot read, and teaching it that a mapping's items are as
/// many as the mapping is that file's
/// `a_walk_over_items_counts_by_the_mappings_length`, written in the single-name
/// spelling for exactly this reason. Neither half moves this test on its own,
/// and it is written at full strength anyway because it is the shape the ticket
/// was opened for. If it is still red when both land, the two changes did not
/// meet.
///
/// The near-miss to keep in view: `for k, v in mapping` - iterating the dict
/// directly - needs **only** this lane, because a `dict` parameter is already a
/// collection and `for k in mapping` is already counted at
/// `Theta(2 + 2 * len(mapping))` today. That spelling is rare in the corpus,
/// which is the whole reason the relation is needed beside this.
///
/// Today: `Partial(3 + #hole0)` on `complex-assignment-target`, with `.items()`
/// never reached - measured, the ledger names no callee at all, because the
/// statement is refused before its iterable is translated.
#[test]
fn a_tuple_target_over_items_counts_by_the_mappings_length() {
    let source = "\
def g(mapping: dict) -> int:
    total = 0
    for k, v in mapping.items():
        total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());

    assert_counted_by(&result, "len(mapping)", source);
    assert_elements_are_not_integers(&function, &result, &["k", "v"], source);
    assert_only_supplied(&function, &result, source);
}

// ---------------------------------------------------------------------------
// 4 · soundness - the bound may never fall below the truth
// ---------------------------------------------------------------------------

/// **A tuple target does not swallow a call in its iterable.**
///
/// The specific way this lane goes wrong, and it is the same shape
/// `length_relations.rs::a_length_relation_does_not_swallow_a_call_in_its_argument`
/// found from the other side - with the target refusal in front of it instead of
/// the relation.
///
/// Measured: `for a, b in sorted(expensive(records))` puts exactly one record in
/// the ledger, `("complex-assignment-target", "")`. **Neither `sorted` nor
/// `expensive` appears anywhere**, because `Translator::for_loop` returns on the
/// target before the iterable is ever translated, so both calls exist in no
/// arena and are invisible to the refusal scan.
///
/// While the statement is a hole denoting `omega` nothing is lost - `omega`
/// dominates whatever `expensive` costs. **Counting the loop takes that cover
/// away**, and a function that runs `expensive(records)` would report a bound
/// with `expensive` missing from it: a number below the truth, published as an
/// analysed result. So the iterable must start being translated in the same
/// change that starts counting, not afterwards.
///
/// Asserted on the ledger because that is the only place a callee's name
/// survives - a hole carries the construct tag and nothing else.
#[test]
fn a_tuple_target_does_not_swallow_a_call_in_its_iterable() {
    let source = "\
def g(records: list) -> int:
    total = 0
    for a, b in sorted(expensive(records)):
        total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());

    assert!(
        refuses(&function, "call", "expensive"),
        "`expensive(records)` is an unknown call standing inside this loop's \
         iterable, and it runs. The ledger holds {:?} - it names neither call, \
         because the statement is refused for its **target** before the iterable \
         is translated - so nothing is accounting for `expensive`, and counting \
         this loop while that is true produces a complete-looking bound that \
         omits it. This is the direction that must never happen: a bound below \
         the truth. For:\n{source}",
        refusals(&function)
    );
    assert!(
        !result.is_complete(),
        "while the iterable holds an unknown call this function makes no complete \
         claim: {} for:\n{source}",
        describe(&result)
    );
}

/// **The loop is charged per iteration, and a nest of two is quadratic.**
///
/// The direction assertion this file is required to make explicitly. A tuple
/// target counted wrongly under-reports here in the way that matters: if the
/// inner walk were counted once instead of `len(pairs)` times, or charged as a
/// constant, the answer would be **linear** for a program that is quadratic, and
/// linear is exceeded by the program at every input above a handful.
///
/// # The arithmetic, hand computed against the measured single-name nest
///
/// `for x in pairs: for y in other: total = total + 1` measures today at
///
/// ```text
/// Theta(2 + len(pairs) * (1 + 2 * len(other)))
/// ```
///
/// Two outside, one loop step per outer iteration, and an inner loop costing two
/// per inner iteration. With both lengths at `n` that is `2 + n + 2n^2`: **212**
/// at `n = 10` and **822** at `n = 20`. The tuple spelling unpacks each element
/// as well, so it may cost more per iteration and may never cost less.
///
/// The ratio is asserted rather than either literal, following
/// `collection_length.rs::a_nested_walk_is_quadratic_in_the_length`, so the test
/// survives a change to what a statement costs; `822 >= 3 * 212 = 636` holds
/// with room, while every linear answer fails it. Completeness is checked first
/// because [`at`] reads an unfilled hole as zero, and a growth ratio computed
/// over one is exactly the under-report this section polices.
#[test]
fn a_nest_of_tuple_target_walks_is_quadratic() {
    let source = "\
def g(pairs: list, other: list) -> int:
    total = 0
    for a, b in pairs:
        for c, d in other:
            total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert_counted_by(&result, "len(pairs)", source);
    assert!(
        mentioned(&result).iter().any(|var| var == "len(other)"),
        "the inner loop runs `len(other)` times on **every** outer iteration, so \
         both lengths must stand in the bound. A bound over only one of them has \
         dropped a whole loop. Got {shown} for:\n{source}"
    );
    assert!(
        result.is_complete(),
        "two collections the caller supplied and a body that adds one to a proven \
         integer: nothing here is unknown, so this must be a finished number \
         before its growth can be read at all. Got {shown} for:\n{source}"
    );
    let (small, large) = (at(&result, 10), at(&result, 20));
    assert!(
        large >= small * 3,
        "doubling both collections must more than double the cost of a nested \
         walk - {small} at length 10 and {large} at 20 is not quadratic growth, \
         so the inner walk is not being counted once per outer iteration. A \
         linear bound here is exceeded by the program, which is the one direction \
         that must never happen. Got {shown} for:\n{source}"
    );
    assert_only_supplied(&function, &result, source);
}

/// **A tuple-target walk costs at least the plain walk it replaces.**
///
/// `for a, b in pairs` and `for x in pairs` run the same number of times.
/// `LAN-89` already derives the second exactly, at `Theta(2 + 2 * len(pairs))`,
/// so the first has a *derived* baseline to be checked against rather than a
/// literal - which means the property survives any change to what a statement
/// costs, and states the thing that actually matters.
///
/// The check is one-sided on purpose: the tuple form does strictly more work per
/// iteration - it takes the element apart - so it may cost more, and it may
/// never cost less. A trip count taken from the target's arity rather than the
/// iterable's length shows up here immediately: `for a, b in pairs` counted as
/// **2** would be a bound the program exceeds at every `len(pairs) > 2`, and
/// that is the most plausible wrong implementation of this lane.
///
/// The baseline's own completeness is asserted first, because a comparison
/// against a partial bound whose holes read as zero would be meaningless in
/// precisely the direction being tested.
#[test]
fn a_tuple_target_walk_costs_at_least_the_plain_walk_it_replaces() {
    let source = walking("a, b", "pairs");
    let function = subject(&source);
    let result = cost(function.program());
    let shown = describe(&result);

    let baseline_source = walking("x", "pairs");
    let baseline = cost(subject(&baseline_source).program());
    assert!(
        baseline.is_complete(),
        "the plain-walk baseline must be complete or this comparison means \
         nothing: {}",
        describe(&baseline)
    );
    assert!(
        result.is_complete(),
        "`for a, b in pairs` walks a collection the caller supplied and does \
         nothing unknown in the body, so it is a finished number and can be \
         compared against the walk it stands in for. Got {shown} for:\n{source}"
    );

    for n in [10_u64, 20] {
        let unpacked = at(&result, n);
        let plain = at(&baseline, n);
        assert!(
            unpacked >= plain,
            "`for a, b in pairs` runs exactly as many times as `for x in pairs` \
             and does strictly more work each time, so it may never report less: \
             {unpacked} against {plain} at `len(pairs) = {n}`. A number below the \
             baseline means the trip count came from the **target** - two names, \
             so two iterations - rather than from the iterable's length, and it \
             is a bound the program exceeds. Got {shown} against {} \
             for:\n{source}",
            describe(&baseline)
        );
    }
}

/// **A tuple-target walk does not erase the loop that follows it.**
///
/// The collateral damage, which is larger than the statement itself and is the
/// reason this lane is worth more than its own line count.
///
/// A refused statement is a region, and a region may change anything - so every
/// value read after it is a value no longer known. Measured: with a plain target
/// this function is complete at `Theta(2 + 2 * len(pairs) + 2 * n)`, and with a
/// tuple target it is `Partial(3 + #hole0 + #hole1)` in which **`n` does not
/// appear at all** and the second loop is separately holed as `for`. One
/// unsupported target costs the trip count of a loop that has nothing to do with
/// it.
///
/// That is what makes 205 refused statements worth more than 205 loops: each one
/// also blinds whatever follows it in the same function. Asserted on the
/// *second* loop's variable, because that is the one that must come back.
#[test]
fn a_tuple_target_walk_does_not_erase_the_loop_after_it() {
    let source = "\
def g(pairs: list, n: int) -> int:
    total = 0
    for a, b in pairs:
        total = total + 1
    for i in range(n):
        total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        mentioned(&result).iter().any(|var| var == "n"),
        "the second loop runs `n` times and has nothing to do with the first, but \
         a refused statement is a region that may change anything - so refusing \
         the tuple target throws away `n` as well. The bound must mention it. Got \
         {shown} for:\n{source}"
    );
    assert_counted_by(&result, "len(pairs)", source);
    assert!(
        result.is_complete(),
        "both loops are counted and nothing in this function is unknown, so it is \
         a finished number. Got {shown} for:\n{source}"
    );
    assert_only_supplied(&function, &result, source);
}

// ---------------------------------------------------------------------------
// 5 · regressions - this lane is about `for` targets and nothing else
// ---------------------------------------------------------------------------

/// **A tuple target in an assignment is still refused.**
///
/// The boundary of this lane, stated so it cannot drift. `a, b = f()` binds two
/// names to two values, and `landav_its`'s assignment binds **one** name to
/// **one** polynomial - there is no expression in the fragment for "the second
/// element of whatever `f` returned", and there deliberately is not going to be
/// one. So `Translator::assign` must keep refusing as `complex-assignment-target`.
///
/// A `for` target is a different question with a different answer, and the
/// difference is not cosmetic: the `for` needs a **count**, which is a property
/// of the iterable and is available; the assignment needs a **value**, which is
/// a property of the element and is not. Counting a loop while binding nothing
/// is coherent. Binding a name to nothing is not.
///
/// Both spellings are pinned because they fail for different reasons and a change
/// could reach one without the other: an unknown call on the right, where the
/// call must *also* still be named in the ledger, and a literal tuple on the
/// right, where nothing is unknown at all and the refusal is purely about the
/// target.
///
/// A regression guard, passing today at `Partial(2 + #hole0 + #hole1)` and
/// `Partial(2 + #hole0)` respectively.
#[test]
fn a_tuple_target_in_an_assignment_is_still_refused() {
    for source in [
        "def g(n: int) -> int:\n    a, b = f()\n    return n\n",
        "def g(n: int) -> int:\n    a, b = 1, 2\n    return n\n",
    ] {
        let function = subject(source);
        let result = cost(function.program());
        let shown = describe(&result);

        assert!(
            holes_on(&result, "complex-assignment-target"),
            "this lane widens the `for` **target**, not the assignment target. \
             An assignment needs a value for each name and the fragment has none \
             - `landav_its::Update` is a total map with no havoc, so admitting \
             this would assert `b` is unchanged across a statement that changes \
             it. Got {shown} for:\n{source}"
        );
        assert!(
            !result.is_complete(),
            "a statement this frontend refuses leaves a hole, so the function \
             makes no complete claim: {shown} for:\n{source}"
        );
    }
    let function = subject("def g(n: int) -> int:\n    a, b = f()\n    return n\n");
    assert!(
        refuses(&function, "call", "f"),
        "the unknown call on the right of a refused assignment still runs and \
         must still be named - `refusals_of` translates the value before refusing \
         the target, and that must not be lost. The ledger holds {:?}",
        refusals(&function)
    );
}

/// **`a[i] = 0` still does not erase the loop that produced `i`.**
///
/// `LAN-91`'s fix, re-asserted from this lane because this lane is the one that
/// could undo it. `written_names` deliberately reports the *container* of a
/// subscript write and not the index, so `a[i] = 0` condemns `a` and leaves `i`
/// the integer the loop bound it to. Measured, 29 of the standard library's 166
/// `range` loops write through their own counter, and before that fix every one
/// of them was refused whole - body, nested loops and all.
///
/// The connection is direct: teaching `for` to walk a target tree means touching
/// `written_names`' neighbourhood, and the obvious "simplification" there -
/// collecting every name a target mentions, now that targets are trees - is
/// exactly the bug `LAN-91` removed.
///
/// A regression guard, passing today: the loop is counted by `n` and the write
/// itself is the only hole, at `Partial(2 + n * (3 + #hole0))`.
#[test]
fn indexing_with_the_loop_counter_still_keeps_its_loop() {
    let source = "\
def g(n: int, a: list) -> int:
    t = 0
    for i in range(n):
        a[i] = 0
        t = t + 1
    return t
";
    let function = subject(source);
    let result = cost(function.program());
    let names = mentioned(&result);

    assert!(
        names.iter().any(|var| var == "n"),
        "the counter `i` is only *read* by the index, so it is still the integer \
         the loop bound it to and the loop is still counted by `n`. The bound \
         mentions {names:?} instead: condemning every name in a write target \
         throws away the loop, its body and any loop nested inside it. Got {} \
         for:\n{source}",
        describe(&result)
    );
    assert!(
        holes_on(&result, "complex-assignment-target"),
        "the write itself is still a refusal - this lane widens `for` targets, \
         not subscript assignment - and it must stay charged inside the loop, \
         once per iteration. Got {} for:\n{source}",
        describe(&result)
    );
    assert_only_supplied(&function, &result, source);
}

/// **A plain walk over a collection parameter still counts, exactly.**
///
/// `LAN-89`'s headline, and the baseline every comparison in this file is made
/// against. The tuple target is meant to reach `walk_collection`, which is the
/// code this depends on, so a change that generalises it can break it.
///
/// The exactness assertion is the load-bearing one. `for x in items` runs
/// `len(items)` times, not at most that: the iteration space is fixed before the
/// first iteration. `Theta` against `O` is a distinction this tool prints, and a
/// generalisation that weakened the endpoint to reach more shapes would make
/// every existing bound one-sided while no other test noticed.
///
/// A regression guard, passing today at `Theta(2 + 2 * len(items))`.
#[test]
fn a_plain_walk_over_a_collection_parameter_still_counts() {
    let source = "\
def g(items: list) -> int:
    total = 0
    for x in items:
        total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert_counted_by(&result, "len(items)", source);
    assert!(
        result.holes().is_empty(),
        "nothing in this function is unknown, so it must stay a complete claim \
         with no holes at all: {shown} for:\n{source}"
    );
    assert!(
        result.is_exact(),
        "the loop runs exactly `len(items)` times - the iteration space is fixed \
         before the loop begins - so this is `Theta` and not `O`. A \
         generalisation that weakened the endpoint to reach a tuple target would \
         make every existing bound one-sided: {shown} for:\n{source}"
    );
    assert!(
        landav_its::lower(function.program()).is_ok(),
        "this function lowers today and must keep lowering - it is counted in the \
         headline coverage figure. The ledger holds {:?} for:\n{source}",
        refusals(&function)
    );
}

// ---------------------------------------------------------------------------
// 6 · lowering - the judgment call, argued rather than guessed
// ---------------------------------------------------------------------------

/// **A counted tuple-target loop lowers.**
///
/// The question the brief asks to be decided rather than assumed, so here is the
/// argument.
///
/// `Coverage::lowered()` counts transition systems, and a construct may only
/// raise it by genuinely entering the fragment - `LAN-91` established that a
/// legitimate rise is allowed, and `attribute_and_subscript.rs`'s
/// `a_read_still_stops_the_program_from_lowering` established the other side:
/// the engine reaching further through holes is a different number and must not
/// move this one.
///
/// The objection to raising it here is that a tuple target binds names the
/// fragment cannot represent. That objection is true, and it does not
/// distinguish this case from one already shipped: **`for x in items` binds a
/// name the fragment cannot represent either**. `walk_collection` builds a
/// `ForRange` over a synthetic `#walk0` counter and never mentions `x` at all;
/// `x` enters no `Update`, no `VarName`, nothing. Measured, that function lowers
/// today, and `length_relations.rs` pins it. `a` and `b` are in exactly the same
/// position as `x`: doomed by `integer_names`, absent from the transition system,
/// and refusing at every read. There is no name here the fragment is being asked
/// to represent and failing to.
///
/// So the honest answer is **yes, it lowers**, and the rise in
/// `Coverage::lowered()` is the real thing rather than an artefact - the same
/// rise `LAN-89` earned for the single-name spelling, now available to 205 more
/// loops' worth of statement.
///
/// The counter-example is pinned beside it, because "it lowers" is conditional
/// and the condition is the one that makes the whole design sound: a body that
/// **reads** one of the names does not lower, and must not, because that read is
/// a refusal. If both halves ever pass, the names became values and section 2 is
/// the file to look at.
#[test]
fn a_counted_tuple_target_loop_lowers() {
    let counted = walking("a, b", "pairs");
    let function = subject(&counted);
    assert!(
        landav_its::lower(function.program()).is_ok(),
        "nothing about a tuple target is unrepresentable: the loop becomes a \
         `ForRange` over a synthetic `#walk` counter and the names it unpacks \
         enter the transition system no more than `x` does in `for x in items`, \
         which lowers today. The ledger holds {:?} for:\n{counted}",
        refusals(&function)
    );

    let reading = "\
def g(pairs: list) -> int:
    total = 0
    for a, b in pairs:
        total = total + b
    return total
";
    let reader = subject(reading);
    assert!(
        landav_its::lower(reader.program()).is_err(),
        "reading `b` is a refusal - it holds a piece of an element and may be a \
         string - so this program must **not** lower, however well the loop \
         around it is counted. If this passes at the same time as the assertion \
         above, the target's names acquired values they must not have. \
         For:\n{reading}"
    );
}
