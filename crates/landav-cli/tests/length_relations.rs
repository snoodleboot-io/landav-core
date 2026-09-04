//! `LAN-99` acceptance: **a signature row that names an argument.**
//!
//! # The measurement this exists to move, and why it is not the one `LAN-89`
//! predicted
//!
//! `landav` has never produced a stdlib bound that mentions a parameter. Not
//! one, for the whole project. `LAN-89` diagnosed that as missing type
//! information and `LAN-98` measured the diagnosis: fed a complete typeshed,
//! only **10** of 3072 analysed stdlib functions would gain a loop directly over
//! an annotated parameter. Annotation is worth ten functions, because only
//! 13.4% of the corpus's 909 `for` loops iterate a parameter at all.
//!
//! What they iterate instead is a **local** (32.3%) or the **result of a
//! length-preserving call** (~17%):
//!
//! | callee | loops |
//! |---|---|
//! | `.items()` | 57 |
//! | `enumerate(...)` | 44 |
//! | `reversed` / `.values()` / `list` / `sorted` / `splitlines` | 50 |
//!
//! Every one of those returns something whose length is exactly the length of
//! its argument. `sorted(x)` has `len(x)` elements, `enumerate(x)` yields
//! `len(x)` pairs, `d.items()` has `len(d)`. Today each is a `call` the frontend
//! cannot read, so the loop over it is `unbounded-iteration` and the function's
//! bound is a constant.
//!
//! # What is missing is a relation, not another constant
//!
//! `LAN-94` shipped a signature pack whose every field is a **constant property
//! of the callee**: what it costs, whether it rebinds a local, whether it
//! mutates an argument. `LAN-89` shipped `len(items)` as a bound variable and
//! the `#walk` counter that counts a collection walk. Neither can express *the
//! length of my result is the length of my argument 0*, because no field today
//! refers to an argument.
//!
//! # The trap, which is the reason this file is worth writing
//!
//! **The length of a call's result and the cost of the call are two separate
//! claims.** `sorted` is length-preserving and it is `n log n`. A row must be
//! able to declare the length relation while still declaring its cost
//! unbounded, and the call site must still carry its cost hole. Conflate them
//! and `for r in sorted(records)` reports a bound the program exceeds - the
//! failure class with a zero target on this project.
//!
//! That conflation is easy to reach by accident here, and the reason is
//! measured rather than imagined. Today `for r in sorted(expensive(records))`
//! puts **exactly one** record in the lowering's ledger -
//! `("unbounded-iteration", "")` - and neither `sorted` nor `expensive` appears
//! anywhere: [`Translator::for_loop`] refuses the whole statement before the
//! iterable is translated, so the callee exists in no arena and is invisible to
//! the refusal scan and to the walk alike. That is harmless only while the loop
//! is a hole, because a hole denotes `omega` and `omega` dominates whatever the
//! sort costs. **Counting the loop removes the thing that was covering for it.**
//! The tests in section 2 and section 5 are the ones that catch that.
//!
//! # A finding the implementer needs before starting
//!
//! `LAN-99`'s first acceptance criterion is `rows = sorted(records)` followed by
//! `for r in rows`. That shape needs more than a signature row.
//! [`Translator::collections`] is a `BTreeSet` built once from
//! `collection_parameters(function)` and never extended, so a **local** carries
//! no length and `len` of one is not a bound variable. Measured today, the
//! shape reports
//! `Partial(4 + #hole0 + #hole1 + #hole2)` over three refusals -
//! `non-integer-value(rows)`, `call(sorted)`, `unbounded-iteration` - and the
//! first of those is the assignment, not the loop.
//!
//! Making it pass is `LAN-98`'s **step 2**, size propagation through
//! assignment, which that ticket puts in `LAN-11`. It is written here at full
//! strength rather than weakened, and
//! [`a_local_bound_to_a_length_preserving_call_carries_its_length`] carries the
//! note; if the ticket is scoped to the direct form, that is the one test that
//! moves to the next one.
//!
//! # Why these tests read the libraries rather than driving the binary
//!
//! Same reason as `collection_length.rs` and `signature_pack.rs`: the
//! assertions are about the *shape* of a bound - which variables it mentions,
//! whether the loop left a hole, whether the claim is `Theta` or `O` - and the
//! process boundary offers only the rendered string, which these tests are
//! forbidden to pin.
//!
//! [`Translator::for_loop`]: landav_python
//! [`Translator::collections`]: landav_python

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, path::Path};

use landav_bound::{Nat, Symbol, Valuation, VarId};
use landav_engine::{TripCount, cost};
use landav_python::LoweredFunction;

