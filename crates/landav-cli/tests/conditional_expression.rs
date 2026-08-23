//! `LAN-91` acceptance: **`a if c else b` is a branch, not a wall.**
//!
//! # The measurement this exists to move
//!
//! `conditional-expression` is the sole blocker for 118 stdlib functions and
//! appears 369 times across the corpus. Python's ternary is not an exotic
//! construct that a cost analysis may reasonably decline: it is the idiom
//! Python offers *instead of* a four-line `if`, and the four-line `if` has been
//! derivable since the engine existed. Two spellings of one program, one of
//! which produces a bound and one of which produces a region, is a gap in the
//! frontend rather than a fact about the program.
//!
//! # What a ternary costs
//!
//! The condition is evaluated whichever way it goes, so its cost is **added**.
//! Exactly one arm then runs, so the arms are combined by **maximum** - the
//! worst-case question the whole tool asks. Both halves of that already exist:
//! [`landav_bound::Bound::max_of`] forms the maximum, and
//! `TripCount::branching` is what an `if` *statement* already uses to form it.
//! Nothing here needs new arithmetic; it needs the ternary to reach it.
//!
//! The tests below therefore never pin a step count they invented. Where a
//! number matters it is either derived from the same program spelled as an
//! `if` statement, or expressed as a *difference* - what changes when one arm
//! gets more expensive - so that the assertions survive a change to what a
//! statement costs and still fail loudly on a maximum done wrong.
//!
//! # The failure this file is really guarding against
//!
//! `max` and `sum` agree whenever the arms cost the same, and every arm in a
//! trivial example costs the same. A suite made only of `x = a if c else b`
//! would pass against an implementation that **adds** both arms - sound but
//! loose - and, worse, against one that keeps only the arm it happened to
//! visit first, which is *unsound* and is the one direction a resource bound
//! must never move. Several tests below give the two arms deliberately
//! different costs and assert the number, because that is the only shape in
//! which those three implementations disagree.
//!
//! # Why these tests read the libraries rather than driving the binary
//!
//! The assertions are about the *shape* of a bound - which regions it names,
//! whether an arm's cost is added or maximised - and the process boundary
//! offers only the rendered string, which these tests are forbidden to pin.
//! This is the only package that depends on both the frontend and the engine,
//! so the file lives here and adds no dependency edge; `collection_length.rs`
//! and `calls_become_holes.rs` are its neighbours for the same reason.
//!
//! # Scope fence
//!
//! `Expr::IfExp`, `Expr::BoolOp` and `Expr::Compare` share one match arm in
//! `landav_python`'s `build_expression` today, and all three refuse as
//! `Construct::ConditionalExpression`. This ticket is about the ternary.
//! `boolean_and_comparison_values_stay_refused` pins the other two where they
//! are, so that splitting the arm cannot widen them by accident; see its doc
//! comment for what a later ticket would have to argue.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, path::Path};

use landav_bound::{Nat, Symbol, Valuation, VarId};
use landav_engine::{Hole, TripCount, cost};
use landav_python::LoweredFunction;

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

/// A valuation over an explicit table, zero elsewhere.
///
/// Zero rather than omega for an unlisted name, so that a test which binds one
/// hole and leaves its neighbours alone is asking "what does *this* region
/// contribute", which is exactly the question every number below is about.
struct Bindings(BTreeMap<Symbol, u64>);

impl Valuation for Bindings {
    fn value_of(&self, var: &VarId) -> Nat {
        Nat::Fin(self.0.get(var.symbol()).copied().unwrap_or(0))
    }
}

/// A valuation binding named parameters and named regions.
fn table(params: &[(&str, u64)], holes: &[(&Hole, u64)]) -> Bindings {
    let mut bound: BTreeMap<Symbol, u64> = params
        .iter()
        .map(|(name, value)| (Symbol::from(*name), *value))
        .collect();
    for (hole, value) in holes {
        bound.insert(hole.var().symbol().clone(), *value);
    }
    Bindings(bound)
}

