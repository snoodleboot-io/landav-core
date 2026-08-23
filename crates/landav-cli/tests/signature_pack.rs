//! `LAN-94` acceptance: **a signature pack for the bounded-cost builtins the
//! corpus actually calls.**
//!
//! # The measurement this exists to move, and the one it does not
//!
//! `call` is the dominant sole blocker: 551 stdlib functions are blocked by it
//! and nothing else. But the callee distribution *inside* those functions is a
//! long tail — 312 distinct callees over 741 stdlib sites, 749 over 3757 typed
//! sites — and this file is written so that nobody can read it as a claim that
//! a small pack closes `call`. It does not. The head of the tail is type tests,
//! the head is what a pack can buy, and the rest is `LAN-87`'s slice 2
//! (intra-repository calls) and `LAN-11` (size analysis).
//!
//! # The two properties that admit a callee, and the second is the prize
//!
//! 1. **Bounded cost.** `isinstance(x, T)` walks an MRO whose depth is a
//!    property of the program's class hierarchy, not of any input. It is O(1)
//!    in the analysis's variables. `join` is linear in its argument and
//!    `sorted` is `n log n`; those are not bounded, they are `LAN-11`.
//!
//! 2. **Cannot rebind a local**, and this is where the coverage is. Today
//!    `Walk::region` clears `readable` at *every* region, because a region may
//!    assign to anything. So this program:
//!
//!    ```python
//!    def g(x, n: int) -> int:
//!        if isinstance(x, int):
//!            pass
//!        for i in range(n):
//!            pass
//!        return 0
//!    ```
//!
//!    reports `Partial(2 + #hole0 + #hole1)` — the guard is a hole, *and the
//!    loop below it is a second hole*, because `n` stopped being readable at
//!    the guard. The identical program with `if n > 0:` reports `Theta(2 + n)`.
//!    A type test above a loop currently costs that loop its entire trip count.
//!
//!    The argument for why it need not is already written in this codebase, for
//!    two other constructs: [`landav_its::Construct::may_rebind_locals`] returns
//!    `false` for `Attribute` and `Subscript` because a `property` getter and a
//!    `__getitem__` run in *their own* frame, and no Python expression rebinds a
//!    local of the frame it is evaluated in except the walrus. `isinstance` runs
//!    in its own frame too. This is that argument's natural extension, and it is
//!    the entire reason a signature must declare rebinding as well as cost.
//!
//! # Why these tests read the libraries rather than driving the binary
//!
//! Same reason as `calls_become_holes.rs` and `collection_length.rs`: the
//! assertions are about the *shape* of a bound — which variables it mentions,
//! whether the loop below the guard left a hole — and the process boundary
//! offers only the rendered string, which these tests are forbidden to pin.
//! This is the only package depending on both the frontend and the engine, so
//! the file adds no dependency edge.
//!
//! # What this file deliberately does not touch
//!
//! `calls_become_holes.rs::a_call_still_stops_the_program_from_lowering` pins
//! `f(n)`, `g(n)` and friends — *unknown* callees — as still refusing to lower.
//! Nothing here contradicts it: the pack is an allowlist keyed by callee name,
//! and every callee in that test is outside it. If a change makes both files
//! pass only by making the pack a default, the fence tests below fail.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, path::Path};

use landav_bound::{Nat, Symbol, Valuation, VarId};
use landav_engine::{TripCount, cost};
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
    let mut functions = landav_python::lower_module(Path::new("pack.py"), source)
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

/// Every `(construct, callee)` pair the lowering refused, callee where known.
///
/// The lowering's ledger is where a callee's *name* survives; a [`Hole`] carries
/// only the construct tag, so "which callee" can be asked here and nowhere else.
///
/// [`Hole`]: landav_engine::Hole
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