// ---------------------------------------------------------------------------
// harness - lifted from `collection_length.rs` and `signature_pack.rs`, because
// the fences below assert those files' outcomes are unchanged and asserting
// them in weaker words would let a regression through
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
/// Last rather than only, because the shadowing fence needs a module that binds
/// `sorted` above the function under test, and a helper that insisted on one
/// function could not express it.
fn subject(source: &str) -> LoweredFunction {
    let functions = landav_python::lower_module(Path::new("relations.py"), source)
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
/// Two tags, because the frontend has two ways of not counting a `for`:
/// `unbounded-iteration` when it cannot read the iterable at all, and `for`
/// when it built the loop but could not read the endpoint. Either one means the
/// trip count is missing, and a test that checked only one of them would pass
/// against the other.
fn loop_is_counted(result: &TripCount) -> bool {
    !holes_on(result, "unbounded-iteration") && !holes_on(result, "for")
}

/// Every `(construct, callee)` pair the lowering refused, callee where known.
///
/// The lowering's ledger is where a callee's *name* survives; a hole carries
/// only the construct tag, so "which callee" can be asked here and nowhere
/// else. Half the assertions in this file are about a name that must not
/// disappear, so they are all asked here.
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

/// Whether the lowering refused a call to `callee`.
fn refuses_call_to(function: &LoweredFunction, callee: &str) -> bool {
    refusals(function)
        .iter()
        .any(|(construct, detail)| construct == "call" && detail == callee)
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
            ("len(records)", n),
            ("len(items)", n),
            ("len(mapping)", n),
        ])) {
        Nat::Fin(value) => value,
        Nat::Omega => panic!("a bound under test must evaluate finitely at n = {n}"),
    }
}

/// **The** assertion this ticket exists for: the loop over a length-preserving
/// call is counted, and counted by the length of that call's *argument*.
///
/// Deliberately weaker than `collection_length.rs::assert_scales_with`, in one
/// respect and one only: that helper also requires `holes().is_empty()`, and
/// here a surviving hole is *mandatory* for an expensive callee. The length
/// claim and the cost claim are separate, so the length assertion may not
/// smuggle in a completeness requirement. What it does require is that no hole
/// blames the **loop**, because that is the claim being made.
fn assert_counted_by(result: &TripCount, name: &str, source: &str) {
    let shown = describe(result);
    let names = mentioned(result);
    assert!(
        loop_is_counted(result),
        "the loop must be counted: `{name}` is the number of values this \
         iterable yields, so the `for` has a trip count and holing it makes no \
         finite claim about the loop at all. Got {shown} for:\n{source}"
    );
    assert!(
        names.iter().any(|var| var == name),
        "the bound must be a function of `{name}` - a length-preserving call \
         yields exactly as many values as its argument holds, and that is the \
         whole relation this ticket adds - but it mentions {names:?}. Got \
         {shown} for:\n{source}"
    );
}

/// Every name a bound mentions must be something the caller can supply.
///
/// A hole variable is exempt only while the result is **partial**: an unfilled
/// hole denotes `omega`, so a partial makes no finite claim. A *complete*
/// result carrying one would be a finite-looking claim with an infinite term in
/// it, which every caller supplying the parameters and nothing else reads as
/// zero.
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
// sources
// ---------------------------------------------------------------------------

/// `for <target> in <iterable>:` over a `list` parameter named `records`.
fn walking(iterable: &str) -> String {
    format!(
        "\
def g(records: list) -> int:
    total = 0
    for item in {iterable}:
        total = total + 1
    return total
"
    )
}

// ---------------------------------------------------------------------------
// 1 · the relation fires where the argument's length is known
// ---------------------------------------------------------------------------

/// **`for r in sorted(records)` counts as `len(records)`.**
///
/// The headline shape of the ticket, in the form that needs no dataflow: the
/// length-preserving call stands directly in the `for`'s iterable, so
/// everything needed to count the loop is on one line.
///
/// Today: `Partial(3 + #hole0)`, `#hole0` blamed on `unbounded-iteration`. The
/// function has `len(records)` as a parameter and does not mention it.
///
/// This test says nothing about what the sort *costs* - see section 2, which
/// exists precisely because that is a different claim.
#[test]
fn a_walk_over_a_sorted_collection_counts_by_the_arguments_length() {
    let source = walking("sorted(records)");
    let function = subject(&source);
    let result = cost(function.program());

    assert_counted_by(&result, "len(records)", &source);
    assert_only_supplied(&function, &result, &source);
}