/// Translates `source` and returns its single function.
fn only_function(source: &str) -> LoweredFunction {
    let mut functions = landav_python::lower_module(Path::new("ternary.py"), source)
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

/// The bound's value under `at`, as a number.
///
/// Every bound in this file is finite once its regions are given finite costs,
/// so an omega here means the analysis lost a term rather than that the test
/// asked something unanswerable - and saying which is worth the helper.
fn value_of(result: &TripCount, at: &Bindings, source: &str) -> u64 {
    let bound = result
        .bound()
        .unwrap_or_else(|| panic!("no bound at all was derived for:\n{source}"));
    match bound.eval(at) {
        Nat::Fin(value) => value,
        Nat::Omega => panic!(
            "the bound evaluated to omega with every region given a finite \
             cost, which means a term escaped the arithmetic: {} for:\n{source}",
            describe(result)
        ),
    }
}

/// The whole cost of `source`, as a constant.
///
/// Used to compare a ternary against the `if` statement it is shorthand for.
/// Both programs are region-free, so both bounds are constants and the
/// comparison is a comparison of numbers rather than of shapes.
fn constant_cost(source: &str) -> u64 {
    let function = only_function(source);
    let result = cost(function.program());
    assert!(
        result.is_complete(),
        "this program is meant to be fully derivable and is the yardstick the \
         ternary is measured against, so it carrying a region means the \
         yardstick moved, not the ternary: {} for:\n{source}",
        describe(&result)
    );
    value_of(&result, &table(&[], &[]), source)
}

/// The regions this result blames on `construct`.
fn holes_blamed_on<'a>(result: &'a TripCount, construct: &str) -> Vec<&'a Hole> {
    result
        .holes()
        .iter()
        .filter(|hole| hole.construct() == construct)
        .collect()
}

/// **The** assertion this ticket exists for: the ternary itself is no longer a
/// region.
///
/// A single helper rather than a repeated `assert!`, because this is the one
/// sentence the ticket is: whatever else a function containing `a if c else b`
/// reports, it may not report the ternary as something it could not read.
fn assert_ternary_is_not_a_region(result: &TripCount, source: &str) {
    let refused = holes_blamed_on(result, "conditional-expression");
    assert!(
        refused.is_empty(),
        "`a if c else b` is Python's spelling of an `if`, and an `if` has been \
         derivable since the engine existed - so the ternary must not be a \
         region of its own. Still blamed on `conditional-expression`: {:?}. \
         Got {} for:\n{source}",
        refused.iter().map(ToString::to_string).collect::<Vec<_>>(),
        describe(result)
    );
}

/// The regions inside the arms, once the arms are read at all.
///
/// Asserted rather than unwrapped so that today's failure reads as the missing
/// behaviour it is. The arms of a ternary are never inspected at the moment -
/// `expression_children` gives `Expr::IfExp` no children, so the whole
/// expression collapses to one `Unsupported` node and the calls inside it are
/// invisible - and a test that merely panicked here would not say that.
fn call_regions(result: &TripCount, wanted: usize, source: &str) -> Vec<Hole> {
    let calls = holes_blamed_on(result, "call");
    assert_eq!(
        calls.len(),
        wanted,
        "the arms of the ternary must be read, so the {wanted} call(s) inside \
         them must each be named as a region a caller can fill in with what \
         that callee costs. Today the whole ternary is one opaque \
         `conditional-expression` region and the calls inside it are never \
         translated, so there is nothing to fill and the arms read as free. \
         Got {} for:\n{source}",
        describe(result)
    );
    calls.into_iter().cloned().collect()
}

