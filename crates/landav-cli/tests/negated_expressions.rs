//! The fourth member of the discarded-position family: **`not` costs its
//! operand and nothing else.**
//!
//! # Why this is the same argument as `discarded_expressions.rs`, not a new one
//!
//! That file's case rests on one fact about the fragment:
//! [`landav_its::SourceStmt::Return`] has no value slot, and a bare expression
//! statement has none either. In those two positions the value of an expression
//! is read by nothing - it cannot become a bound variable, because there is
//! nowhere to put it - and the only question left is what evaluating it
//! **costs**. On that argument `Expr::BoolOp`, `Expr::Compare` and
//! `Expr::IfExp` were accepted there, and their operands traversed.
//!
//! `Expr::UnaryOp` with `ast::UnaryOp::Not` was left out. Not because the
//! argument fails for it - the argument is *identical*, and `not` is if
//! anything the easiest of the four, since it has one operand and no
//! short-circuit, so charging it is the truth rather than an upper bound - but
//! because it was outside the three forms that pass was scoped to, and every
//! accepted construct is soundness surface that has to be argued for
//! individually rather than swept up.
//!
//! Two further facts make it cheap. `Translator::build_condition` already
//! handles `Not` in full (`condition_children` has the arm), so nothing new is
//! being taught about negation; and `not` truth-tests its operand exactly as an
//! `if` does, so accepting it in a discarded position introduces no evaluation
//! rule the frontend does not already implement somewhere else.
//!
//! # What this is worth, measured - and it is *not* coverage
//!
//! The four remaining `conditional-expression` sole-blockers on the stdlib are
//! all `return not <expr>`: `filecmp._cmp`, `operator.not_`, `pydoc.isdata`,
//! `uuid._is_universal`. It is tempting to read that as "four more functions
//! derive". They do not. Each was measured by running the same body with the
//! `not` replaced by `... or 0` - a `BoolOp`, which discarded position already
//! accepts - and reading what the operands then refuse:
//!
//! | function | what it becomes | derives? |
//! |---|---|---|
//! | `filecmp._cmp` | `call` - `abs`, `cmp` | no |
//! | `operator.not_` | `non-integer-value` - `a` is unannotated | no |
//! | `pydoc.isdata` | `call` - six `inspect.*` predicates | no |
//! | `uuid._is_universal` | `bitwise-operator` - `mac & (1 << 41)` | no |
//!
//! So the coverage this buys is **zero functions on the stdlib**. What it buys
//! is that the `conditional-expression` sole-blocker count goes 4 -> 0 and
//! those four functions start naming what is *actually* in the way. Today the
//! tool tells the author of `uuid._is_universal` to go and look at a
//! conditional expression; the obstacle is a bitwise `&`, and the report never
//! says so. That is the same argument `discarded_expressions.rs` makes for
//! keeping value position refused, run in the other direction: a name that
//! points at the wrong construct is worse than a name that points at a
//! construct nobody can do anything about, because the reader acts on it.
//!
//! The coverage claim that *is* real is on annotated code, where the operand is
//! readable: `def even(n: int) -> int: return not n` derives completely and
//! lowers, and today it does neither. `a_returned_negation_over_integers_is_
//! fully_derived` is that claim. The corpus carries 14 returned-`not` sites in
//! the stdlib and 27 in the typed corpus, so this is a small, cheap widening
//! whose value is in the blame it corrects rather than in the number it moves,
//! and it should be described that way in the ticket.
//!
//! # The rule that makes it safe, restated because it is the whole risk
//!
//! `expression_children` returns nothing for a **refused** form. `return not
//! f(n)` today is one `conditional-expression` region and no `call` region at
//! all - `f` appears nowhere in the report. That is sound only because the
//! region denotes `omega`. The moment `not` is accepted, `f(n)` vanishes with
//! no hole anywhere unless the `Not` arm is added to `expression_children` in
//! the same change, and the function publishes a complete bound that omits a
//! call. `landav_engine::Walk::reconciled` cannot catch it: a node that was
//! never built is not in the arena to reconcile against.
//! `a_discarded_negation_names_the_call_inside_it` is the test that fails if
//! only one of the two arms is written.
//!
//! # The three fences
//!
//! * **Value position stays refused.** `x = not n` keeps its
//!   `conditional-expression` region. Soundness does not require it - `x` is
//!   condemned as `non-integer-value` either way - but the report does, for the
//!   reason `Translator::value_discarded` gives.
//! * **Membership and identity keep their own names.** `not (c in items)` runs
//!   `__contains__`; one operator answers to one construct wherever it is
//!   written, so this is a `collection` region and not a
//!   `conditional-expression` one.
//! * **Condition position is untouched.** `if not n:` already derives, and must
//!   go on deriving.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_engine::{Hole, TripCount, cost};
use landav_python::LoweredFunction;

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