/// **`for x in enumerate(records)` counts as `len(records)`.**
///
/// 44 stdlib loops, the second-largest length-preserving shape after
/// `.items()`. `enumerate(x)` yields exactly `len(x)` pairs.
///
/// Unlike `sorted`, constructing an `enumerate` is genuinely constant work - it
/// wraps the iterable and returns - so nothing about this shape forces a
/// surviving hole. That is asserted separately in
/// [`a_nest_of_enumerate_walks_is_quadratic`], where completeness is load
/// bearing; here only the relation is being pinned.
///
/// Today: `Partial(3 + #hole0)`, `#hole0` blamed on `unbounded-iteration`.
#[test]
fn a_walk_over_enumerate_counts_by_the_arguments_length() {
    let source = walking("enumerate(records)");
    let function = subject(&source);
    let result = cost(function.program());

    assert_counted_by(&result, "len(records)", &source);
    assert_only_supplied(&function, &result, &source);
}

/// **`for pair in mapping.items()` counts as `len(mapping)`.**
///
/// 57 stdlib loops, the single largest length-preserving shape in the corpus.
///
/// The target is one name rather than `k, v`, which was originally a way of
/// isolating the length relation from a second construct: when this file was
/// written, `for k, v in mapping.items()` was refused as
/// `complex-assignment-target` before anything about the iterable was consulted,
/// so the idiomatic spelling would have been red for an unrelated reason. That
/// measurement is what widened `LAN-99` to cover tuple targets too.
///
/// Both halves shipped together, so the idiomatic spelling now works and is
/// tested by `tuple_targets::a_tuple_target_over_items_counts_by_the_mappings_length`.
/// This one is kept as the single-name case, which is a real spelling in its own
/// right and exercises the relation without the target machinery.
///
/// This is also the row that needs the pack to match a **bare attribute** on a
/// receiver whose length is known, rather than a bare name. `LAN-94`'s
/// `declaration_for` requires `Expr::Name` and its method rows are documentary;
/// a length relation on `.items()` is the first row that has to bite on a
/// method, and it may bite only when the receiver is a known-length collection.
///
/// Today: `Partial(3 + #hole0)`, `#hole0` blamed on `unbounded-iteration`.
#[test]
fn a_walk_over_items_counts_by_the_mappings_length() {
    let source = "\
def g(mapping: dict) -> int:
    total = 0
    for pair in mapping.items():
        total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());

    assert_counted_by(&result, "len(mapping)", source);
    assert_only_supplied(&function, &result, source);
}

/// **`len(sorted(records))` reads as `len(records)`.**
///
/// The relation has to be readable as a *value*, not only as an iterable, or
/// the `range(len(...))` idiom - the other half of `LAN-89` - gets nothing from
/// it.
///
/// Today: `Partial(2 + #hole0 + #hole1 + #hole2)` with `call(len)`,
/// `call(sorted)` and a `for` hole. Note which hole is which: the loop was
/// *built*, and then holed because its endpoint could not be read. That is why
/// [`loop_is_counted`] checks the `for` tag as well as `unbounded-iteration`.
///
/// The sort still costs what it costs; it happens once, before the loop, and
/// section 2 is what pins that.
#[test]
fn a_length_of_a_length_preserving_call_reads_as_the_arguments_length() {
    let source = "\
def g(records: list) -> int:
    total = 0
    for i in range(len(sorted(records))):
        total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());

    assert_counted_by(&result, "len(records)", source);
    assert_only_supplied(&function, &result, source);
}

/// **The relation composes one level: `enumerate(sorted(records))`.**
///
/// A relation that did not compose would be a lookup table keyed by
/// `(callee, argument-is-a-parameter)`, and the corpus does not oblige: the
/// argument of one length-preserving call is routinely another one.
///
/// One level is the scope. Nothing here claims anything about three.
///
/// Today: `Partial(3 + #hole0)`, `#hole0` blamed on `unbounded-iteration`, and
/// measured, **neither** `enumerate` nor `sorted` appears in the refusal ledger,
/// because the iterable is never translated. Section 5 is where that becomes a
/// soundness problem.
#[test]
fn the_relation_composes_one_level() {
    let source = walking("enumerate(sorted(records))");
    let function = subject(&source);
    let result = cost(function.program());

    assert_counted_by(&result, "len(records)", &source);
    assert_only_supplied(&function, &result, &source);
}