/// A region must be placed as well as named, or the user cannot act on it.
fn assert_placed_and_used(result: &TripCount, hole: &Hole, source: &str) {
    assert!(
        hole.origin().as_str().contains(':'),
        "a region must be placed as well as named, got {} for:\n{source}",
        hole.origin()
    );
    let bound = result
        .bound()
        .unwrap_or_else(|| panic!("no bound at all was derived for:\n{source}"));
    assert!(
        bound.vars().contains(&hole.var()),
        "the region {} does not occur in {bound}, so the bound reads as a \
         complete cost with a footnote and `Bound::subst` has nothing to fill \
         for:\n{source}",
        hole.var().symbol()
    );
}

// ---------------------------------------------------------------------------
// the ternary is derived
// ---------------------------------------------------------------------------

/// **A ternary on the right of an assignment is analysed, not holed.**
///
/// The headline. `x = a if c else b` and the four-line `if` below it are the
/// same program, and the engine derives one of them exactly today
/// (`Theta(3)`) while reporting the other as three regions it could not read.
///
/// # Why the number is a comparison rather than a literal
///
/// Two readings of "the ternary costs the test plus the maximum of the arms"
/// are both defensible, and this test deliberately accepts either. If the
/// frontend desugars the ternary into a branch statement, the branch charges
/// its own step and the function costs 3, exactly as the spelled-out form
/// does. If instead the engine learns to cost a conditional *expression*, an
/// expression is not a statement and charges no step of its own, so the
/// function costs 2 - the assignment and the `return` - just as `x = a + b`
/// does. What is *not* defensible is either extreme: below 2 means a statement
/// went uncharged, and above the spelled-out form means the shorthand costs
/// more than the long form of the same program.
///
/// Note also what the ternary does **not** buy: `x` holds one of two values
/// and the fragment has no way to say which, so nothing here claims `x` is
/// readable afterwards. See `the_value_of_a_ternary_is_not_taken_from_one_arm`.
#[test]
fn a_ternary_in_an_assignment_is_analysed_rather_than_holed() {
    let source = "\
def choose(a: int, b: int, c: int) -> int:
    x = a if c else b
    return x
";
    let spelled = "\
def spelled(a: int, b: int, c: int) -> int:
    if c:
        x = a
    else:
        x = b
    return x
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_ternary_is_not_a_region(&result, source);
    assert!(
        result.is_complete(),
        "every part of this function is in the fragment once the ternary is - \
         three integer parameters, a branch on one of them, a `return` - so \
         there is nothing left for a region to stand for: {} for:\n{source}",
        describe(&result)
    );

    let bound = result.bound().expect("a complete result carries a bound");
    assert!(
        bound.vars().is_empty(),
        "this function costs the same whatever its arguments are, so its bound \
         must mention no variable at all, got {bound} for:\n{source}"
    );

    let derived = value_of(&result, &table(&[], &[]), source);
    let long_form = constant_cost(spelled);
    assert!(
        derived >= 2,
        "the assignment and the `return` are two statements and each costs a \
         step, so {derived} is below what this function certainly does: {} \
         for:\n{source}",
        describe(&result)
    );
    assert!(
        derived <= long_form,
        "`x = a if c else b` is shorthand for the four-line `if`, which costs \
         {long_form}. Reporting {derived} for the shorthand means the two \
         spellings of one program disagree, and the shorthand is the one \
         Python programmers actually write: {} for:\n{source}",
        describe(&result)
    );
}

