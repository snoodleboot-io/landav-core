//! `LAN-67`, end to end: Python source in, integer transition system out.
//!
//! The property suite in `landav-its` judges the lowering from the neutral
//! fragment onwards. This file judges the half that only this crate can do —
//! deciding what a piece of Python *means* in that fragment, and refusing when
//! it means something the fragment cannot say.
//!
//! Two kinds of assertion, and the second is the one that matters:
//!
//! * **it lowers** — the named Python construct produces a system, and
//!   simulating that system reproduces a trip count worked out by hand;
//! * **it refuses, by name** — the Python construct outside the fragment
//!   produces exactly the [`Construct`] it should, at the position it should.
//!
//! The simulator below is deliberately small and written from the operational
//! reading of a transition rather than from anything in the crate under test.

// See `common/mod.rs` for why the panic lints are relaxed in test code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{collections::BTreeMap, path::Path};

use landav_its::{Construct, Its, Relation, lower};
use landav_python::{LoweredFunction, lower_module};

/// A valuation of the system's variables.
type State = BTreeMap<String, i128>;

/// Translates `source` and returns its single function.
fn only_function(source: &str) -> LoweredFunction {
    let mut functions = lower_module(Path::new("case.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    assert_eq!(
        functions.len(),
        1,
        "expected exactly one function in:\n{source}"
    );
    functions.remove(0)
}

/// Translates and lowers `source`, expecting success.
fn its_of(source: &str) -> Its {
    let function = only_function(source);
    lower(function.program())
        .unwrap_or_else(|error| panic!("refused a program inside the fragment:\n{source}\n{error}"))
}

/// Translates and lowers `source`, expecting a refusal, and returns the
/// constructs it named.
fn refusal_of(source: &str) -> Vec<Construct> {
    let function = only_function(source);
    match lower(function.program()) {
        Ok(_) => panic!("expected a refusal, but this lowered:\n{source}"),
        Err(error) => error
            .refusals()
            .unwrap_or_else(|| panic!("expected a refusal, got: {error}"))
            .constructs(),
    }
}

/// A polynomial's value — written out from the definition, not taken from the
/// crate under test.
fn evaluate(polynomial: &landav_its::Polynomial, state: &State) -> Option<i128> {
    let mut total: i128 = 0;
    for monomial in polynomial.monomials() {
        let mut term = i128::from(monomial.coefficient());
        for (var, exponent) in monomial.powers() {
            let value = *state.get(var.as_str())?;
            for _ in 0..*exponent {
                term = term.checked_mul(value)?;
            }
        }
        total = total.checked_add(term)?;
    }
    Some(total)
}

/// Runs the system to its exit, requiring a unique successor at each step.
fn simulate(its: &Its, inputs: &[(&str, i128)]) -> State {
    let mut state: State = its
        .vars()
        .iter()
        .map(|var| (var.as_str().to_owned(), 0_i128))
        .collect();
    for (name, value) in inputs {
        state.insert((*name).to_owned(), *value);
    }

    let mut location = its.start();
    for _ in 0..200_000_u32 {
        if location == its.exit() {
            return state;
        }
        let mut successors = Vec::new();
        for transition in its.transitions_from(location) {
            let enabled = transition.guard().constraints().iter().all(|constraint| {
                let value = evaluate(constraint.polynomial(), &state)
                    .expect("the corpus stays inside i128");
                match constraint.relation() {
                    Relation::Ge => value >= 0,
                    Relation::Gt => value > 0,
                    Relation::Eq => value == 0,
                }
            });
            if !enabled {
                continue;
            }
            let mut next = state.clone();
            let mut computed = Vec::new();
            for (var, polynomial) in transition.update().assignments() {
                computed.push((
                    var.as_str().to_owned(),
                    evaluate(polynomial, &state).expect("the corpus stays inside i128"),
                ));
            }
            for (name, value) in computed {
                next.insert(name, value);
            }
            let candidate = (transition.target(), next);
            if !successors.contains(&candidate) {
                successors.push(candidate);
            }
        }
        match successors.len() {
            1 => {
                let (target, next) = successors.remove(0);
                location = target;
                state = next;
            }
            other => panic!("l{} offered {other} distinct successors", location.index()),
        }
    }
    panic!("the system did not reach its exit");
}

// ---------------------------------------------------------------------------
// it lowers, and it computes the right answer
// ---------------------------------------------------------------------------

/// A counted loop over `range(n)`, end to end from Python source.
#[test]
fn a_counted_loop_lowers_and_counts_correctly() {
    let its = its_of(
        "\
def count(n: int) -> int:
    total = 0
    for i in range(n):
        total = total + 1
    return total
",
    );
    // Worked out by hand: `range(5)` visits 0..4, so `total` ends at 5.
    let state = simulate(&its, &[("n", 5)]);
    assert_eq!(state.get("total"), Some(&5));
    assert_eq!(its.params().len(), 1, "n is the only integer parameter");
}

/// Augmented assignment expands to the arithmetic it stands for.
#[test]
fn augmented_assignment_expands() {
    let its = its_of(
        "\
def total(n: int) -> int:
    acc = 0
    for i in range(n):
        acc += 3
    return acc
",
    );
    // Three per iteration, four iterations.
    assert_eq!(simulate(&its, &[("n", 4)]).get("acc"), Some(&12));
}

/// `range(start, stop, step)` with a negative literal step counts down.
#[test]
fn a_negative_step_counts_down() {
    let its = its_of(
        "\
def down(n: int) -> int:
    seen = 0
    for i in range(n, 0, -2):
        seen += 1
    return seen
",
    );
    // n = 7: i visits 7, 5, 3, 1 — four iterations.
    assert_eq!(simulate(&its, &[("n", 7)]).get("seen"), Some(&4));
}

/// The KoAT worked example, written as the Python it stands for.
///
/// An outer loop over `n` and an inner one that doubles, so the body runs
/// `n * (log2(n) + 1)` times. Worked out by hand at `n = 8`: three inner steps
/// per outer pass, eight passes, twenty-four.
#[test]
fn the_koat_worked_example_lowers_from_python() {
    let its = its_of(
        "\
def worked(n: int) -> int:
    steps = 0
    for i in range(n):
        j = 1
        while j < n:
            j = j * 2
            steps += 1
    return steps
",
    );
    assert_eq!(simulate(&its, &[("n", 8)]).get("steps"), Some(&24));
}

/// A loop body that grows the endpoint does not lengthen the loop.
///
/// Python evaluates `range(n)` once. An ITS that re-read `n` would not
/// terminate at all.
#[test]
fn the_range_endpoint_is_evaluated_once() {
    let its = its_of(
        "\
def growing(n: int) -> int:
    for i in range(n):
        n += 10
    return n
",
    );
    // n = 3: three iterations, n ends at 33.
    assert_eq!(simulate(&its, &[("n", 3)]).get("n"), Some(&33));
}

/// Truthiness, chained comparisons and the connectives.
#[test]
fn conditions_lower_with_python_semantics() {
    let its = its_of(
        "\
def classify(n: int) -> int:
    result = 0
    if 0 < n < 10 and n != 5:
        result = 1
    elif not n:
        result = 2
    else:
        result = 3
    return result
",
    );
    // Worked out by hand: 3 is in (0, 10) and is not 5; 5 is excluded by the
    // second conjunct; 0 is falsy; 20 falls through.
    assert_eq!(simulate(&its, &[("n", 3)]).get("result"), Some(&1));
    assert_eq!(simulate(&its, &[("n", 5)]).get("result"), Some(&3));
    assert_eq!(simulate(&its, &[("n", 0)]).get("result"), Some(&2));
    assert_eq!(simulate(&its, &[("n", 20)]).get("result"), Some(&3));
}

/// A docstring is documentation, not a value.
///
/// Treating it as a string constant would refuse every documented function in
/// the corpus, which would make the coverage number a measure of docstrings.
#[test]
fn a_docstring_does_not_refuse() {
    let its = its_of(
        "\
def documented(n: int) -> int:
    \"\"\"Adds one to n.\"\"\"
    return n + 1
",
    );
    assert!(!its.transitions().is_empty());
}

/// `True` and `False` are `1` and `0`, exactly.
#[test]
fn booleans_are_integers() {
    let its = its_of(
        "\
def flag(n: int) -> int:
    x = True
    y = False
    return x + y
",
    );
    let state = simulate(&its, &[("n", 0)]);
    assert_eq!(state.get("x"), Some(&1));
    assert_eq!(state.get("y"), Some(&0));
}

/// Every top-level function is translated, including ones that will refuse.
///
/// Returning only the functions that happen to lower would throw away the
/// answer `LAN-68`'s coverage report is built from.
#[test]
fn every_top_level_function_is_translated() {
    let functions = lower_module(
        Path::new("many.py"),
        "\
def a(n: int) -> int:
    return n

def b(n: int) -> int:
    return sorted(n)

def c(n: int) -> int:
    return n * n
",
    )
    .expect("parses");
    assert_eq!(functions.len(), 3);
    let names: Vec<&str> = functions.iter().map(LoweredFunction::name).collect();
    assert_eq!(names, ["a", "b", "c"]);
    assert!(lower(functions[0].program()).is_ok());
    assert!(lower(functions[1].program()).is_err(), "b calls sorted");
    assert!(lower(functions[2].program()).is_ok());
}

// ---------------------------------------------------------------------------
// it refuses, by name
// ---------------------------------------------------------------------------

/// **The table of refusals.** Each snippet names the construct it must produce.
///
/// Driven off a table so that adding a Python form means adding a row, and so
/// that a construct silently changing which refusal it produces is a failure
/// rather than a surprise later.
#[test]
fn python_constructs_outside_the_fragment_refuse_by_name() {
    let cases: &[(&str, Construct, &str)] = &[
        ("items = [1, 2, 3]", Construct::Collection, "a list literal"),
        ("x = n[0]", Construct::Subscript, "an index"),
        ("x = n.bit_length", Construct::Attribute, "an attribute"),
        ("x = len(n)", Construct::Call, "a call"),
        (
            "x = [i for i in range(n)]",
            Construct::Comprehension,
            "a comprehension",
        ),
        ("x = n / 2", Construct::IntegerDivision, "true division"),
        ("x = n // 2", Construct::IntegerDivision, "floor division"),
        ("x = n % 2", Construct::IntegerDivision, "modulo"),
        ("x = n << 1", Construct::BitwiseOperator, "a shift"),
        ("x = n & 1", Construct::BitwiseOperator, "a bitwise and"),
        (
            "x = n ** n",
            Construct::NonPolynomialPower,
            "a symbolic exponent",
        ),
        (
            "x = 1 if n else 2",
            Construct::ConditionalExpression,
            "a conditional expression",
        ),
        (
            "x = (n > 1)",
            Construct::ConditionalExpression,
            "a comparison as a value",
        ),
        (
            "for q in [1, 2]:\n        pass",
            Construct::UnboundedIteration,
            "iterating a list",
        ),
        (
            "for q in range(0, n, n):\n        pass",
            Construct::UnboundedIteration,
            "a symbolic step",
        ),
        ("while n:\n        break", Construct::LoopJump, "break"),
        (
            "while n:\n        continue",
            Construct::LoopJump,
            "continue",
        ),
        (
            "try:\n        pass\n    except ValueError:\n        pass",
            Construct::ExceptionalControlFlow,
            "try",
        ),
        (
            "raise ValueError",
            Construct::ExceptionalControlFlow,
            "raise",
        ),
        (
            "with open('f') as handle:\n        pass",
            Construct::ExceptionalControlFlow,
            "with",
        ),
        ("assert n > 0", Construct::ExceptionalControlFlow, "assert"),
        ("import os", Construct::Declaration, "an import"),
        (
            "def inner():\n        pass",
            Construct::Declaration,
            "a nested def",
        ),
        ("x = lambda: 1", Construct::Declaration, "a lambda"),
        ("global counter", Construct::BindingForm, "global"),
        ("del n", Construct::BindingForm, "del"),
        (
            "a, b = 1, 2",
            Construct::ComplexAssignmentTarget,
            "tuple unpacking",
        ),
        ("x = yield n", Construct::Coroutine, "yield"),
        ("x = 'text'", Construct::Collection, "a string"),
        // Position matters, and both positions must refuse. In a *value*
        // position the comparison itself is out of the fragment, so it refuses
        // as a comparison-as-value before `in` is ever reached; in a condition
        // position `in` is what is refused.
        (
            "x = (n in [1])",
            Construct::ConditionalExpression,
            "membership as a value",
        ),
        (
            "if n in [1]:\n        pass",
            Construct::Collection,
            "membership in a condition",
        ),
        (
            "if n is None:\n        pass",
            Construct::NonIntegerValue,
            "identity in a condition",
        ),
    ];

    for (body, expected, description) in cases {
        let source = format!("def case(n: int) -> int:\n    {body}\n    return 0\n");
        let named = refusal_of(&source);
        assert!(
            named.contains(expected),
            "{description} should refuse as {expected}, but named {named:?}\n{source}"
        );
    }
}

/// A parameter without an `int` annotation poisons every read of it.
///
/// "Typed Python" means proved, not hoped. Treating an unannotated parameter as
/// an integer is the shortest path to an unsound bound.
#[test]
fn an_unannotated_parameter_refuses_at_its_use() {
    let named = refusal_of(
        "\
def untyped(n):
    total = 0
    for i in range(n):
        total += 1
    return total
",
    );
    assert!(
        named.contains(&Construct::NonIntegerValue),
        "an unannotated parameter must not be assumed integral: {named:?}"
    );
}

/// A `float` annotation is not an `int` annotation.
#[test]
fn a_float_parameter_refuses() {
    let named = refusal_of(
        "\
def scaled(n: float) -> float:
    return n + 1
",
    );
    assert!(named.contains(&Construct::NonIntegerValue));
}

/// A local assigned a non-integer anywhere is a non-integer everywhere.
///
/// The type pass is a least fixed point, so a single bad assignment demotes the
/// name even where an earlier one looked fine — which is the sound direction.
#[test]
fn one_non_integer_assignment_demotes_a_local_everywhere() {
    let named = refusal_of(
        "\
def mixed(n: int) -> int:
    acc = 0
    for i in range(n):
        acc = acc + 1
    acc = 'done'
    return acc
",
    );
    assert!(
        named.contains(&Construct::NonIntegerValue) || named.contains(&Construct::Collection),
        "a name assigned a string is not an integer: {named:?}"
    );
}

/// **The orphan case.** `return f()` refuses, even though the fragment's
/// `return` carries no value.
///
/// This is the case that forces `landav_its::lower` to scan the arenas rather
/// than trust the traversal. The call has an unknown cost and must be refused,
/// but the node the translation builds for it has no parent. Before the scan,
/// this program lowered cleanly and the bound silently omitted the call.
#[test]
fn a_call_in_a_return_is_still_refused() {
    let named = refusal_of(
        "\
def delegating(n: int) -> int:
    return helper(n)
",
    );
    assert!(
        named.contains(&Construct::Call),
        "a call in a return position was dropped: {named:?}"
    );
}

/// The same trap, for a bare expression statement.
#[test]
fn a_bare_call_statement_is_refused() {
    let named = refusal_of(
        "\
def side_effecting(n: int) -> int:
    log(n)
    return n
",
    );
    assert!(named.contains(&Construct::Call), "{named:?}");
}

/// A refusal names the callee, not just "a call".
#[test]
fn a_refused_call_names_the_callee() {
    let function = only_function(
        "\
def delegating(n: int) -> int:
    return expensive_helper(n)
",
    );
    let error = lower(function.program()).expect_err("a call refuses");
    let rendered = error.to_string();
    assert!(
        rendered.contains("expensive_helper"),
        "the refusal does not name the callee: {rendered}"
    );
    assert!(
        rendered.contains("case.py:2"),
        "the refusal does not name the position: {rendered}"
    );
}

/// An integer literal too large for the fragment refuses rather than wrapping.
///
/// Python integers are unbounded; the fragment's coefficients are `i64`.
/// Truncating would change the program.
#[test]
fn an_enormous_literal_refuses() {
    let named = refusal_of(
        "\
def huge(n: int) -> int:
    x = 99999999999999999999999999999999
    return x
",
    );
    assert!(named.contains(&Construct::ArithmeticOverflow), "{named:?}");
}

/// A syntax error is a parse error, not a refusal.
///
/// "This file is not Python" and "this function uses a dict" need different
/// responses, so they are different kinds of failure.
#[test]
fn a_syntax_error_is_reported_as_a_parse_error() {
    let error = lower_module(Path::new("broken.py"), "def (:\n")
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(error.starts_with("broken.py:1:"), "{error}");
}

/// Deeply nested input does not overflow the stack during translation.
///
/// Non-negotiable 2. The frontend's byte-level guard rejects the truly extreme
/// cases before parsing, but everything under the cap still has to be
/// translated without recursion.
#[test]
fn deep_expressions_translate_without_overflowing() {
    let chain = " + 1".repeat(2_000);
    let source = format!("def deep(n: int) -> int:\n    x = 1{chain}\n    return x\n");
    let its = its_of(&source);
    // 1 plus two thousand ones.
    assert_eq!(simulate(&its, &[("n", 0)]).get("x"), Some(&2_001));
}
