//! `LAN-91` acceptance: **`x.y` and `x[i]` stop costing more than they are.**
//!
//! # These are reads of something the fragment cannot see into
//!
//! An attribute or a subscript is refused as a *value* today, and that refusal
//! is correct and must survive this ticket. [`landav_its::VarName`] carries a
//! hard promise - the name denotes a mathematical integer - and `x.y` may be a
//! string, a float, a list or a property that returns a different thing each
//! time. Admitting one as an integer would put a non-number into every guard
//! and every trip count that reads it, which is the one direction a resource
//! bound may not move. So this file asserts the refusal *stays*, in
//! [`neither_an_attribute_nor_a_subscript_becomes_a_readable_integer`] and
//! [`an_attribute_range_endpoint_does_not_become_a_trip_count`], and everything
//! else it asserts is about what the refusal **costs** and **who it blames**.
//!
//! # What was measured before any of this was written
//!
//! Over the 3057 top-level functions of the Python 3.12 standard library,
//! `landav_engine::cost` charges 26557 holes. `attribute` accounts for 930 of
//! them and `subscript` for 714 - the two counts this ticket names. 43
//! functions are blocked by nothing else.
//!
//! Those 43 are the surprise, and they redirect the ticket. Forty-two of them
//! report `Partial(1 + #hole0)` and the forty-third `Partial(2 + ...)`: they are
//! one-line accessors whose entire body is a read. There is no bound hiding
//! inside them to recover, and there is no honest constant to charge them
//! either - `x.y` runs a `property` and `x[i]` runs `__getitem__`, both of which
//! are arbitrary user code. `Partial(1 + omega)` is the true answer for a
//! `return self._x` whose class the fragment never saw. **A value model would
//! buy those 43 functions nothing, and would cost the soundness argument
//! above.**
//!
//! What the two constructs actually cost the corpus is paid somewhere else:
//!
//! * **Cascade.** `non-integer-value` is the largest hole kind in the corpus by
//!   a wide margin - 10561 holes across 1991 of the 3057 functions - and it is
//!   mostly *derived*: `x = obj.k` dooms `x`, so every later read of `x` refuses
//!   too. One attribute read seeds a chain of refusals that name a local rather
//!   than the construct that caused it.
//! * **Double charge.** 100 functions carry a hole whose construct *and*
//!   position are identical to another hole's - 265 duplicated regions - and 9
//!   of those duplicates are an attribute or a subscript. `n += a[0]` charges
//!   `a[0]` twice at the same column. See
//!   [`an_augmented_assignment_charges_its_read_once`].
//! * **Erasure by a write.** 166 of the standard library's 2281 `for` loops
//!   iterate a `range`, and 29 of those - 17% - contain `x[i] = v` for the
//!   loop's own counter. Every one of them is refused *whole*: the counter is
//!   condemned as non-integer, the loop's body is never lowered, and any nested
//!   counted loop inside it vanishes from the ledger with it. See
//!   [`indexing_with_the_loop_counter_does_not_erase_the_loop`].
//! * **Erasure by a read.** A bare `obj.field` on the line *above* a counted
//!   loop turns `Theta(2 + 2n)` into `Partial(3 + #hole0 + #hole1)` - the
//!   parameter leaves the bound entirely. The same read *below* the loop keeps
//!   `2n`. See [`a_read_does_not_erase_a_later_loops_trip_count`], and see
//!   [`a_read_still_forgets_a_collection_length`] for the half of that which
//!   must not move.
//!
//! # Why these tests read the libraries rather than driving the binary
//!
//! As in [`collection_length`] and [`calls_become_holes`]: the assertions are
//! about the *shape* of a bound - which variables it mentions, where a hole was
//! placed, whether one region got charged twice - and the process boundary
//! offers only the rendered string, which these tests are forbidden to pin.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

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
    let mut functions = landav_python::lower_module(Path::new("access.py"), source)
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
        .map(|hole| format!("{}@{}", hole.construct(), hole.origin()))
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

/// The parameters the caller supplies.
fn params(function: &LoweredFunction) -> Vec<String> {
    function
        .program()
        .params()
        .iter()
        .map(|param| param.symbol().as_str().to_owned())
        .collect()
}