/// **A nested ternary is analysed too.**
///
/// `a if c else (b if d else e)` is the shape the corpus reaches for when a
/// value has three cases, and it is the shape that separates "the frontend
/// learned one pattern" from "the frontend learned the construct". A
/// translation that only handled a ternary whose arms are plain names would
/// pass the test above and fail here.
///
/// The upper yardstick is the nested `if` this is shorthand for, so a reading
/// that *multiplied* the levels rather than maximising within each one is
/// caught by the number rather than by inspection.
#[test]
fn a_nested_ternary_is_analysed_rather_than_holed() {
    let source = "\
def nest(a: int, b: int, c: int, d: int, e: int) -> int:
    x = a if c else (b if d else e)
    return x
";
    let spelled = "\
def spelled(a: int, b: int, c: int, d: int, e: int) -> int:
    if c:
        x = a
    else:
        if d:
            x = b
        else:
            x = e
    return x
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_ternary_is_not_a_region(&result, source);
    assert!(
        result.is_complete(),
        "a ternary inside a ternary is still two branches on integers and \
         nothing else, so nothing is left to stand for: {} for:\n{source}",
        describe(&result)
    );

    let derived = value_of(&result, &table(&[], &[]), source);
    let long_form = constant_cost(spelled);
    assert!(
        derived >= 2,
        "the assignment and the `return` are two statements: {derived} is \
         below what this function certainly does for:\n{source}"
    );
    assert!(
        derived <= long_form,
        "the nested ternary is shorthand for the nested `if`, which costs \
         {long_form}; reporting {derived} means nesting was charged as a \
         product of the levels rather than a maximum within each: {} \
         for:\n{source}",
        describe(&result)
    );
}

// ---------------------------------------------------------------------------
// the maximum, where max, sum and "first arm wins" disagree
// ---------------------------------------------------------------------------

/// **The cost follows the more expensive arm, and only that arm.**
///
/// Exactly one arm runs, so the two are combined by maximum. Every trivial
/// example hides this, because two arms of equal cost make `max`, `sum` and
/// "keep whichever arm we saw first" agree on the answer. Here they do not:
///
/// | reading | at `expensive = 10`, `cheap = 3` |
/// |---|---|
/// | maximum - correct | base + 10 |
/// | sum - sound but loose, and what a hoisted refusal list would give | base + 13 |
/// | the cheap arm only - **unsound** | base + 3 |
///
/// `base` is the same bound with both regions costing nothing, so the
/// assertion is about what the arms *contribute* and does not depend on how
/// many steps the statement around them is charged.
///
/// # The sum reading is not hypothetical
///
/// `landav_python`'s `refusals_of`/`hoisted` turns the refusals inside an
/// expression into a *list of statements* at the same position, and the engine
/// sums a statement list. That is right for `x = f(n) + g(n)`, where both calls
/// happen, and wrong here, where exactly one does. A ternary implemented by
/// hoisting its arms' refusals the way every other expression's are gets
/// `base + 13`, which is sound and permanently loose on 369 sites.
///
/// # And the unsound reading is the one to fear
///
/// Both orderings are asserted, so an implementation that keeps whichever arm
/// it visited first cannot pass by luck: it fails on one of the two.
#[test]
fn the_cost_follows_the_more_expensive_arm() {
    let source = "\
def choice(c: int) -> int:
    expensive() if c else cheap()
    return 0
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_ternary_is_not_a_region(&result, source);
    let arms = call_regions(&result, 2, source);
    let (left, right) = (&arms[0], &arms[1]);
    for hole in &arms {
        assert_placed_and_used(&result, hole, source);
    }

    let base = value_of(&result, &table(&[], &[(left, 0), (right, 0)]), source);
    for (first, second) in [(10_u64, 3_u64), (3, 10)] {
        let derived = value_of(
            &result,
            &table(&[], &[(left, first), (right, second)]),
            source,
        );
        assert_eq!(
            derived,
            base + 10,
            "one arm runs, so the two are combined by maximum: with arms \
             costing {first} and {second} the ternary must contribute 10 over \
             the {base} this function costs with both arms free, and it \
             contributed {}. {} more than 10 means both arms were added, which \
             is sound and loose; less than 10 means an arm was dropped, which \
             is a bound below the truth. Got {} for:\n{source}",
            derived.saturating_sub(base),
            "Contributing",
            describe(&result)
        );
    }
}