/// **`rows = sorted(records)` then `for r in rows` counts as `len(records)`.**
///
/// `LAN-99`'s first acceptance criterion, and the shape the whole ladder in
/// `LAN-98` exists for: 32.3% of stdlib loops iterate a **local**, and a local's
/// length is a function of whatever produced it.
///
/// # This test depends on work `LAN-98` places in a later step, and it is
/// written at full strength anyway
///
/// [`Translator::collections`] is built once from `collection_parameters` and
/// never extended, so a local is not a collection, `len` of one is not a bound
/// variable, and the assignment itself refuses: measured today this reports
/// `Partial(4 + #hole0 + #hole1 + #hole2)` over
/// `non-integer-value(rows)`, `call(sorted)`, `unbounded-iteration`. A signature
/// row alone moves none of those. What moves them is size propagation through
/// assignment - `LAN-98`'s step 2, which that ticket assigns to `LAN-11`.
///
/// It is stated here rather than softened because the acceptance criterion says
/// this shape, and a weakened test would let the ticket close with the criterion
/// unmet and nothing in the tree recording it. If `LAN-99` is scoped to the
/// direct form, this is the one test in the file that moves to the next ticket -
/// and it moves as written.
///
/// **Deferred rather than weakened.** `Translator::collections` is a set built
/// once from `collection_parameters` and never extended, so a local carries no
/// length at all: the first blocker here is the *assignment*, not the loop, and
/// no signature row moves it. That is `LAN-98` step 2, size propagation through
/// assignment, which `LAN-98` assigns to `LAN-11`. Runnable with `--ignored`, so
/// the shape stays measurable, and it is written at full strength so it passes
/// the moment size flow lands rather than needing to be rewritten then.
#[test]
#[ignore = "needs assignment-level size flow (LAN-98 step 2 / LAN-11): a local \
            carries no length, so the assignment blocks this before the loop does"]
fn a_local_bound_to_a_length_preserving_call_carries_its_length() {
    let source = "\
def g(records: list) -> int:
    rows = sorted(records)
    total = 0
    for r in rows:
        total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());

    assert_counted_by(&result, "len(records)", source);
    assert_only_supplied(&function, &result, source);
}

// ---------------------------------------------------------------------------
// 2 · THE TRAP - the length of the result and the cost of the call are two
//     separate claims, and a row must be able to make one without the other
// ---------------------------------------------------------------------------

/// **A sorted walk is counted *and* still pays for the sort.**
///
/// The single most valuable assertion in this lane.
///
/// `sorted` is length-preserving. `sorted` is `n log n`. Those are two claims
/// about two different things - how many values come out, and how much work it
/// took - and a signature row has to be able to make the first while still
/// making the second say *unbounded*. `builtin_signatures.toml` already carries
/// `sorted` with `cost = "loglinear"` and a `why` explaining that it is two
/// unbounded things at once; that row must survive gaining a length relation,
/// and the call site must survive as a cost hole.
///
/// If the two are conflated - if resolving the length is taken as resolving the
/// call - this function becomes a **complete** bound for a program that sorts,
/// and every stdlib function that sorts inside a loop reports a number the
/// program exceeds. That is the failure class with a zero target on this
/// project.
///
/// # Why the ledger assertion comes first, and why it is red today
///
/// Measured: `for r in sorted(records)` puts exactly one record in the
/// ledger - `("unbounded-iteration", "")` - and `sorted` is **not in it at
/// all**. [`Translator::for_loop`] refuses the whole statement before the
/// iterable is translated, so the call exists in no arena. Today the
/// `unbounded-iteration` hole denotes `omega` and covers for it. Count the loop
/// and that cover is gone, so the name has to start being recorded in the same
/// change that starts counting - not afterwards.
#[test]
fn a_sorted_walk_still_carries_the_cost_of_the_sort() {
    let source = walking("sorted(records)");
    let function = subject(&source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        refuses_call_to(&function, "sorted"),
        "`sorted` is n log n in its argument and the pack declares it so, but \
         the ledger holds {:?} and does not name it - the loop is refused before \
         its iterable is translated, so the sort exists in no arena. That is \
         survivable only while the loop is a hole denoting omega. The length of \
         the result and the cost of the call are two separate claims: declaring \
         the first must not silently discharge the second, or this function \
         reports a bound the program exceeds. For:\n{source}",
        refusals(&function)
    );
    assert!(
        holes_on(&result, "call"),
        "the sort must still stand in the bound as a hole the user can see and \
         `Bound::subst` can fill. A length relation says how many values come \
         out; it says nothing about the work that produced them. Got {shown} \
         for:\n{source}"
    );
    assert!(
        !result.is_complete(),
        "a function that sorts an input of unknown size makes no complete claim: \
         the trip count is known, the sort is not, and reporting the pair as a \
         finished number is the conflation this test exists for. Got {shown} \
         for:\n{source}"
    );
}