/// The holes charged for `construct`.
fn holes_named<'a>(result: &'a TripCount, construct: &str) -> Vec<&'a Hole> {
    result
        .holes()
        .iter()
        .filter(|hole| hole.construct() == construct)
        .collect()
}

/// The result must name the read, place it, and put its variable in the bound.
///
/// One helper rather than three assertions per test, because a region that was
/// not named, one that was not placed and one that was named but left out of
/// the arithmetic are three different failures with one symptom, and the
/// message is what tells the implementer which.
fn assert_blames(result: &TripCount, construct: &str, source: &str) -> Hole {
    let shown = describe(result);
    assert!(
        matches!(result, TripCount::Partial { .. }),
        "a function containing `{construct}` is not derivable and must say so \
         with a hole rather than a complete claim, got {shown} for:\n{source}"
    );
    let hole = holes_named(result, construct)
        .first()
        .copied()
        .unwrap_or_else(|| {
            panic!(
                "the region must be blamed on the read by name - `{construct}` \
                 is the construct the user can act on - got {shown} \
                 for:\n{source}"
            )
        })
        .clone();
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
    hole
}

/// Every name a bound mentions must be a parameter or a hole.
///
/// A hole variable is exempt only while the result is **partial**: an unfilled
/// hole denotes `omega`, so a partial makes no finite claim. A name that is
/// neither is a name `Bound::eval` reads as zero, which turns a cost the caller
/// cannot supply into no cost at all.
fn assert_only_supplied(function: &LoweredFunction, result: &TripCount, source: &str) {
    let declared = params(function);
    let partial = !result.is_complete();
    for name in mentioned(result) {
        assert!(
            declared.contains(&name) || (partial && name.starts_with("#hole")),
            "the bound mentions `{name}`, which is neither a parameter of the \
             function nor a hole - the caller has nothing to supply for it, so \
             `Bound::eval` reads it as zero. Parameters are {declared:?}, got \
             {} for:\n{source}",
            describe(result),
        );
    }
}

/// The bound's value with every named variable bound to a concrete number.
fn value_at(result: &TripCount, pairs: &[(&str, u64)]) -> Nat {
    let table = pairs
        .iter()
        .map(|(name, value)| (Symbol::from(*name), *value))
        .collect();
    result
        .bound()
        .expect("a result carrying a bound")
        .eval(&Bindings(table))
}

// ---------------------------------------------------------------------------
// the two reads are named, placed, and do not erase the function
// ---------------------------------------------------------------------------

/// **`x = obj.field` names its region and still derives a bound.**
///
/// Regression guard, and the floor everything else stands on. Without it the
/// obvious "simplification" of `build_expression` - folding `Attribute` into
/// the `NonIntegerValue` arm that already fires on the statement beside it -
/// would lose the only word in the report that tells the user what to change,
/// and no other test in the tree would notice.
#[test]
fn an_attribute_read_is_a_named_placed_region() {
    let source = "def g(n: int) -> int:\n    x = obj.field\n    return n\n";
    let function = only_function(source);
    let result = cost(function.program());

    let _ = assert_blames(&result, "attribute", source);
    assert_only_supplied(&function, &result, source);
}

/// **`x = items[0]` names its region and still derives a bound.**
///
/// The same guard for the other construct. `subscript` and `attribute` are
/// separate words in the vocabulary because they are separate things to fix -
/// one is a class the analysis never saw, the other is a container - and
/// collapsing them would halve the actionability of 1644 holes.
#[test]
fn a_subscript_read_is_a_named_placed_region() {
    let source = "def g(n: int, items: list) -> int:\n    x = items[0]\n    return n\n";
    let function = only_function(source);
    let result = cost(function.program());

    let _ = assert_blames(&result, "subscript", source);
    assert_only_supplied(&function, &result, source);
}