/// **The bound never falls below always taking the expensive arm.**
///
/// The soundness statement, written as a comparison against a program whose
/// answer is not in doubt. `expensive() if c else 0` must cost at least what
/// `expensive()` on its own costs, because `c` may be true - and the engine
/// performs no reachability analysis, so it has no evidence that it is not.
///
/// Stated this way rather than as a literal because it stays meaningful
/// however the implementation is arranged: whatever regions the ternary names,
/// giving the ones that stand for `expensive` a cost of `K` and every other
/// region nothing must not produce a smaller number than the same `K` in the
/// unconditional program.
///
/// Today it fails, and the failure is precisely the thing the ticket
/// describes: the ternary is one opaque region, `expensive()` inside it is
/// never translated, so there is no region standing for that call and a caller
/// who knows exactly what `expensive` costs has nowhere to put the number. The
/// bound then reads as though the arm were free.
#[test]
fn the_bound_never_falls_below_always_taking_the_expensive_arm() {
    let conditional = "\
def choice(c: int) -> int:
    expensive() if c else 0
    return 0
";
    let unconditional = "\
def always(c: int) -> int:
    expensive()
    return 0
";
    const CALL: u64 = 100;

    let chosen = only_function(conditional);
    let chosen_result = cost(chosen.program());
    let taken = only_function(unconditional);
    let taken_result = cost(taken.program());

    // Whatever regions stand for `expensive`, they are the ones named for a
    // call; everything else in either program costs nothing.
    let charge = |result: &TripCount, source: &str| -> u64 {
        let calls: Vec<(&Hole, u64)> = holes_blamed_on(result, "call")
            .into_iter()
            .map(|hole| (hole, CALL))
            .collect();
        value_of(result, &table(&[], &calls), source)
    };
    let with_choice = charge(&chosen_result, conditional);
    let without = charge(&taken_result, unconditional);

    assert!(
        with_choice >= without,
        "`expensive() if c else 0` must cost at least what `expensive()` costs \
         on its own - `c` may be true, and this engine does no reachability \
         analysis, so it has no evidence that it is not. With the call costing \
         {CALL} the unconditional program reports {without} and the \
         conditional one reports {with_choice}, which is a bound BELOW the \
         truth: the one direction a resource bound must never move. A caller \
         comparing {with_choice} against a budget would pass a program that \
         actually costs {without}.\n\n\
         conditional:   {}\n\
         unconditional: {}",
        describe(&chosen_result),
        describe(&taken_result)
    );
}

/// **A ternary in a loop body is paid once per iteration.**
///
/// The coverage claim rests on this. A region charged flat at the top level
/// instead of inside the loop that runs it understates by a factor of the trip
/// count, which is the failure `Walk::reconciled` fails closed to avoid, and
/// it is easy to reintroduce: a frontend that hoists a ternary's refusals to
/// the enclosing *function* rather than the enclosing *statement* produces
/// exactly that shape.
///
/// Two numbers separate the readings, and neither depends on what a statement
/// costs:
///
/// * at `n = 3` the arm must contribute three times its cost, not once;
/// * at `n = 0` it must contribute nothing at all, because the body never runs.
#[test]
fn a_ternary_in_a_loop_body_is_paid_once_per_iteration() {
    let source = "\
def repeat(n: int, c: int) -> int:
    for i in range(n):
        work() if c else 0
    return 0
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_ternary_is_not_a_region(&result, source);
    let arms = call_regions(&result, 1, source);
    let call = &arms[0];
    assert_placed_and_used(&result, call, source);

    let free_at_three = value_of(&result, &table(&[("n", 3)], &[(call, 0)]), source);
    let paid_at_three = value_of(&result, &table(&[("n", 3)], &[(call, 7)]), source);
    assert_eq!(
        paid_at_three - free_at_three,
        21,
        "the loop runs three times and the arm costs 7 each time, so it must \
         contribute 21 rather than 7. A contribution of 7 means the region was \
         charged once, outside the multiplication - which understates every \
         loop containing a ternary by a factor of the trip count. Got {} \
         for:\n{source}",
        describe(&result)
    );

    let free_at_zero = value_of(&result, &table(&[("n", 0)], &[(call, 0)]), source);
    let paid_at_zero = value_of(&result, &table(&[("n", 0)], &[(call, 7)]), source);
    assert_eq!(
        paid_at_zero,
        free_at_zero,
        "a body that never runs costs nothing, so a region inside it must \
         contribute nothing at `n = 0`. Got {} for:\n{source}",
        describe(&result)
    );
}