fn functions_of(source: &str) -> Vec<LoweredFunction> {
    landav_python::lower_module(Path::new("negated.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"))
}

fn only_function(source: &str) -> LoweredFunction {
    let mut functions = functions_of(source);
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

/// Every distinct construct this result blames something on.
fn constructs_blamed(result: &TripCount) -> Vec<&str> {
    let mut named: Vec<&str> = result.holes().iter().map(Hole::construct).collect();
    named.sort_unstable();
    named.dedup();
    named
}

// ---------------------------------------------------------------------------
// 1. accepting the container obliges you to traverse it
// ---------------------------------------------------------------------------

/// **`return not f(n)` names the call.**
///
/// The most important test in this file. Today it reports one
/// `conditional-expression` region and `f` is nowhere: not in the arena, not in
/// the refusal ledger, not in the bound. That is sound while the region denotes
/// `omega`, and stops being sound the instant the `not` is accepted without the
/// matching `expression_children` arm - at which point this function reports a
/// complete `Theta(1)` for a body that runs an unknown callee.
///
/// Both assertions are needed and they fail for different reasons: the first
/// catches a widening that forgot to traverse, the second catches one that
/// traversed and then kept the container as a region anyway, which would leave
/// the sole-blocker count exactly where it is.
#[test]
fn a_discarded_negation_names_the_call_inside_it() {
    let source = "def negate(n: int) -> int:\n    return not f(n)\n";
    let function = only_function(source);
    let result = cost(function.program());

    let calls = holes_blamed_on(&result, "call");
    assert_eq!(
        calls.len(),
        1,
        "`not` is accepted in a returned position, so the call it truth-tests \
         is the only thing left that can cost anything - and it must be named \
         where it stands. A count of 0 means the operand was never translated \
         and this function now publishes a bound that omits a call, which no \
         later check can recover because the node is not in the arena for \
         `Walk::reconciled` to notice missing. Got {} for:\n{source}",
        describe(&result)
    );
    assert!(
        calls[0].origin().as_str().contains(':'),
        "a region must be placed as well as named, got {} for:\n{source}",
        calls[0].origin()
    );
    assert!(
        holes_blamed_on(&result, "conditional-expression").is_empty(),
        "the `not` itself is no longer a region - if it still is, the operand \
         was traversed but the container kept its refusal, and the four stdlib \
         functions this is for go on being blamed on a construct that is not \
         their obstacle: {} for:\n{source}",
        describe(&result)
    );
}

/// **A call under a negation under a boolean is still named.**
///
/// `not (f(n) or g(n))` is a `Not` over a `BoolOp` over two calls. The `BoolOp`
/// arm of `expression_children` already exists, so this fails only if the `Not`
/// arm is missing - and it separates a fix that special-cased "`not` of a
/// call" from one that descends properly.
#[test]
fn a_call_under_a_negated_boolean_is_still_named() {
    let source = "def negate(n: int) -> int:\n    return not (f(n) or g(n))\n";
    let function = only_function(source);
    let result = cost(function.program());

    assert_eq!(
        holes_blamed_on(&result, "call").len(),
        2,
        "two calls are written and two regions must be reported. One means the \
         descent stopped at the `not`'s single operand instead of continuing \
         through it: {} for:\n{source}",
        describe(&result)
    );
}

/// **A bare negation statement names its call too.**
///
/// The other position with no value slot. `not f(n)` on a line of its own is
/// unusual Python, and it is here because the two positions are threaded by the
/// same `value_discarded` flag and a fix keyed on `Stmt::Return` alone would
/// pass every other test in this file.
#[test]
fn a_bare_negation_statement_names_its_call() {
    let source = "def negate(n: int) -> int:\n    not f(n)\n    return n\n";
    let function = only_function(source);
    let result = cost(function.program());

    assert_eq!(
        holes_blamed_on(&result, "call").len(),
        1,
        "a bare expression statement reads no value either, so the same rule \
         applies and the same call must be named: {} for:\n{source}",
        describe(&result)
    );
}

// ---------------------------------------------------------------------------
// 2. what the change buys, on code the fragment can read
// ---------------------------------------------------------------------------

/// **A returned negation over integers derives a bound and lowers.**
///
/// The real coverage claim, and the only one this widening makes. Each of these
/// is a single `return` costing a single step once the `not` stops being a
/// region: `not` has one operand, evaluates it exactly once and never
/// short-circuits, so unlike `and` and the ternary there is not even an
/// over-charge to argue about - the sum *is* the cost.
///
/// `lower` is asserted beside `is_complete` because they are different claims
/// and this widening has to make both. `Coverage::lowered()` counts transition
/// systems and is the headline number; a `not` in a discarded position binds
/// nothing, so nothing stops it reaching one.
#[test]
fn a_returned_negation_over_integers_is_fully_derived() {
    for source in [
        "def negate(n: int) -> int:\n    return not n\n",
        "def compared(a: int, b: int) -> int:\n    return not (a < b)\n",
        "def conjoined(a: int, b: int) -> int:\n    return not (a and b)\n",
        "def doubled(n: int) -> int:\n    return not not n\n",
        "def arithmetic(a: int, b: int) -> int:\n    return not (a + b)\n",
        "def bare(n: int) -> int:\n    not n\n    return n\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert!(
            result.is_complete(),
            "nothing here is unknown - the value is returned into a slot the \
             fragment never represents, and truth-testing it touches only \
             proven integers: {} for:\n{source}",
            describe(&result)
        );
        assert!(
            landav_its::lower(function.program()).is_ok(),
            "and it must reach a transition system, which is the headline \
             coverage number:\n{source}"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. the four stdlib shapes, and the honest thing to say about them
// ---------------------------------------------------------------------------

/// The four remaining `conditional-expression` sole-blockers, copied verbatim
/// from `/usr/lib/python3.12`.
///
/// They are the ticket's whole justification, so they are checked as written
/// rather than paraphrased - a paraphrase that dropped `_cmp`'s `try` or
/// `isdata`'s six disjuncts would be testing a shape the corpus does not
/// contain.
const STDLIB_FOUR: &str = r#"
def _cmp(a, b, sh, abs=abs, cmp=cmp):
    try:
        return not abs(cmp(a, b, sh))
    except OSError:
        return 2


def not_(a):
    "Same as not a."
    return not a


def isdata(object):
    """Check if an object is of a type that probably means it's data."""
    return not (inspect.ismodule(object) or inspect.isclass(object) or
                inspect.isroutine(object) or inspect.isframe(object) or
                inspect.istraceback(object) or inspect.iscode(object))


def _is_universal(mac):
    return not (mac & (1 << 41))
"#;

/// **Each of the four stops being blamed on `conditional-expression` and names
/// its real obstacle.**
///
/// This is deliberately *not* "the four now derive". They do not, and the
/// module documentation gives the measurement: replacing each `not` with an
/// `or 0` - a form discarded position already accepts - and reading the
/// operands shows two blocked by calls, one by an unannotated parameter and one
/// by a bitwise `&`. Writing a test that asserted they derive would have to be
/// deleted by the implementer, and deleting an acceptance test to make a ticket
/// pass is how a criterion quietly stops meaning anything.
///
/// What the change genuinely delivers is here: the sole-blocker count for
/// `conditional-expression` goes to zero on this corpus, and each of the four
/// starts pointing at something its author can act on. The permitted sets are
/// deliberately generous about *which* real construct - a later pass that also
/// descends into a refused call's callee would add `attribute` to `isdata`, and
/// that is an improvement this test must not forbid - and exact about the one
/// that must be gone.
#[test]
fn each_stdlib_negation_names_its_real_obstacle() {
    let expected: [(&str, &[&str]); 4] = [
        // `not abs(cmp(a, b, sh))`: two calls, of which the inner one is the
        // subject of `nested_calls.rs`. Either count is an improvement here;
        // what matters is that a call is named at all.
        ("_cmp", &["call"]),
        // `not a` with `a` unannotated. The obstacle is the signature, and
        // that is exactly what the report should say.
        ("not_", &["non-integer-value"]),
        // Six `inspect.*` predicates under an `or` chain.
        ("isdata", &["call", "attribute"]),
        // `mac & (1 << 41)`. `&` stays refused by the integer-ops lane's
        // verdict, and `mac` is unannotated besides.
        ("_is_universal", &["bitwise-operator", "non-integer-value"]),
    ];

    for function in functions_of(STDLIB_FOUR) {
        let name = function.name().to_owned();
        let Some((_, permitted)) = expected.iter().find(|(candidate, _)| *candidate == name) else {
            panic!("the fixture defined `{name}`, which this test says nothing about");
        };
        let result = cost(function.program());
        let named = constructs_blamed(&result);

        assert!(
            !named.contains(&"conditional-expression"),
            "`{name}` is still blamed on `conditional-expression`. The \
             construct in the way is {permitted:?}; a negation is not what \
             stops this function being analysed, and telling its author to go \
             and look at a conditional expression sends them to the wrong line. \
             Got {} ",
            describe(&result)
        );
        assert!(
            !named.is_empty(),
            "`{name}` reported no region at all. It is not analysable - it \
             calls out, or reads an unannotated parameter, or uses an operator \
             the fragment refuses - so a complete bound here would be a \
             fabrication, and it is exactly the fabrication that follows from \
             accepting the `not` without traversing its operand. Got {}",
            describe(&result)
        );
        assert!(
            named.iter().any(|construct| permitted.contains(construct)),
            "`{name}` must name one of {permitted:?} - the thing actually in \
             the way - and named {named:?} instead. Got {}",
            describe(&result)
        );
    }
}

// ---------------------------------------------------------------------------
// 4. the fences
// ---------------------------------------------------------------------------

/// **A negation bound to a name is still a region.**
///
/// The near half of the fence, and the same one
/// `discarded_expressions::a_boolean_in_value_position_stays_a_region` holds
/// for `and`. `Translator::refusals_of` hoists the right-hand side of
/// `x = not n` through the same scratch builder a `return` uses, so a widening
/// keyed on "am I in a scratch builder" rather than on "is there a value slot"
/// takes this with it.
///
/// Soundness survives that - `x` is condemned as `non-integer-value` whatever
/// happens to the `not`. The report does not: "`x` is not a proven integer"
/// without "because of the negation at 2:9" names a symptom and withholds the
/// cause.
#[test]
fn a_negation_in_value_position_stays_a_region() {
    for source in [
        "def bound(n: int) -> int:\n    x = not n\n    return x\n",
        "def compared(a: int, b: int) -> int:\n    x = not (a < b)\n    return x\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert!(
            !holes_blamed_on(&result, "conditional-expression").is_empty(),
            "a binding names a value, and the construct that made that value \
             unreadable has to be named beside it. If this is now empty, the \
             widening was keyed on the scratch builder rather than on there \
             being no value slot: {} for:\n{source}",
            describe(&result)
        );
    }
}

/// **Membership and identity under a negation keep their own names.**
///
/// `not (c in items)` runs `__contains__`, which is arbitrary user code with an
/// arbitrary cost, and `not (a is None)` compares identities the fragment has
/// no model for. Both are refused in every position, and one operator answers
/// to one construct wherever it is written - so the region here is `collection`
/// and `non-integer-value` respectively, never `conditional-expression`.
///
/// The construct name is the whole assertion, and it is the same one
/// `discarded_expressions::membership_and_identity_are_refused_wherever_they_
/// are_written` makes for the un-negated forms. Reporting these as
/// "conditional expression" tells the reader to look at a negation when what
/// the tool could not read is the container being searched.
#[test]
fn membership_and_identity_under_a_negation_keep_their_own_names() {
    for (source, expected) in [
        (
            "def member(c: int) -> int:\n    return not (c in \"0123456789\")\n",
            "collection",
        ),
        (
            "def identity(a: int) -> int:\n    return not (a is None)\n",
            "non-integer-value",
        ),
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert!(
            !holes_blamed_on(&result, expected).is_empty(),
            "the operator under the `not` decides the name, and `{expected}` is \
             what this one should be: {} for:\n{source}",
            describe(&result)
        );
        assert!(
            holes_blamed_on(&result, "conditional-expression").is_empty(),
            "and a negated membership test is not a conditional expression \
             used as a value: {} for:\n{source}",
            describe(&result)
        );
    }
}

/// **Condition position still works, and is untouched.**
///
/// A regression guard, passing today, and it earns its place because the change
/// this file asks for is in the same family of functions that serve conditions.
/// `build_condition` handles `Not` in full and `condition_children` already has
/// the arm; a widening that reroutes negation through the discarded-position
/// path would be a regression here, turning a function that derives an exact
/// bound into one carrying a region.
#[test]
fn a_negation_in_a_condition_still_derives() {
    for source in [
        "def guarded(n: int) -> int:\n    x = 0\n    if not n:\n        x = 1\n    return x\n",
        "def compared(a: int, b: int) -> int:\n    x = 0\n    if not (a < b):\n        x = 1\n    \
         return x\n",
    ] {
        let function = only_function(source);
        let result = cost(function.program());
        assert!(
            result.is_complete(),
            "`not` in a condition is a truth test the fragment has always \
             understood, and it must go on being one: {} for:\n{source}",
            describe(&result)
        );
        assert!(
            landav_its::lower(function.program()).is_ok(),
            "and it must go on lowering:\n{source}"
        );
    }
}

/// **`~` is not `not`, and stays refused.**
///
/// The two are one character apart in Python and one variant apart in the AST -
/// `ast::UnaryOp::Invert` sits beside `ast::UnaryOp::Not` in the same `match`,
/// and the arm being changed is that `match`. `~x` is a bitwise operator whose
/// result the fragment has no rule for, and the integer-ops lane's verdict
/// keeps `&`, `|`, `^` refused; `~` belongs with them.
///
/// It is also the operator inside `uuid._is_universal`'s sibling shapes, so a
/// widening that took it by accident would turn a refusal into a silent
/// acceptance on exactly the corpus this ticket cites.
#[test]
fn bitwise_inversion_is_not_swept_up_with_negation() {
    let source = "def inverted(n: int) -> int:\n    return ~n\n";
    let function = only_function(source);
    let result = cost(function.program());

    assert!(
        !holes_blamed_on(&result, "bitwise-operator").is_empty(),
        "`~` is a bitwise operator and must keep saying so. `not` and `Invert` \
         are adjacent arms of one `match`, and this is the test that fails if \
         the widening reached one variant too far: {} for:\n{source}",
        describe(&result)
    );
}