/// **A chain is one region, not one per link.**
///
/// Regression guard on `expression_children`, which deliberately gives a
/// refused form no children so that its interior is never walked. `a.b.c.d.e`
/// is one read the analysis cannot see into, not four, and charging four holes
/// would quadruple the region count of the most common shape in the corpus and
/// make the 930/714 measurement meaningless.
///
/// The mixed chain is the case a naive per-node walk gets wrong in a second
/// way: `a.b[0].c[1]` is one region, and it does not matter which of the two
/// words is used to name it as long as exactly one is.
#[test]
fn a_chain_of_accesses_is_one_region_not_one_per_link() {
    for source in [
        "def g(n: int) -> int:\n    a.b.c.d.e\n    return n\n",
        "def g(n: int, a: list) -> int:\n    a[0][1][2]\n    return n\n",
        "def g(n: int, a: list) -> int:\n    a.b[0].c[1]\n    return n\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        let reads =
            holes_named(&result, "attribute").len() + holes_named(&result, "subscript").len();
        assert_eq!(
            reads,
            1,
            "a chain is one thing the analysis cannot see into, so it is one \
             region: charging one per link inflates the cost of the commonest \
             shape in the corpus and names the same fix several times. Got {} \
             for:\n{source}",
            describe(&result)
        );
        assert_only_supplied(&function, &result, source);
    }
}

/// **A read in a loop body is paid once per iteration.**
///
/// Regression guard, and the assertion the whole coverage claim rests on: a
/// region charged once *outside* the loop understates every loop that touches
/// an attribute, which is nearly every loop in a method. The arithmetic is the
/// same as `calls_become_holes`' - `n * (2 + read)`, the loop's own step plus
/// the statement whose cost is the read's - and every plausible wrong answer is
/// separated by evaluating it:
///
/// | answer | at `n = 3`, `read = 5` |
/// |---|---|
/// | `n * (2 + read)` - correct | 21 |
/// | `n` - the read dropped | 3 |
/// | `n + read` - charged once, outside the loop | 8 |
#[test]
fn a_read_in_a_loop_body_is_paid_once_per_iteration() {
    for (source, construct) in [
        (
            "def g(n: int) -> int:\n    for i in range(n):\n        obj.field\n",
            "attribute",
        ),
        (
            "def g(n: int, a: list) -> int:\n    for i in range(n):\n        a[i]\n",
            "subscript",
        ),
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        let hole = assert_blames(&result, construct, source);
        let name = hole.var().symbol().as_str().to_owned();

        for (n, read, expected) in [(3_u64, 5_u64, 21_u64), (0, 5, 0), (4, 0, 8)] {
            assert_eq!(
                value_at(&result, &[("n", n), (&name, read)]),
                Nat::Fin(expected),
                "at n = {n} with the read costing {read} the loop costs \
                 n * (2 + read) = {expected}: {}\n\n\
                 `n + read` means the read was charged once outside the loop, \
                 which understates every loop that touches an attribute.",
                describe(&result)
            );
        }
        assert_only_supplied(&function, &result, source);
    }
}

// ---------------------------------------------------------------------------
// the soundness fence: neither may ever become a number
// ---------------------------------------------------------------------------

/// **An attribute or subscript range endpoint never becomes a trip count.**
///
/// The soundness-critical case, and a regression guard on the direction this
/// ticket must not be "fixed" in. `for i in range(obj.n)` is a loop whose trip
/// count the analysis does not know. If `obj.n` were admitted as a variable the
/// count would be a bound over a name the caller supplies nothing for, and
/// `Bound::eval` reads such a name as **zero** - so a loop that runs a million
/// times would report as costing nothing, completely, with no hole to warn
/// anyone. That is the precise failure `landav_engine::expr_bound::read`'s
/// `readable` argument exists to prevent, reached by a different road.
///
/// So the claim asserted here is only that the result stays **incomplete** and
/// mentions nothing but parameters and holes. It says nothing about *how* the
/// loop is refused, which is deliberate: the next test asserts the blame, and
/// that one is expected to change.
#[test]
fn an_attribute_range_endpoint_does_not_become_a_trip_count() {
    for source in [
        "def g(n: int) -> int:\n    t = 0\n    for i in range(obj.n):\n        t = t + 1\n    return t\n",
        "def g(a: list) -> int:\n    t = 0\n    for i in range(a[0]):\n        t = t + 1\n    return t\n",
        "def g(n: int) -> int:\n    t = 0\n    for i in range(0, obj.n):\n        t = t + 1\n    return t\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert!(
            !result.is_complete(),
            "the trip count of this loop is the value of a read the analysis \
             cannot see into, so there is no complete claim to make about it. A \
             complete bound here is one a budget gate would act on: {} \
             for:\n{source}",
            describe(&result)
        );
        assert_only_supplied(&function, &result, source);
    }
}