/// **A sort in a loop body is still unbounded work, once per iteration.**
///
/// The concrete program the trap ruins. `for i in range(n): for r in
/// sorted(records)` runs `n` sorts, and a bound that charged the sort nothing
/// would report `n * len(records)` for work that is `n * len(records) * log
/// len(records)` - exceeded by the program at every input.
///
/// Written as a second test rather than folded into the one above because a
/// change that discharges the cost only at statement level would pass one and
/// fail the other, and two tests say which half landed where one would not.
///
/// Today: `Partial(2 + n * (2 + #hole0))`, `#hole0` blamed on
/// `unbounded-iteration`, and `sorted` again absent from the ledger.
#[test]
fn a_sort_inside_a_loop_is_still_unbounded_work() {
    let source = "\
def g(records: list, n: int) -> int:
    total = 0
    for i in range(n):
        for r in sorted(records):
            total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        refuses_call_to(&function, "sorted"),
        "the sort runs once per outer iteration and its cost is n log n in an \
         input size this analysis cannot see, so it must be named where it \
         stands. The ledger holds {:?}. For:\n{source}",
        refusals(&function)
    );
    assert!(
        !result.is_complete(),
        "n sorts of an unknown-sized input is not a finished number. A complete \
         bound here is the exact shape the ticket warns about: a sort inside a \
         loop reporting a bound the program exceeds. Got {shown} for:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// 3 · what must stay weakened or refused
// ---------------------------------------------------------------------------

/// **`set(records)` never claims an exact length.**
///
/// `set` is not length-preserving: `{1, 1, 2}` from a three-element input has
/// two elements. Its result's length is an **upper** bound, so the loop over it
/// is `O(len(records))` and never `Theta`.
///
/// This is the same distinction `LAN-91` drew for a set display, and the
/// machinery for it already exists: `walk_display` builds a non-exact endpoint
/// with `unsupported_expr_bounded`, and the engine reads that as an upper bound
/// and reports `O`. A length relation that admits `set` has to reach for the
/// same constructor rather than the equality one.
///
/// # This test passes today, and is a fence rather than a driver
///
/// Today `set(records)` is `unbounded-iteration`, which is `Partial` and
/// therefore not `Exact` - a refusal, which the ticket explicitly allows as one
/// of the two honest answers. The assertion is written so that it stays valid
/// under either choice and fails only under the third, wrong one: an equality.
/// `Theta` against `O` is a distinction this codebase reports to its users, so
/// a false `Theta` is a lie in the report and not merely a rounding.
#[test]
fn a_set_never_claims_an_exact_length() {
    let source = walking("set(records)");
    let function = subject(&source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        !result.is_exact(),
        "`set(records)` collapses equal elements, so the loop runs *at most* \
         `len(records)` times and possibly fewer. An exact claim here is a false \
         `Theta` - the same error `LAN-91` refused for the `{{1, 1, 2}}` display \
         - and `Theta` against `O` is a difference this tool prints. Pin `O` or \
         pin the refusal; never the equality. Got {shown} for:\n{source}"
    );
    assert_only_supplied(&function, &result, &source);
}

/// **A generator argument confers no length.**
///
/// The relation is conditional on the **argument**, not a property of the
/// callee's name. `enumerate(x)` yields `len(x)` pairs only when `x` has a
/// length to yield; a generator does not, and a filtered comprehension does not
/// even in principle - `(r for r in records if keep(r))` yields between 0 and
/// `len(records)` values and nothing here can say which.
///
/// Two shapes, because they fail for different reasons and an implementation
/// could get one right and the other wrong: a comprehension, which the frontend
/// can see and must decline, and an unknown callee's result, which it cannot
/// see at all.
///
/// # A fence, passing today
///
/// Both are `unbounded-iteration` now. The assertion is that they stay
/// non-exact and incomplete: an `O(len(records))` answer for the comprehension
/// would actually be *sound*, so the fence is pitched exactly where unsoundness
/// starts rather than where convenience ends.
#[test]
fn a_generator_argument_confers_no_length() {
    for iterable in [
        "enumerate(r for r in records if keep(r))",
        "enumerate(stream())",
    ] {
        let source = walking(iterable);
        let function = subject(&source);
        let result = cost(function.program());
        let shown = describe(&result);

        assert!(
            !result.is_exact(),
            "`{iterable}` has no length: a generator is consumed rather than \
             measured, and a filtered one yields fewer values than it reads. A \
             length relation keyed on the callee's name alone would fire here \
             and publish an equality for a count nothing knows. Got {shown} \
             for:\n{source}"
        );
        assert!(
            !result.is_complete(),
            "the loop's trip count is unknown and the generator's own per-item \
             work is unknown, so this function makes no complete claim: {shown} \
             for:\n{source}"
        );
    }
}

/// **`.items()` on something without a known length stays a hole.**
///
/// The matched fence to [`a_walk_over_items_counts_by_the_mappings_length`].
/// `d` is built by a call this analysis cannot read, so `len(d)` is not a value
/// the caller supplies and `d.items()` has no length either. A row keyed on the
/// attribute name alone would fire on every `.items()` in the corpus, including
/// this one.
///
/// A fence, passing today: `d.items()` is `unbounded-iteration`. It is here
/// because the change that makes the sibling test pass is exactly the change
/// that could break this one.
#[test]
fn items_on_something_without_a_known_length_stays_a_hole() {
    let source = "\
def g(n: int) -> int:
    d = build(n)
    total = 0
    for pair in d.items():
        total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        !loop_is_counted(&result),
        "`d` was built by an unknown call, so its length is not a value the \
         caller supplies and `d.items()` has no length to preserve. The relation \
         is conditional on the argument, never a property of the name. Got \
         {shown} for:\n{source}"
    );
    assert!(
        !result.is_complete(),
        "nothing is known about `build`, so this function makes no complete \
         claim: {shown} for:\n{source}"
    );
    assert_only_supplied(&function, &result, source);
}