/// The bound's value with `n` bound and everything else zero.
///
/// Zero is the right default *only* for a complete result; a hole variable read
/// as zero is exactly the under-report these tests police, so every caller
/// checks completeness first.
fn at(result: &TripCount, n: u64) -> u64 {
    match result
        .bound()
        .expect("a result under test carries a bound")
        .eval(&bindings(&[("n", n), ("len(items)", n)]))
    {
        Nat::Fin(value) => value,
        Nat::Omega => panic!("a bound under test must evaluate finitely at n = {n}"),
    }
}

/// The result must name the region, place it, and put its variable in the bound.
///
/// Lifted from `calls_become_holes.rs`, because the fence tests below are
/// asserting that file's outcome is *unchanged* for callees outside the pack,
/// and asserting it in weaker words would let a regression through.
fn assert_blames_a_call(result: &TripCount, source: &str) {
    let shown = describe(result);
    assert!(
        matches!(result, TripCount::Partial { .. }),
        "a call outside the pack is not derivable and must still say so with a \
         hole, got {shown} for:\n{source}"
    );
    let hole = result
        .holes()
        .iter()
        .find(|hole| hole.construct() == "call")
        .unwrap_or_else(|| {
            panic!(
                "the region must still be blamed on the call by name - `call` is \
                 what the user can act on - got {shown} for:\n{source}"
            )
        });
    assert!(
        hole.origin().as_str().contains(':'),
        "a hole must be placed as well as named, got {} for:\n{source}",
        hole.origin()
    );
    let bound = result.bound().expect("a partial result carries a bound");
    assert!(
        bound.vars().contains(&hole.var()),
        "the hole {} does not occur in {bound}, so the bound reads as a complete \
         cost with a footnote and `Bound::subst` has nothing to fill for:\n{source}",
        hole.var().symbol()
    );
}

// ---------------------------------------------------------------------------
// sources shared between a driver and its baseline
// ---------------------------------------------------------------------------

/// `if <guard>: pass` above `for i in range(n): pass`. The prize's shape.
fn guard_above_loop(guard: &str) -> String {
    format!(
        "\
def g(x, n: int) -> int:
    if {guard}:
        pass
    for i in range(n):
        pass
    return 0
"
    )
}

/// `for i in range(n): <body>`. The per-iteration shape.
fn loop_over(body: &str) -> String {
    format!(
        "\
def g(x, n: int) -> int:
    for i in range(n):
        {body}
    return 0
"
    )
}

// ---------------------------------------------------------------------------
// 1 · a pack entry costs a bounded amount rather than holing
// ---------------------------------------------------------------------------

/// **`isinstance(x, T)` in a statement is charged, not holed.**
///
/// Today: `Partial(1 + #hole0)`, `#hole0` blamed on `call`. The statement's own
/// step is charged and the callee is an unknown, so the function makes no finite
/// claim at all.
///
/// The assertion is deliberately *not* a literal total. What the signature
/// declares `isinstance` to cost is the implementer's to choose; what it may not
/// do is leave a variable in the bound. A bound over no variables **is** the
/// bounded-cost claim, and it is the whole first property in the ticket.
///
/// The lower fence — at least one step — is the composability argument from
/// `calls_become_holes.rs`: one statement executed is one step in this engine's
/// declared unit, whatever the callee turns out to cost.
#[test]
fn a_type_test_in_a_statement_costs_a_bounded_amount() {
    for (callee, source) in bounded_pack_statements() {
        let function = only_function(&source);
        let result = cost(function.program());
        let shown = describe(&result);

        assert!(
            result.is_complete(),
            "`{callee}` has a cost bounded by the program's class hierarchy, not \
             by any input, so a statement calling it makes a finite claim and \
             must not be holed. Got {shown} for:\n{source}"
        );
        assert!(
            mentioned(&result).is_empty(),
            "the cost of `{callee}` is O(1) in the analysis's variables, so its \
             bound may mention none of them - a bound that grew with an input \
             would mean the signature declared the wrong thing. Got {shown} \
             for:\n{source}"
        );
        assert!(
            at(&result, 0) >= 1,
            "the statement executes, so it costs at least the one step this \
             engine charges a statement - charging zero is the same confident \
             wrong answer `LAN-87` removed. Got {shown} for:\n{source}"
        );
    }
}