/// **The refusal blames the read, not the loop counter.**
///
/// `for i in range(obj.n)` is refused today as `non-integer-value` positioned
/// on the `for` line - the words `attribute` and `subscript` never appear. The
/// user reads "value is not a proven integer" against their loop counter `i`,
/// which they did not write and cannot change; the thing they can change is
/// `obj.n`, several columns to the right.
///
/// This is a measurement problem as well as a usability one. `landav_its::lower`
/// records `NonIntegerValue` for this shape and nothing else, so a loop refused
/// for an attribute endpoint is invisible to the very count - "attribute solely
/// blocks 23 functions" - that this ticket is scoped by. The construct that
/// caused the refusal has to appear in the ledger for the ledger to mean
/// anything.
///
/// Getting there also recovers structure that is thrown away today: the frontend
/// returns before `self.block(&loop_stmt.body)` runs, so the loop's body - and
/// any counted loop nested inside it - is never lowered at all. Building the
/// `ForRange` with an `Unsupported` stop expression instead keeps the body in
/// the program while `expr_bound::read` still answers `None` for the endpoint,
/// which is what keeps the previous test passing.
#[test]
fn a_refused_range_endpoint_blames_the_read_rather_than_the_counter() {
    for (source, construct) in [
        (
            "def g(n: int) -> int:\n    t = 0\n    for i in range(obj.n):\n        t = t + 1\n    return t\n",
            "attribute",
        ),
        (
            "def g(a: list) -> int:\n    t = 0\n    for i in range(a[0]):\n        t = t + 1\n    return t\n",
            "subscript",
        ),
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert!(
            !holes_named(&result, construct).is_empty(),
            "the loop was refused because its endpoint is a `{construct}`, and \
             that word must reach the report: blaming the counter `i` names \
             something the user did not write, and leaves the refusal out of \
             the per-construct ledger this ticket is scoped by. Got {} \
             for:\n{source}",
            describe(&result)
        );
        assert_only_supplied(&function, &result, source);
    }
}

/// **Neither ever becomes a readable integer value.**
///
/// The explicit statement of the promise, swept over every position a read can
/// occupy: an assignment, arithmetic, a condition, a range endpoint, a loop
/// body. In none of them may a bound acquire a variable that is not a declared
/// parameter or a hole.
///
/// Written as its own test rather than left implicit in the others because the
/// failure it guards against is silent. A bound mentioning `obj` or `n` derived
/// from `obj.n` still renders as a perfectly ordinary polynomial; nothing about
/// the printed answer says the variable came from a read the analysis invented
/// a value for. The only place it shows is here.
#[test]
fn neither_an_attribute_nor_a_subscript_becomes_a_readable_integer() {
    for source in [
        "def g(n: int) -> int:\n    x = obj.field\n    return n\n",
        "def g(n: int) -> int:\n    t = n + obj.k\n    return t\n",
        "def g(n: int, a: list) -> int:\n    t = n + a[0]\n    return t\n",
        "def g(n: int) -> int:\n    x = 0\n    if obj.flag:\n        x = 1\n    return x\n",
        "def g(n: int, a: list) -> int:\n    x = 0\n    if a[0]:\n        x = 1\n    return x\n",
        "def g(n: int) -> int:\n    t = 0\n    for i in range(obj.n):\n        t = t + 1\n    return t\n",
        "def g(n: int, a: list) -> int:\n    t = 0\n    for i in range(len(a)):\n        t = t + a[i]\n    return t\n",
        "def g(n: int) -> int:\n    k = obj.n\n    t = 0\n    for i in range(k):\n        t = t + 1\n    return t\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert_only_supplied(&function, &result, source);

        let declared = params(&function);
        for name in mentioned(&result) {
            assert!(
                !name.contains('.') && !name.contains('['),
                "`{name}` is spelled like a read, so something turned `x.y` or \
                 `x[i]` into a bound variable. The caller supplies no such \
                 value and `Bound::eval` reads it as zero. Parameters are \
                 {declared:?}, got {} for:\n{source}",
                describe(&result)
            );
        }
    }
}