/// **A callee that takes a function is not length-preserving.**
///
/// `filter(pred, records)` yields *at most* `len(records)` values - `pred`
/// decides - and `map(fn, records)` yields exactly that many but runs `fn` on
/// every one of them, which is arbitrary user code with an arbitrary cost. Both
/// are the same mistake in the same place: reading "walks its argument once" as
/// "is free and preserves length".
///
/// A fence, passing today, and pitched at completeness rather than at counting:
/// an implementation that counts `map`'s loop by `len(records)` **and** keeps a
/// hole for `fn` would be sound, and this test lets it through. What it does not
/// let through is a finished number for a program that calls something unknown
/// `len(records)` times.
#[test]
fn a_callee_taking_a_function_is_not_length_preserving() {
    for iterable in ["filter(pred, records)", "map(fn, records)"] {
        let source = walking(iterable);
        let function = subject(&source);
        let result = cost(function.program());
        let shown = describe(&result);

        assert!(
            !result.is_complete(),
            "`{iterable}` calls user code once per element, and its cost is \
             whatever that code costs. `filter` additionally yields fewer values \
             than it reads, so it does not preserve a length at all. A complete \
             bound here charges nothing for a call the program makes \
             `len(records)` times. Got {shown} for:\n{source}"
        );
    }
}

/// **An unknown callee is unchanged: the pack is an allowlist.**
///
/// The fence that stops the prize being won by giving every call a length. The
/// matched pair to
/// `signature_pack.rs::an_unknown_callee_above_a_loop_still_erases_it`, in the
/// new vocabulary. `mystery` is user code: it may return three values or none,
/// and this analysis knows nothing about it.
///
/// A fence, passing today, and the bound-mentions check is the sharp half: a
/// bound over `len(records)` here would be a claim about a collection the loop
/// may never touch.
#[test]
fn an_unknown_callee_confers_no_length() {
    let source = walking("mystery(records)");
    let function = subject(&source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        !loop_is_counted(&result),
        "`mystery` is not in the pack and nothing is known about it - it may \
         return one value or a million. The pack is an allowlist, never a \
         default. Got {shown} for:\n{source}"
    );
    assert!(
        !mentioned(&result).iter().any(|name| name == "len(records)"),
        "the bound must not read `len(records)` through a callee that was never \
         declared to preserve it: passing a collection to something is not \
         evidence about what comes back. Got {shown} for:\n{source}"
    );
    assert!(
        !result.is_complete(),
        "a function containing an unknown callee makes no complete claim: \
         {shown} for:\n{source}"
    );
}

/// **A shadowed name gets no relation.**
///
/// `LAN-94`'s shadowing discipline, which this ticket must not disturb. A
/// module that writes `def sorted(x)` is not calling the builtin, and a pack
/// keyed by name would otherwise resolve a call to something it knows nothing
/// about. `SignaturePack::signature` already takes the bound-name set and
/// `crates/landav-python/tests/signature_shadowing.rs` already pins it; this is
/// the same rule asked in the new vocabulary, because a length relation is a
/// second thing a row can hand out and both have to go through the same gate.
///
/// A fence, passing today.
#[test]
fn a_shadowed_length_preserving_name_gets_no_relation() {
    let source = "\
def sorted(x):
    return x

def g(records: list) -> int:
    total = 0
    for r in sorted(records):
        total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        !loop_is_counted(&result),
        "this module binds `sorted` itself, so the call is to that function and \
         not to the builtin the pack describes. A length relation handed out on \
         the strength of a name would be a claim about code the pack has never \
         seen. Got {shown} for:\n{source}"
    );
    assert!(
        !mentioned(&result).iter().any(|name| name == "len(records)"),
        "the bound must not be a function of `len(records)` through a `sorted` \
         this module defined: {shown} for:\n{source}"
    );
}