/// The callees whose bounded cost this ticket admits, each as a bare statement.
///
/// `getattr` is here in its two-argument form only. Nothing about a default or a
/// `__getattr__` is claimed beyond what `Construct::Attribute` already claims.
fn bounded_pack_statements() -> Vec<(&'static str, String)> {
    [
        ("isinstance", "isinstance(x, int)"),
        ("hasattr", "hasattr(x, \"field\")"),
        ("getattr", "getattr(x, \"field\")"),
        ("bool", "bool(x)"),
        ("callable", "callable(x)"),
        ("issubclass", "issubclass(x, int)"),
    ]
    .into_iter()
    .map(|(callee, call)| {
        (
            callee,
            format!("def g(x, n: int) -> int:\n    {call}\n    return 0\n"),
        )
    })
    .collect()
}

// ---------------------------------------------------------------------------
// 2 · THE PRIZE — a guard above a loop no longer erases the loop
// ---------------------------------------------------------------------------

/// **A type test above a loop does not cost that loop its trip count.**
///
/// This is the assertion the ticket is worth writing for, and the one a
/// cost-only signature does not satisfy.
///
/// | program | today | required |
/// |---|---|---|
/// | `if n > 0: pass` then `for i in range(n)` | `Theta(2 + n)` | unchanged |
/// | `if isinstance(x, int): pass` then the same loop | `Partial(2 + #hole0 + #hole1)` | mentions `n`, no `for` hole |
///
/// The second hole is the loop. `Walk::region` clears `readable` at the guard —
/// every value the engine knew is forgotten — so `n` is no longer available to
/// count the loop with, and a function whose only unknown is a type test reports
/// nothing about its own `for`.
///
/// A signature declaring only a *cost* fills `#hole0` and leaves `#hole1`
/// standing. It takes the second declaration — that this callee cannot rebind a
/// local of the caller's frame, so there is nothing to forget — to recover the
/// loop. That is why the ticket asks for two fields and not one.
///
/// The final assertion compares against the constant-guard baseline rather than
/// pinning a total, so it survives any choice of what `isinstance` costs: the
/// difference between the two programs must be a *constant*, identical at
/// `n = 10` and `n = 20`. A difference that grew with `n` would mean the guard
/// is still being charged inside, or instead of, the loop.
#[test]
fn a_type_test_guard_does_not_erase_the_loop_below_it() {
    let source = guard_above_loop("isinstance(x, int)");
    let function = only_function(&source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        !holes_on(&result, "for"),
        "the `for` below the guard is counted by `n`, and it is only holed \
         because the guard cleared every readable value. `isinstance` runs in \
         its own frame and cannot rebind a local of this one - the same argument \
         `Construct::may_rebind_locals` already makes for `x.y` and `x[i]` - so \
         there is nothing for the guard to forget. Got {shown} for:\n{source}"
    );
    assert!(
        mentioned(&result).iter().any(|name| name == "n"),
        "the bound must be a function of `n`: the loop runs `n` times whatever \
         the guard decided, and a bound that does not mention `n` is not a scale \
         answer for this function. Got {shown} for:\n{source}"
    );
    assert!(
        result.is_complete(),
        "nothing in this function is unknown once `isinstance` has a signature - \
         the guard is bounded and the loop is counted - so the result must be a \
         complete claim rather than a partial one. Got {shown} for:\n{source}"
    );

    let baseline = only_function(&guard_above_loop("n > 0"));
    let baseline = cost(baseline.program());
    assert!(
        baseline.is_complete(),
        "the constant-guard baseline must be complete or this comparison means \
         nothing: {}",
        describe(&baseline)
    );
    let delta_10 = at(&result, 10) - at(&baseline, 10);
    let delta_20 = at(&result, 20) - at(&baseline, 20);
    assert_eq!(
        delta_10, delta_20,
        "replacing `n > 0` with `isinstance(x, int)` must cost a constant, and \
         it cost {delta_10} at n = 10 and {delta_20} at n = 20. A difference \
         that grows with `n` means the guard's cost was folded into the loop \
         rather than charged once above it. Got {shown} for:\n{source}"
    );
}