/// **The cost of a subscript is never assumed constant.**
///
/// `x[i]` calls `__getitem__`. On a `dict` that is amortised constant and not
/// worst-case constant; on a `list` with a slice it is linear; on a user class
/// it is whatever the class does, including a database round trip. There is no
/// number to charge it, so the hole must stay *open* - present in the bound, so
/// that an unfilled hole reads as `omega` and the result makes no finite claim.
///
/// The guard is against the tempting shortcut of this ticket: "an index is O(1),
/// charge it one step and stop holing it". That would turn 714 regions into a
/// complete bound the tool would then offer for comparison against a budget.
#[test]
fn the_cost_of_a_read_is_never_assumed_constant() {
    for (source, construct) in [
        (
            "def g(n: int, a: list) -> int:\n    a[0]\n    return n\n",
            "subscript",
        ),
        (
            "def g(n: int, d: dict) -> int:\n    d[n]\n    return n\n",
            "subscript",
        ),
        (
            "def g(n: int) -> int:\n    obj.field\n    return n\n",
            "attribute",
        ),
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        let hole = assert_blames(&result, construct, source);
        assert!(
            !result.is_complete(),
            "`{construct}` runs user code whose worst case this fragment cannot \
             see, so its hole must stay open and the result must make no finite \
             claim: {} for:\n{source}",
            describe(&result)
        );
        let name = hole.var().symbol().as_str().to_owned();
        let low = value_at(&result, &[("n", 0), (&name, 0)]);
        let high = value_at(&result, &[("n", 0), (&name, 1000)]);
        assert_ne!(
            low,
            high,
            "the hole for `{construct}` does not move the bound, so it has been \
             charged a fixed cost in all but name: {} for:\n{source}",
            describe(&result)
        );
    }
}

// ---------------------------------------------------------------------------
// what the refusal costs: charged once, and not paid by the code around it
// ---------------------------------------------------------------------------

/// **`x += a[0]` charges the read once.**
///
/// It charges it twice today, at the same line and the same column, and the
/// binding refusal alongside it twice as well: five holes for a two-line
/// function. `aug_assign`'s non-integer path translates the value inside
/// `hoisted` to record the read, then hands the same expression to
/// `refuse_binding`, which translates it again.
///
/// Duplicated blame is not a cosmetic problem. The report names one fix twice;
/// `landav_its::lower`'s per-construct ledger counts one occurrence as two, so
/// the 930 and 714 this ticket is scoped by are both overstated; and once
/// `Bound::subst` starts filling holes with real costs, the same read's cost is
/// added to the bound twice. Corpus-wide: 100 functions carry a hole with a
/// duplicate construct-and-position, 265 duplicated regions in all.
///
/// Asserted on the pair rather than on a total, so that a fix which merely
/// renumbers the holes does not pass.
#[test]
fn an_augmented_assignment_charges_its_read_once() {
    for (source, construct) in [
        (
            "def g(n: int, m: int) -> int:\n    m += obj.k\n    return n\n",
            "attribute",
        ),
        (
            "def g(n: int, m: int, a: list) -> int:\n    m += a[0]\n    return n\n",
            "subscript",
        ),
    ] {
        let function = only_function(source);
        let result = cost(function.program());

        let mut seen: BTreeMap<(&str, String), usize> = BTreeMap::new();
        for hole in result.holes() {
            *seen
                .entry((hole.construct(), hole.origin().as_str().to_owned()))
                .or_default() += 1;
        }
        let repeated: Vec<_> = seen
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|((construct, origin), count)| format!("{construct}@{origin} x{count}"))
            .collect();
        assert!(
            repeated.is_empty(),
            "one construct at one position is one region: {repeated:?} are \
             charged more than once, so the report names the same fix twice, \
             the per-construct ledger overcounts it, and filling the hole adds \
             the read's cost to the bound twice. Got {} for:\n{source}",
            describe(&result)
        );
        assert_eq!(
            holes_named(&result, construct).len(),
            1,
            "`{construct}` appears once in the source and must be charged once: \
             {} for:\n{source}",
            describe(&result)
        );
    }
}