// A fence stood here: `a_tuple_target_is_refused_by_something_this_ticket_does_not_fix`.
//
// It recorded that `for k, v in mapping.items()` - the idiomatic spelling, and
// what all 57 stdlib `.items()` loops look like - was refused as
// `complex-assignment-target` before anything about the iterable was read, so
// only the single-name spelling was reachable by a length relation. That
// measurement is what widened `LAN-99` to cover tuple targets as well, since
// neither half paid on its own.
//
// Its own doc named the condition for removing it: it fails the moment tuple
// unpacking is implemented, and that is the right moment to delete it. That
// moment is this commit. The idiomatic spelling is now tested directly by
// `tuple_targets::a_tuple_target_over_items_counts_by_the_mappings_length`,
// which is green - so the knowledge is kept and the assertion is not duplicated
// in its negated form.

// ---------------------------------------------------------------------------
// 4 · `LAN-89` regressions - the machinery this ticket reuses must not move
// ---------------------------------------------------------------------------

/// **A plain walk over a collection parameter still counts.**
///
/// `LAN-89`'s headline, and the only shape in the corpus that produces a
/// non-constant bound today. A length relation is built on the same
/// `len(items)` variable and the same `#walk` counter, so a change that
/// generalises them can break them. This function lowers today and is counted in
/// the headline coverage figure; that number may not fall because a relation was
/// added beside it.
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
         before the loop begins - so this is `Theta` and not `O`. A relation \
         that weakened it would make every existing bound one-sided: {shown} \
         for:\n{source}"
    );
    assert!(
        landav_its::lower(function.program()).is_ok(),
        "this function lowers today and must keep lowering - it is counted in \
         the headline coverage figure. The ledger holds {:?} for:\n{source}",
        refusals(&function)
    );
}

/// **`len` of a local still holes.**
///
/// The other half of `LAN-89`'s narrow reading, re-asserted because a relation
/// is the obvious way to widen it too far. `len(made)` where `made` came from an
/// unknown call is not a value the caller supplies and is not a constant, and no
/// row about `len` may make it one: `len` is deliberately absent from
/// `builtin_signatures.toml`, with the reason written there.
///
/// A regression guard, passing today.
#[test]
fn a_length_of_a_local_from_an_unknown_call_still_holes() {
    let source = "\
def g(n: int) -> int:
    made = build(n)
    return len(made)
";
    let function = subject(source);
    let result = cost(function.program());

    assert!(
        !result.is_complete(),
        "`len` of something the caller did not supply is neither a constant nor \
         a variable, and a length *relation* is about a call's result, not about \
         a name the analysis never saw a value for: {} for:\n{source}",
        describe(&result)
    );
    assert!(
        refuses_call_to(&function, "len") && refuses_call_to(&function, "build"),
        "both calls must still be refused by name, or `LAN-89`'s narrow reading \
         has quietly widened. The ledger holds {:?} for:\n{source}",
        refusals(&function)
    );
}

// ---------------------------------------------------------------------------
// 5 · soundness - the bound may never fall below the truth
// ---------------------------------------------------------------------------

/// **A length relation does not swallow a call in its own argument.**
///
/// The specific way this ticket goes wrong, and it is the same shape
/// `signature_pack.rs::a_pack_entry_does_not_swallow_an_unknown_call_in_its_arguments`
/// found for `isinstance(x, lookup(n))` - one level further out, and worse,
/// because there is no ledger entry at all to start from.
///
/// Measured: `for r in sorted(expensive(records))` puts exactly one record in
/// the ledger, `("unbounded-iteration", "")`. Neither `sorted` nor `expensive`
/// is anywhere. The `for` is refused before its iterable is translated, so both
/// calls exist in no arena, and the hole standing there is the **loop's**.
///
/// While that hole denotes `omega` nothing is lost. **Counting the loop takes
/// the cover away**, and a function that runs `expensive(records)` reports a
/// bound with `expensive` missing from it - a number below the truth, published
/// as an analysed result. The fix is not in the pack: the iterable must be
/// translated before its callee may be resolved, exactly as `LAN-94` concluded
/// for an argument list.
///
/// Asserted on the ledger because that is where a callee's name survives.
#[test]
fn a_length_relation_does_not_swallow_a_call_in_its_argument() {
    let source = walking("sorted(expensive(records))");
    let function = subject(&source);
    let result = cost(function.program());

    assert!(
        refuses_call_to(&function, "expensive"),
        "`expensive(records)` is an unknown call standing inside a \
         length-preserving one, and it runs. The ledger holds {:?} - it names \
         neither call, because the loop is refused before its iterable is \
         translated - so nothing is accounting for `expensive`, and counting \
         this loop would produce a complete-looking bound that omits it. This is \
         the direction that must never happen: a bound below the truth. \
         For:\n{source}",
        refusals(&function)
    );
    assert!(
        !result.is_complete(),
        "while the iterable holds an unknown call this function makes no \
         complete claim: {} for:\n{source}",
        describe(&result)
    );
}