// ---------------------------------------------------------------------------
// what stays a region, and what it must be blamed on
// ---------------------------------------------------------------------------

/// **An unanalysable condition is blamed on itself, placed, and charged once.**
///
/// A ternary whose *arms* are fine and whose *test* is a call is the common
/// corpus shape, and the report has to point at the part the user can act on.
/// The ternary is not that part - it is Python's own syntax, and telling a user
/// their `if`-expression is unanalysable tells them nothing they can change.
/// The call is: fill in what `probe` costs and the bound completes. This is the
/// same blame discipline `calls_become_holes.rs` already holds an `if`
/// *statement* to.
///
/// The condition also runs whichever arm is taken, so its cost is **added**
/// rather than maximised over the arms - asserted as a difference, so it fails
/// both if the condition is charged twice and if it is folded into the maximum
/// and disappears when one arm is cheap.
#[test]
fn an_unanalysable_condition_is_named_placed_and_charged_once() {
    let source = "\
def guarded(a: int, b: int) -> int:
    x = a if probe() else b
    return x
";
    let function = only_function(source);
    let result = cost(function.program());

    assert_ternary_is_not_a_region(&result, source);
    assert!(
        result.bound().is_some(),
        "a region inside a ternary must leave the rest of the function \
         derived - naming what could not be read is the whole point of a hole, \
         and `Unknown` names nothing: {} for:\n{source}",
        describe(&result)
    );

    let calls = call_regions(&result, 1, source);
    let call = &calls[0];
    assert_placed_and_used(&result, call, source);

    let free = value_of(&result, &table(&[], &[(call, 0)]), source);
    let charged = value_of(&result, &table(&[], &[(call, 5)]), source);
    assert_eq!(
        charged - free,
        5,
        "the test is evaluated on every path, so it is added once - not \
         maximised against the arms, where a cheap arm would make it vanish, \
         and not charged in both arms, where it would be counted twice. Got {} \
         for:\n{source}",
        describe(&result)
    );
}