/// **A read does not erase a later loop's trip count.**
///
/// `obj.field` on the line above `for i in range(n)` costs the whole loop: the
/// clean function derives `Theta(2 + 2n)` and the one with the read above it
/// derives `Partial(3 + #hole0 + #hole1)`, with `n` gone from the bound
/// entirely. The same read *below* the loop keeps `2n`, which is what shows the
/// mechanism: `Walk::region` clears `readable`, so nothing after a region may be
/// read as the value the caller supplied, and `count_of` therefore has no
/// endpoint left to count with.
///
/// That rule is right for the case it was written for - `if (n := 100) > 0` is
/// refused as a region and really does rebind `n` - and wrong for a read. `x.y`
/// and `x[i]` run arbitrary user code, but there is no user code that can rebind
/// a local name in *this* frame: the fragment refuses `global`, `nonlocal` and
/// `del`, and the only expression in Python that rebinds a local is the walrus,
/// which is a different construct. An integer parameter still holds the caller's
/// value on the far side of `obj.field`.
///
/// The scope of the exemption is exactly the point, and
/// [`a_read_still_forgets_a_collection_length`] is the other half of it.
#[test]
fn a_read_does_not_erase_a_later_loops_trip_count() {
    for source in [
        "def g(n: int) -> int:\n    obj.field\n    t = 0\n    for i in range(n):\n        t = t + 1\n    return t\n",
        "def g(n: int, a: list) -> int:\n    a[0]\n    t = 0\n    for i in range(n):\n        t = t + 1\n    return t\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        let names = mentioned(&result);
        assert!(
            names.iter().any(|var| var == "n"),
            "reading an attribute or an index cannot rebind a local, so `n` \
             still holds what the caller passed and the loop below is still \
             counted by it. The bound mentions {names:?} instead - the same \
             loop with the read deleted derives `Theta(2 + 2n)`, and with the \
             read moved *below* it keeps `2n`, so the parameter was lost to the \
             read's position rather than to anything it could do. Got {} \
             for:\n{source}",
            describe(&result)
        );
        assert_only_supplied(&function, &result, source);
    }
}

/// **A read still forgets a collection's length.**
///
/// The fence around the previous test, and the reason it is scoped to integer
/// parameters. `obj.field` may be a `property` whose getter calls
/// `items.append(...)`, so `len(items)` after the read is not `len(items)` on
/// entry - exactly the case
/// `collection_length::appending_to_a_collection_forgets_its_entry_length`
/// already pins for an explicit call. An integer parameter is a name bound to
/// an immutable object and cannot be moved by anything the read does; a
/// collection's length can.
///
/// So a fix to `Walk::region` that keeps `readable` intact wholesale is unsound
/// and this test is what catches it. It passes today for the blunt reason that
/// everything is cleared.
#[test]
fn a_read_still_forgets_a_collection_length() {
    for source in [
        "def g(items: list) -> int:\n    obj.field\n    t = 0\n    for x in items:\n        t = t + 1\n    return t\n",
        "def g(items: list, a: list) -> int:\n    a[0]\n    t = 0\n    for x in items:\n        t = t + 1\n    return t\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert!(
            !result.is_complete(),
            "the read may run a `property` or a `__getitem__` that appends to \
             `items`, so the length the caller supplied no longer counts the \
             loop below it: {} for:\n{source}",
            describe(&result)
        );
        assert_only_supplied(&function, &result, source);
    }
}

// ---------------------------------------------------------------------------
// writes - a different thing from reads, and pinned separately
// ---------------------------------------------------------------------------

/// **`obj.f = 1` and `a[0] = 1` are placed regions and keep the loop counted.**
///
/// A write is not a read: nothing is being pulled out of an object the analysis
/// cannot see into, so there is no value question at all. The frontend refuses
/// it at *statement* level as `complex-assignment-target`, which is the honest
/// name - the target is not a plain variable - and the region carries
/// `Extent::Statement`, so the line pays its own step as well as the write's
/// cost.
///
/// Pinned as a regression guard, including the part that already works: a write
/// inside a counted loop leaves the loop counted, at `n * (2 + write)`. A change
/// to this lane that started refusing the enclosing statement differently would
/// take the trip count with it, which is precisely what the next test is about.
#[test]
fn a_write_to_an_attribute_or_an_index_is_a_placed_region() {
    for source in [
        "def g(n: int) -> int:\n    for i in range(n):\n        obj.field = 1\n",
        "def g(n: int, a: list) -> int:\n    for i in range(n):\n        a[0] = 1\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        let hole = assert_blames(&result, "complex-assignment-target", source);
        let name = hole.var().symbol().as_str().to_owned();
        assert_eq!(
            value_at(&result, &[("n", 3), (&name, 5)]),
            Nat::Fin(21),
            "the write is one statement inside the loop, so the loop costs \
             n * (2 + write): {} for:\n{source}",
            describe(&result)
        );
        assert_only_supplied(&function, &result, source);
    }
}