/// **The same, one level deeper: a guard above a nest keeps the nest quadratic.**
///
/// `for i in range(n): for j in range(n): isinstance(x, int)` reports
/// `Partial(1 + n * (1 + #hole0))` today, `#hole0` being the *inner* `for`: the
/// type test in the innermost body cleared `readable` on the first iteration, so
/// the inner loop lost its own endpoint.
///
/// Stated as growth rather than as a literal, following
/// `collection_length.rs::a_nested_walk_is_quadratic_in_the_length`. A linear
/// answer here would be *exceeded by the program*, which is the direction that
/// must never happen.
#[test]
fn a_type_test_inside_a_nest_keeps_the_nest_quadratic() {
    let source = "\
def g(x, n: int) -> int:
    for i in range(n):
        for j in range(n):
            isinstance(x, int)
    return 0
";
    let function = only_function(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        result.is_complete(),
        "both loops are counted by `n` and the body is a bounded type test, so \
         this nest has a complete bound: {shown} for:\n{source}"
    );
    let (small, large) = (at(&result, 10), at(&result, 20));
    assert!(
        large >= small * 3,
        "doubling `n` must more than double the cost of a nested loop - {small} \
         at n = 10 and {large} at n = 20 is not quadratic growth, so the inner \
         loop is not being counted `n` times. A linear bound here is exceeded by \
         the program. Got {shown} for:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// 3 · the fence — the pack is an allowlist, never a default
// ---------------------------------------------------------------------------

/// **An unknown callee above a loop still erases it.**
///
/// The matched pair to `a_type_test_guard_does_not_erase_the_loop_below_it`, and
/// the reason that test cannot be satisfied by relaxing `region` for calls in
/// general. `foo` is user code: it may hold a reference to this frame's
/// namespace, it may be a closure, and this analysis knows nothing about it. The
/// loop below it must stay a hole and the bound must not mention `n`.
///
/// A change that makes the prize test pass by dropping the `readable.clear()` at
/// every call region fails here, which is the entire purpose of writing both.
#[test]
fn an_unknown_callee_above_a_loop_still_erases_it() {
    let source = guard_above_loop("foo(x)");
    let function = only_function(&source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        holes_on(&result, "for"),
        "`foo` is not in the pack and nothing is known about it, so the loop \
         below it may not be counted by a value `foo` could have changed. The \
         pack is an allowlist, never a default. Got {shown} for:\n{source}"
    );
    assert!(
        !result.is_complete(),
        "a function containing an unknown callee makes no complete claim: \
         {shown} for:\n{source}"
    );
    assert!(
        !mentioned(&result).iter().any(|name| name == "n"),
        "the bound must not read `n` across an unknown call - that is precisely \
         the over-claim `Walk::region` clears `readable` to prevent. Got {shown} \
         for:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// 4 · an unknown callee is still a named, placed hole
// ---------------------------------------------------------------------------

/// **Nothing about the pack changes what an unknown call reports.**
///
/// `LAN-87`'s outcome, re-asserted here because a pack is a filter placed in
/// front of exactly this path and a filter that swallows its miss case is the
/// obvious way to break it. The hole must still be named `call`, still be
/// placed, and still occur in the bound so `Bound::subst` has something to fill.
#[test]
fn an_unknown_callee_is_still_a_named_and_placed_hole() {
    for source in [
        "def g(n: int) -> int:\n    foo(n)\n    return 0\n",
        "def g(n: int) -> int:\n    return foo(n)\n",
        "def g(x, n: int) -> int:\n    x.method(n)\n    return 0\n",
    ] {
        let function = only_function(source);
        assert_blames_a_call(&cost(function.program()), source);
    }
}

// ---------------------------------------------------------------------------
// 5 · exception constructors
// ---------------------------------------------------------------------------

/// **`raise TypeError("x")` costs its argument evaluation plus a step.**
///
/// After `LAN-92` a `raise` is a real statement — one step, plus an edge out of
/// the region it stands in — and `raise TypeError` with a bare class name
/// already reports `Theta(1)`, because a global lookup runs no user code.
/// Adding parentheses turns it into `Partial(1 + #hole0)`: constructing the
/// exception became an unknown call.
///
/// Constructing an exception is bounded. The assertion is stated as an equality
/// with the bare-class form rather than as a literal, which says the thing that
/// actually matters — the parentheses cost nothing beyond evaluating what is
/// inside them, and here that is a string constant.
#[test]
fn a_raised_exception_constructor_costs_a_step() {
    for callee in ["TypeError", "ValueError", "NotImplementedError"] {
        let called = format!("def g(n: int) -> int:\n    raise {callee}(\"x\")\n");
        let bare = format!("def g(n: int) -> int:\n    raise {callee}\n");

        let function = only_function(&called);
        let result = cost(function.program());
        let shown = describe(&result);
        assert!(
            result.is_complete(),
            "constructing `{callee}` is bounded work and the `raise` is one \
             step, so this function has a complete bound. Got {shown} \
             for:\n{called}"
        );

        let baseline = cost(only_function(&bare).program());
        assert_eq!(
            at(&result, 0),
            at(&baseline, 0),
            "`raise {callee}(\"x\")` and `raise {callee}` differ only by \
             constructing the exception from a string constant, so they cost the \
             same. Got {shown} against {} for:\n{called}",
            describe(&baseline)
        );
    }
}

/// **An exception constructor does not swallow a call in its argument.**
///
/// `raise ValueError(explain(n))` reports `Partial(1 + #hole0)` today with
/// `call(ValueError)` the only refusal in the ledger — `explain` is never
/// translated at all, so the hole standing there is the *constructor's*, not the
/// argument's. Give `ValueError` a signature that fills its own hole and the
/// function becomes a complete claim with `explain`'s cost missing from it.
///
/// That is a bound below the truth, and it is the specific way this ticket can
/// go wrong. The fix is not in the signature: the frontend must translate the
/// argument, so that admitting the constructor removes only the constructor's
/// hole. The assertion is on the ledger because that is where a callee's name
/// survives.
#[test]
fn an_exception_constructor_does_not_swallow_a_call_in_its_argument() {
    let source = "def g(n: int) -> int:\n    raise ValueError(explain(n))\n";
    let function = only_function(source);

    assert!(
        refuses_call_to(&function, "explain"),
        "`explain(n)` is an unknown call and must be named where it stands. The \
         ledger holds {:?}, which names only the constructor - so the argument \
         was never translated, and a signature for `ValueError` would turn this \
         into a complete bound with `explain`'s cost silently missing. \
         For:\n{source}",
        refusals(&function)
    );
    assert!(
        !cost(function.program()).is_complete(),
        "a function whose raised value is built by an unknown call makes no \
         complete claim: {} for:\n{source}",
        describe(&cost(function.program()))
    );
}

// ---------------------------------------------------------------------------
// 6 · the callees that stay holes, and why
// ---------------------------------------------------------------------------

/// **Cost that depends on an argument's size is not a signature's business.**
///
/// Pinned as a table so that a later contributor who wants one of these in the
/// pack has to delete a test with a reason written on it rather than append a
/// row. Each entry is here for one of three reasons:
///
/// | callee | why it is not admissible |
/// |---|---|
/// | `join` | linear in its input. `LAN-11`'s size analysis, not a signature. |
/// | `sorted` | `n log n` in its input. Same. |
/// | `list` | copies its argument; 14 stdlib sole-blocked sites, and every one of them is linear. |
/// | `print` | I/O. Unbounded by anything this analysis can see. |
/// | `append` | mutates a collection, so it is not even *safe*: `collection_length.rs` relies on it forgetting the entry length. |
/// | `decode` | allocates, and its cost is the input's length. |
///
/// A bounded-cost claim about any of them would be a bound the program exceeds.
#[test]
fn a_callee_whose_cost_depends_on_its_argument_stays_a_hole() {
    for (callee, statement) in [
        ("join", "\",\".join(items)"),
        ("sorted", "sorted(items)"),
        ("list", "list(items)"),
        ("print", "print(items)"),
        ("append", "items.append(1)"),
        ("decode", "items.decode(\"utf-8\")"),
    ] {
        let source = format!("def g(items: list) -> int:\n    {statement}\n    return 0\n");
        let function = only_function(&source);
        let result = cost(function.program());

        assert!(
            !result.is_complete(),
            "`{callee}`'s cost is a function of its argument's size, which this \
             analysis cannot see - admitting it to the pack would publish a \
             constant bound for work that grows with the input. It stays a hole \
             until `LAN-11`. Got {} for:\n{source}",
            describe(&result)
        );
        assert!(
            refuses_call_to(&function, callee),
            "`{callee}` must still be refused by name so the coverage report can \
             keep counting it: the ledger holds {:?} for:\n{source}",
            refusals(&function)
        );
    }
}

/// **`__enter__` and `__exit__` stay holes, and keep costing the block below.**
///
/// 96 sites each on the typed corpus, which makes them the most tempting rows in
/// the table and the two that must not be added. They are landav's *own* holes,
/// emitted by `LAN-92`'s `with` support rather than written by the user, and
/// they are calls to arbitrary user code: a lock's `__enter__` can block
/// indefinitely, and there is no bound to declare for it.
///
/// The loop below the `with` must therefore still be holed, exactly as under an
/// unknown callee. This is the same fence as
/// `an_unknown_callee_above_a_loop_still_erases_it`, aimed at the one pair of
/// callees a coverage number would argue hardest for.
#[test]
fn a_context_manager_protocol_call_stays_a_hole() {
    let source = "\
def g(lock, n: int) -> int:
    with lock:
        pass
    for i in range(n):
        pass
    return 0
";
    let function = only_function(source);
    let result = cost(function.program());
    let shown = describe(&result);

    for callee in ["__enter__", "__exit__"] {
        assert!(
            refuses_call_to(&function, callee),
            "`{callee}` runs arbitrary user code - a lock's `__enter__` can block \
             indefinitely - so it has no bounded cost to declare and must stay \
             refused by name. The ledger holds {:?} for:\n{source}",
            refusals(&function)
        );
    }
    assert!(
        holes_on(&result, "for") && !result.is_complete(),
        "`__enter__` may run anything, including code that rebinds nothing but \
         takes unbounded time, so the loop after the `with` may not be counted \
         across it. Got {shown} for:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// 7 · `len` — the one already half-solved, in both directions
// ---------------------------------------------------------------------------

/// **`LAN-89` regression guard: `len(items)` for a collection parameter is a
/// bound variable, not a pack entry.**
///
/// `len` is not in the pack and must not be put there. For a collection
/// *parameter* the frontend already reads it as the variable `len(items)` — a
/// natural number the caller supplies — and it lowers, so it is counted in the
/// headline coverage figure. A signature declaring `len` to cost a constant
/// would replace a bound that *scales with the input* by a constant, which is
/// the opposite of what this tool is for.
#[test]
fn a_collection_parameters_length_is_still_read_as_a_variable() {
    let source = "\
def g(items: list) -> int:
    total = 0
    for i in range(len(items)):
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        mentioned(&result).iter().any(|name| name == "len(items)"),
        "`LAN-89` reads `len(items)` for a collection parameter as a bound \
         variable, and this bound must still be a function of it: {shown} \
         for:\n{source}"
    );
    assert!(
        result.holes().is_empty(),
        "the loop over the length is counted, not holed: {shown} for:\n{source}"
    );
    assert!(
        landav_its::lower(function.program()).is_ok(),
        "this function lowers today and must keep lowering - it is counted in \
         the headline coverage figure, and that number may not fall because a \
         pack was added beside it. For:\n{source}"
    );
}

/// **`len` of anything that is not a collection parameter stays a hole.**
///
/// The other half. `len(items)` where `items` is unannotated, and `len(made)`
/// where `made` was built by a call, are not values the caller supplies and are
/// not constants either — the frontend cannot even say the argument is a
/// collection. There is nothing bounded to declare, so the call stays a region.
#[test]
fn a_length_of_something_that_is_not_a_collection_parameter_stays_a_hole() {
    for source in [
        // Unannotated: the frontend trusts annotations and there is none.
        "def g(items) -> int:\n    return len(items)\n",
        // A local built by a call this analysis cannot read.
        "def g(n: int) -> int:\n    made = build(n)\n    return len(made)\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());

        assert!(
            !result.is_complete(),
            "`len` of something the caller did not supply is neither a constant \
             nor a variable, so it may not be resolved by a signature: {} \
             for:\n{source}",
            describe(&result)
        );
        assert!(
            refuses_call_to(&function, "len"),
            "the `len` must still be refused by name, or `LAN-89`'s narrow \
             reading has quietly widened. The ledger holds {:?} for:\n{source}",
            refusals(&function)
        );
    }
}

// ---------------------------------------------------------------------------
// 8 · a pack entry is charged per iteration
// ---------------------------------------------------------------------------

/// **A pack entry inside a loop is paid once per iteration, not once.**
///
/// The failure a cost signature makes easy: resolve `isinstance` to a constant,
/// add the constant to the function's total, and a loop calling it `n` times is
/// under-reported by `n - 1` of them. Charged once outside the loop the bound
/// grows by a constant; charged inside it grows with `n`.
///
/// Stated against the empty loop's own marginal cost so it survives a change to
/// what a statement costs. `for i in range(n): pass` costs one step per
/// iteration; adding a statement to the body costs at least one more, so the
/// marginal cost of the type-test loop must be at least twice the empty loop's.
#[test]
fn a_pack_entry_inside_a_loop_is_charged_every_iteration() {
    let source = loop_over("isinstance(x, int)");
    let function = only_function(&source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        result.is_complete(),
        "a counted loop whose body is a bounded type test has a complete bound: \
         {shown} for:\n{source}"
    );

    let empty = cost(only_function(&loop_over("pass")).program());
    assert!(
        empty.is_complete(),
        "the empty-loop baseline must be complete or this comparison means \
         nothing: {}",
        describe(&empty)
    );

    let tested = at(&result, 20) - at(&result, 10);
    let bare = at(&empty, 20) - at(&empty, 10);
    assert!(
        tested >= bare * 2,
        "ten more iterations cost {tested} more with `isinstance(x, int)` in the \
         body and {bare} more with `pass`. A body statement is charged at least \
         one step per iteration on top of the loop's own, so the marginal cost \
         must be at least double. A ratio near 1 means the type test was charged \
         once for the whole loop rather than once per iteration - which \
         under-reports every loop in the corpus that tests a type. Got {shown} \
         for:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// 9 · soundness — the bound may never fall below the truth
// ---------------------------------------------------------------------------

/// **A pack entry does not swallow an unknown call in its own arguments.**
///
/// `isinstance(x, lookup(n))` reports `Partial(2 + #hole0)` today with
/// `call(isinstance)` as the *only* refusal in the ledger. `lookup(n)` is never
/// translated: the frontend refuses the outer call and stops, so the inner one
/// exists in no arena and is invisible to the refusal scan and to the walk
/// alike.
///
/// That is harmless only while the outer call is itself a hole, because the hole
/// denotes `omega` and `omega` dominates whatever `lookup` costs. **Admitting
/// `isinstance` to the pack removes the thing that was covering for it**, and
/// the function becomes `Theta(2)` — complete, exact, no holes — for a program
/// that runs `lookup(n)`. A bound below the truth, published as certainty.
///
/// This is the one place in this file where the required change is in the
/// *frontend* rather than in the pack, and it is stated as a precondition: the
/// argument must be translated before its callee may be resolved. The session
/// notes the same gap as "`fetch(g(n))` reports 1 query, not 2".
#[test]
fn a_pack_entry_does_not_swallow_an_unknown_call_in_its_arguments() {
    let source = "def g(x, n: int) -> int:\n    isinstance(x, lookup(n))\n    return 0\n";
    let function = only_function(source);

    assert!(
        refuses_call_to(&function, "lookup"),
        "`lookup(n)` is an unknown call inside a pack entry's argument list and \
         must be named where it stands. The ledger holds {:?} - only the outer \
         callee - so nothing is accounting for `lookup`, and resolving \
         `isinstance` would turn this into a complete bound that omits it. This \
         is the direction that must never happen: a bound below the truth. \
         For:\n{source}",
        refusals(&function)
    );
    assert!(
        !cost(function.program()).is_complete(),
        "while an argument holds an unknown call the function makes no complete \
         claim: {} for:\n{source}",
        describe(&cost(function.program()))
    );
}

/// **The prize does not extend to a value the callee could have changed.**
///
/// `isinstance` cannot rebind a local, which is what recovers the loop below a
/// guard. It can still run user code — a `__instancecheck__`, a `__class__`
/// property — and that code may *mutate an object*. `items.append(...)` inside
/// one changes the length the loop below will walk.
///
/// `Walk::region` already draws exactly this distinction for `Attribute` and
/// `Subscript`, via `SourceProgram::is_volatile`, and the pack must inherit it
/// rather than reopen it. A complete bound over `len(items)` here would be
/// exceeded by the program.
#[test]
fn a_pack_entry_still_forgets_a_length_it_could_have_changed() {
    let source = "\
def g(items: list, x) -> int:
    total = 0
    items.append(1)
    if isinstance(x, int):
        pass
    for y in items:
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        !result.is_complete(),
        "`items.append(1)` changed the length the loop walks, so the length the \
         caller supplied no longer counts it - and no signature on `isinstance` \
         may re-license reading it. A complete bound here is exceeded by the \
         program. Got {shown} for:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// the headline number
// ---------------------------------------------------------------------------

/// **A function blocked only by a pack entry lowers.**
///
/// The ticket's first acceptance criterion is that the `call` sole-blocker count
/// *falls*, and that count is taken from the lowering's refusal ledger, not from
/// the engine. An engine that charges `isinstance` correctly while
/// `landav_its::lower` still refuses it moves no headline number.
///
/// This is separated from the engine tests deliberately: an implementation that
/// satisfies one and not the other is a real, partial state of the world, and
/// two tests say which half landed where one would not.
#[test]
fn a_function_blocked_only_by_a_pack_entry_lowers() {
    for (callee, source) in bounded_pack_statements() {
        let function = only_function(&source);
        assert!(
            !refuses_call_to(&function, callee),
            "`{callee}` has a bounded cost and cannot rebind a local, so it is \
             representable as an ordinary step with an identity update and must \
             stop refusing to lower - the `call` sole-blocker count is measured \
             from this ledger, and it holds {:?} for:\n{source}",
            refusals(&function)
        );
        assert!(
            landav_its::lower(function.program()).is_ok(),
            "nothing else in this function is refused, so once `{callee}` is in \
             the pack it lowers and is counted: the ledger holds {:?} \
             for:\n{source}",
            refusals(&function)
        );
    }
}