/// **A ternary over non-integer arms does not erase the enclosing bound.**
///
/// `s = "yes" if c else "no"` is not a value this fragment has, and it must
/// stay refused - the fragment's contract is that every variable it reads
/// denotes a mathematical integer, and a string is not one. What it must not
/// do is take the rest of the function down with it: the loop above it is
/// counted over `len(items)` today and must still be, and the whole result must
/// remain a bound naming its regions rather than `TripCount::Unknown`.
///
/// # This one passes today, and it is a fence rather than a driver
///
/// The way it breaks is specific and has already bitten this codebase once. If
/// the frontend translates a ternary into arena nodes the engine's walk does
/// not reach (the orphan shape `LAN-90` fixed for refused comparison operands),
/// then `Walk::reconciled` finds an `Unsupported` node the walk never charged
/// and fails the **entire function** closed to `Unknown`. A ternary with a
/// string arm is the most likely place to build such an orphan, because the
/// arm refuses while the ternary around it succeeds. The function would go from
/// a partial bound naming two regions to no bound at all, and nothing else in
/// this file would notice.
#[test]
fn a_ternary_over_non_integer_arms_does_not_erase_the_enclosing_bound() {
    let source = "\
def label(items: list, c: int) -> int:
    total = 0
    for x in items:
        total = total + 1
    s = \"yes\" if c else \"no\"
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        !matches!(result, TripCount::Unknown),
        "a ternary over values the fragment cannot hold must become a named \
         region, not erase everything derived around it: {shown} for:\n{source}"
    );
    let bound = result
        .bound()
        .unwrap_or_else(|| panic!("{shown} carries no bound for:\n{source}"));

    let names: Vec<String> = bound
        .vars()
        .iter()
        .map(|var| var.symbol().as_str().to_owned())
        .collect();
    assert!(
        names.iter().any(|name| name == "len(items)"),
        "the loop above the ternary is counted by the collection's length \
         today and must still be - the ternary below it changes nothing about \
         how many times that loop runs. Bound mentions {names:?}: {shown} \
         for:\n{source}"
    );
    assert!(
        holes_blamed_on(&result, "for").is_empty(),
        "the counted loop must not become a region because of something \
         written after it: {shown} for:\n{source}"
    );

    // The value side of the refusal, which no cost result can express: a
    // string-valued name may never enter the integer transition system.
    let refused = landav_its::lower(function.program());
    assert!(
        refused.is_err(),
        "`s` holds a string, and `landav_its::VarName` promises a mathematical \
         integer - so this program must still fail to lower however well its \
         cost is derived:\n{source}"
    );
    let constructs = refused
        .as_ref()
        .err()
        .and_then(landav_its::LoweringError::refusals)
        .map(landav_its::Refusals::constructs)
        .unwrap_or_default();
    assert!(
        !constructs.is_empty(),
        "the refusal must name what it refused, or the coverage report has \
         nothing to attribute it to:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// soundness fences
// ---------------------------------------------------------------------------

/// **The value of a ternary is not taken from one of its arms.**
///
/// The cost of `a if c else b` is derivable; its *value* is not. Which arm ran
/// is exactly what this analysis does not know, so `x` afterwards holds one of
/// two numbers and the fragment has no expression for that - there is no
/// maximum in [`landav_its::SourceExpr`], and there deliberately is not, because
/// every variant of it must denote a polynomial.
///
/// The tempting shortcut when desugaring a ternary is to let `x` keep one arm's
/// value, and here that is measurable: a loop running `x` times runs `a` times
/// or `b` times depending on `c`, so a *complete* bound claiming `a` alone is
/// exceeded whenever `b` is larger. Guarded on completeness because a partial
/// result makes no finite claim - an unfilled region denotes omega - which is
/// the honest answer this program gets today.
///
/// # This one passes today, on the partial branch
///
/// It is here because it is the assertion that turns red the moment someone
/// makes the ternary cheap by making it dishonest, and nothing else in the file
/// looks at what a ternary's value is allowed to be used for. The partial
/// branch is not a free pass either: an incomplete answer has to *say* what it
/// could not read, at a position, and with that region actually occurring in
/// the bound - otherwise "not complete" would be an excuse rather than a claim,
/// and the test would be green for no reason.
#[test]
fn the_value_of_a_ternary_is_not_taken_from_one_arm() {
    let source = "\
def risky(a: int, b: int, c: int) -> int:
    x = a if c else b
    total = 0
    for i in range(x):
        total = total + 1
    return total
";
    let function = only_function(source);
    let result = cost(function.program());
    let shown = describe(&result);

    assert!(
        !matches!(result, TripCount::Unknown),
        "whatever the trip count turns out to be, the function still has three \
         statements outside the loop and must report them: {shown} for:\n{source}"
    );

    if !result.is_complete() {
        // A partial result claims nothing finite, which is the honest answer
        // when the trip count is a value the fragment cannot write down. It is
        // only honest if it names what it could not read, though, so that half
        // is checked rather than taken on trust.
        let regions = result.holes();
        assert!(
            !regions.is_empty(),
            "an incomplete answer must name the region that made it \
             incomplete: {shown} for:\n{source}"
        );
        for hole in regions {
            assert_placed_and_used(&result, hole, source);
        }
        return;
    }
    for (a, b) in [(1_000_u64, 0_u64), (0, 1_000)] {
        let derived = value_of(&result, &table(&[("a", a), ("b", b)], &[]), source);
        assert!(
            derived >= 1_000,
            "the loop runs `a` times or `b` times depending on `c`, so a \
             complete bound must dominate both. With a = {a} and b = {b} the \
             program performs at least 1000 iterations and the bound reports \
             {derived}: {shown} for:\n{source}"
        );
    }
}

// ---------------------------------------------------------------------------
// the scope fence: what shares the match arm and must not move with it
// ---------------------------------------------------------------------------

/// **`and`, `or` and comparisons used as *values* stay refused.**
///
/// These three share one match arm with `Expr::IfExp` in `build_expression`
/// today and all refuse as `Construct::ConditionalExpression`. Splitting the
/// ternary out of that arm is the whole ticket, and this test is what stops the
/// other two coming along by accident - the implementer has to delete an
/// assertion to widen them, which is a decision rather than a side effect.
///
/// # Why they are genuinely a separate question
///
/// `a and b` does not evaluate to a boolean in Python: it evaluates to `a` when
/// `a` is falsy and to `b` otherwise, so it is a ternary whose condition and
/// first arm are the *same expression*. `a < b` evaluates to a `bool`, which is
/// `0` or `1` and which the fragment could in principle read - a different
/// argument again, and one about values rather than cost. Both may be worth
/// doing; neither is `LAN-91`, and neither is justified by anything this file
/// establishes.
///
/// # This passes today
///
/// It pins current behaviour deliberately. A later ticket that extends either
/// construct updates this test with its own argument recorded here.
#[test]
fn boolean_and_comparison_values_stay_refused() {
    for source in [
        "def conjunction(a: int, b: int) -> int:\n    x = a and b\n    return x\n",
        "def disjunction(a: int, b: int) -> int:\n    x = a or b\n    return x\n",
        "def comparison(a: int, b: int) -> int:\n    x = a < b\n    return x\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        let refused = holes_blamed_on(&result, "conditional-expression");
        assert!(
            !refused.is_empty(),
            "`and`, `or` and comparisons in **value** position are outside \
             `LAN-91` and must still be reported as regions. If this now \
             derives a bound, the ternary was split out of the shared match arm \
             in a way that took its neighbours with it - which is a widening \
             nobody argued for. Got {} for:\n{source}",
            describe(&result)
        );
        for hole in refused {
            assert!(
                hole.origin().as_str().contains(':'),
                "a region must be placed as well as named, got {} for:\n{source}",
                hole.origin()
            );
        }
    }
}

/// **`and`, `or` and comparisons used as *conditions* stay fully derived.**
///
/// The other half of the fence, and the one that costs coverage if it breaks.
/// In condition position these never went through `build_expression` at all -
/// `build_condition` handles `BoolOp` with `and`/`or` nodes and `Compare` with
/// real comparisons - so `if n > 0 and m > 0:` is derived exactly and the
/// program lowers to a transition system with no refusal at all.
///
/// A ternary implementation that routes conditions through the expression
/// builder, or that reuses the value path for the test of an `if`, would turn
/// every guarded loop in the corpus into a region. That is a far larger
/// regression than the 369 sites this ticket is buying, and nothing else here
/// would catch it.
///
/// # This passes today
#[test]
fn boolean_and_comparison_conditions_stay_analysed() {
    for source in [
        "def guard(n: int, m: int) -> int:\n    x = 0\n    if n > 0 and m > 0:\n        x = 1\n    return x\n",
        "def either(n: int, m: int) -> int:\n    x = 0\n    if n > 0 or m > 0:\n        x = 1\n    return x\n",
        "def counted(n: int) -> int:\n    total = 0\n    for i in range(n):\n        if i > 0:\n            total = total + 1\n    return total\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert!(
            result.is_complete(),
            "a boolean or comparison in **condition** position is derived \
             today and must stay derived - it never reached the expression \
             builder's refusing arm, and it must not start reaching it: {} \
             for:\n{source}",
            describe(&result)
        );
        assert!(
            landav_its::lower(function.program()).is_ok(),
            "this program lowers to a transition system today, and \
             `Coverage::lowered()` is the headline number - it must not fall \
             because the expression builder changed:\n{source}"
        );
    }
}