/// **`a[i] = v` does not erase the loop that produced `i`.**
///
/// The sharpest failure in this lane, and the one with a measured corpus
/// footprint: 29 of the standard library's 166 `range` loops write through the
/// loop's own counter, and every one of them is refused whole.
///
/// The cause is in `integer_names`. `collect_non_integer_bindings` condemns
/// every name a non-`Name` assignment target mentions, and `target_names` walks
/// the entire target expression - so `a[i] = 0` condemns `i` alongside `a`. `i`
/// is the counter, `for_loop` then finds it is not a proven integer, and the
/// whole `for` statement becomes one `non-integer-value` refusal. The body is
/// never lowered; a nested counted loop inside it disappears from the program
/// and from the ledger.
///
/// Condemning `a` is right - `a` is being written and the analysis cannot see
/// what to. Condemning `i` is not: `i` appears in the *index*, which is a read,
/// and a read of a name proves nothing about it either way. Contrast `a[0] = 0`
/// in the previous test, which leaves the same loop counted at `n * (2 + write)`.
#[test]
fn indexing_with_the_loop_counter_does_not_erase_the_loop() {
    for source in [
        "def g(n: int, a: list) -> int:\n    t = 0\n    for i in range(n):\n        a[i] = 0\n        t = t + 1\n    return t\n",
        "def g(self, n: int) -> int:\n    t = 0\n    for i in range(n):\n        self.x[i] = 0\n        t = t + 1\n    return t\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        let names = mentioned(&result);
        assert!(
            names.iter().any(|var| var == "n"),
            "the counter `i` is only *read* by the index, so it is still the \
             integer the loop bound it to and the loop is still counted by `n`. \
             The bound mentions {names:?} instead: condemning every name in the \
             write target throws away the loop, its body and any loop nested \
             inside it - the same write with a literal index keeps the count. \
             Got {} for:\n{source}",
            describe(&result)
        );
        assert_only_supplied(&function, &result, source);
    }
}

// ---------------------------------------------------------------------------
// the half that must not move
// ---------------------------------------------------------------------------

/// **The lowering still refuses both constructs.**
///
/// The engine deriving a partial bound through a read is a second number, not a
/// change to coverage. `landav_its::Update` is a total map with no havoc, so a
/// transition system admitting `x = obj.k` would assert that `x` is unchanged
/// across it. `Coverage::lowered()` counts transition systems and must not move
/// because the engine's reach did.
///
/// The endpoint and write cases are deliberately absent from this list: what
/// construct *names* their refusal is what
/// [`a_refused_range_endpoint_blames_the_read_rather_than_the_counter`] and
/// [`indexing_with_the_loop_counter_does_not_erase_the_loop`] are asking to
/// change, and pinning it here as well would make this test contradict those.
#[test]
fn a_read_still_stops_the_program_from_lowering() {
    for (source, construct) in [
        (
            "def g(n: int) -> int:\n    x = obj.field\n    return n\n",
            landav_its::Construct::Attribute,
        ),
        (
            "def g(n: int) -> int:\n    obj.field\n    return n\n",
            landav_its::Construct::Attribute,
        ),
        (
            "def g(n: int, a: list) -> int:\n    x = a[0]\n    return n\n",
            landav_its::Construct::Subscript,
        ),
        (
            "def g(n: int, a: list) -> int:\n    for i in range(n):\n        a[i]\n",
            landav_its::Construct::Subscript,
        ),
    ] {
        let function = only_function(source);
        let refused = landav_its::lower(function.program());
        assert!(
            refused.is_err(),
            "a read of something the fragment cannot see into must still stop a \
             program from lowering:\n{source}"
        );
        let constructs: BTreeSet<_> = refused
            .as_ref()
            .err()
            .and_then(landav_its::LoweringError::refusals)
            .map(landav_its::Refusals::constructs)
            .unwrap_or_default()
            .into_iter()
            .collect();
        assert!(
            constructs.contains(&construct),
            "the refusal must name the read as `{construct:?}`, got \
             {constructs:?} for:\n{source}"
        );
    }
}