/// **A nest of `enumerate` walks is quadratic, and complete.**
///
/// The direction assertion the file is required to make explicitly. A wrong
/// length relation under-reports here in the way that matters: if the inner
/// walk were counted once instead of `len(records)` times - or counted as a
/// constant - the answer would be *linear* for a program that is quadratic, and
/// linear is exceeded by the program at every input above a handful.
///
/// Stated as growth rather than as a literal, following
/// `collection_length.rs::a_nested_walk_is_quadratic_in_the_length`, so it
/// survives a change to what a statement costs.
///
/// # Completeness is load bearing here, and it is a claim
///
/// `enumerate(x)` wraps its argument and returns; constructing one is constant
/// work in this analysis's variables, which is the same claim
/// `builtin_signatures.toml` already makes for `isinstance` and friends. So
/// unlike `sorted`, nothing in this function is unknown once the relation
/// exists, and the result must be a finished number rather than a partial one.
/// If `enumerate` is admitted with a length relation but *not* a constant cost,
/// this test fails and that is the conversation to have - written down here
/// rather than discovered from a coverage figure.
///
/// The growth check is guarded by the completeness check because [`at`] reads a
/// hole as zero, and a growth ratio computed over an unfilled hole is exactly
/// the under-report this section polices.
#[test]
fn a_nest_of_enumerate_walks_is_quadratic() {
    let source = "\
def g(records: list) -> int:
    total = 0
    for a in enumerate(records):
        for b in enumerate(records):
            total = total + 1
    return total
";
    let function = subject(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert_counted_by(&result, "len(records)", source);
    assert!(
        result.is_complete(),
        "constructing an `enumerate` is constant work - it wraps its argument \
         and returns - so both loops are counted and nothing in this function is \
         unknown. A partial result here means the relation landed without the \
         cost claim beside it. Got {shown} for:\n{source}"
    );
    let (small, large) = (at(&result, 10), at(&result, 20));
    assert!(
        large >= small * 3,
        "doubling the collection must more than double the cost of a nested \
         walk - {small} at `len(records) = 10` and {large} at 20 is not \
         quadratic growth, so the inner walk is not being counted once per outer \
         iteration. A linear bound here is exceeded by the program, which is the \
         one direction that must never happen. Got {shown} for:\n{source}"
    );
    assert_only_supplied(&function, &result, source);
}

/// **A length-preserving walk costs at least the walk it stands in for.**
///
/// `for x in enumerate(records)` and `for x in records` run the same number of
/// times. `LAN-89` already derives the second exactly, so the first has a
/// baseline to be checked against, and the check is one-sided on purpose: the
/// enumerate form does strictly more work per iteration - it builds a pair - so
/// it may cost more, and it may never cost less.
///
/// A relation applied to the wrong argument position, or a trip count taken from
/// something other than the argument's length, shows up here as a number below
/// the baseline. Comparing against a derived baseline rather than a literal
/// means the test survives any change to what a statement costs, and states the
/// property that actually matters.
///
/// The baseline's own completeness is asserted first, because a comparison
/// against a partial bound whose holes read as zero would be meaningless in
/// precisely the direction being tested.
#[test]
fn a_length_preserving_walk_costs_at_least_the_walk_it_replaces() {
    let source = walking("enumerate(records)");
    let function = subject(&source);
    let result = cost(function.program());
    let shown = describe(&result);

    let baseline_source = walking("records");
    let baseline = cost(subject(&baseline_source).program());
    assert!(
        baseline.is_complete(),
        "the plain-walk baseline must be complete or this comparison means \
         nothing: {}",
        describe(&baseline)
    );
    assert!(
        result.is_complete(),
        "`enumerate(records)` yields `len(records)` pairs and building one is \
         constant work, so this function is a finished number and can be \
         compared against the walk it stands in for. Got {shown} \
         for:\n{source}"
    );

    for n in [10_u64, 20] {
        let enumerated = at(&result, n);
        let plain = at(&baseline, n);
        assert!(
            enumerated >= plain,
            "`for x in enumerate(records)` runs exactly as many times as \
             `for x in records` and does strictly more work each time, so it may \
             never report less: {enumerated} against {plain} at \
             `len(records) = {n}`. A number below the baseline means the trip \
             count came from somewhere other than the argument's length, and it \
             is a bound the program exceeds. Got {shown} against {} \
             for:\n{source}",
            describe(&baseline)
        );
    }
}
