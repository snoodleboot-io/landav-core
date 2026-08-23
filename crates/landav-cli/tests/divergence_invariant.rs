//! `LAN-91`: **what a run may claim about a function the toolchain refused.**
//!
//! # The invariant, and why `lowered` stopped being the right test for it
//!
//! A function that did not lower may not be reported with a complete bound -
//! `Theta` or `O` - unless the engine can be shown to have seen everything the
//! lowering refused. `LAN-87` implemented that as "did it lower", which was
//! right while every refusal was an `Unsupported` node the engine charged as a
//! hole: "did not lower" and "the engine is missing something" coincided.
//!
//! `LAN-91` broke the coincidence in both directions, and this file pins both.
//!
//! * A refusal carrying `bounded_by` - `n // 2`, a `set` display whose equal
//!   elements collapse - **is** a node in the arena. The engine reads the
//!   expression that dominates it and reports a sound one-sided bound.
//!   Withholding that was pure loss, and it is why the `integer-division` and
//!   `bitwise-operator` sole-blocker counts did not move when those constructs
//!   were implemented: the work landed and the report suppressed it.
//! * `Construct::PolynomialDegree` and `Construct::PolynomialSize` are raised by
//!   the **lowering**, from limits on the representation it emits into, and
//!   leave no node in the arena at all. The engine is not wrong about those
//!   programs - `x = a * b * ... * i` really does cost one step - but the run
//!   holds two answers that disagree about whether the function was analysed,
//!   and resolving that in favour of the stronger one is how an over-claim
//!   ships.
//!
//! The tests below are a matched pair on purpose. A change that makes the first
//! pass by relaxing the invariant wholesale breaks the second, which is the
//! outcome this file exists to prevent.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{path::Path, process::Command};

/// Runs the built binary over `source` and returns `--bounds` output.
fn bounds_of(name: &str, source: &str) -> String {
    let dir = std::env::temp_dir().join(format!("landav-divergence-{name}"));
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let path = dir.join("subject.py");
    std::fs::write(&path, source).expect("write source");

    let binary = Path::new(env!("CARGO_BIN_EXE_landav"));
    let output = Command::new(binary)
        .arg("check")
        .arg(&path)
        .arg("--bounds")
        .output()
        .expect("run landav");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// **A refusal the engine could see may still carry a complete bound.**
///
/// `n // 2` has no exact representation in the bound algebra - there is no
/// division constructor - so the frontend records it as a refusal that names the
/// dividend as dominating it. `landav_its::lower` still refuses the function,
/// because the transition system cannot represent the quotient either. But the
/// engine read the dividend and derived `O(1 + 2n)`, which is sound: the loop
/// runs `n / 2` times and `n` dominates that.
///
/// Reporting nothing here was the failure mode this test exists to catch. It is
/// not merely unhelpful - it made the `integer-division` work invisible, so the
/// sole-blocker count it was measured by did not move and the implementation
/// looked like it had failed when the reporting layer was suppressing it.
#[test]
fn a_refusal_the_engine_could_read_still_reports_its_bound() {
    let shown = bounds_of(
        "readable",
        "\
def halves(n: int) -> int:
    for i in range(n // 2):
        x = i
    return 0
",
    );

    assert!(
        shown.contains("halves: O("),
        "a quotient endpoint is bounded by its dividend and the engine derived \
         that soundly, so the run must report it as a one-sided bound. Got:\n{shown}"
    );
    assert!(
        !shown.contains("halves: Theta("),
        "and it must NOT be two-sided: the loop runs n/2 times while the bound \
         is written over n, so an equality would be a claim the program does \
         not meet. Got:\n{shown}"
    );
    assert!(
        !shown.contains("halves: no bound"),
        "withholding it entirely is the regression this test exists for. Got:\n{shown}"
    );
}

/// **The divisor never appears in the bound.**
///
/// Division is anti-monotone in its second operand while every `Bound` is weakly
/// monotone, so a bound mentioning the divisor is at its *minimum* exactly where
/// the true quotient is at its *maximum*. At `m = 1`, `n // m` is `n`; a bound
/// written over `m` would evaluate to 1.
#[test]
fn a_quotient_bound_never_mentions_the_divisor() {
    let shown = bounds_of(
        "divisor",
        "\
def split(n: int, m: int) -> int:
    for i in range(n // m):
        x = i
    return 0
",
    );

    let line = shown
        .lines()
        .find(|line| line.contains("split:"))
        .unwrap_or_else(|| panic!("no line for `split` in:\n{shown}"));
    // The rendered line carries a path and prose either side of the bound, both
    // of which contain the letter this test is looking for. Only the expression
    // itself is evidence.
    let start = line.find("O(").unwrap_or_else(|| {
        panic!("`split` did not report a one-sided bound: {line}");
    });
    let expression = &line[start + 1..line.rfind(')').expect("a closing bracket") + 1];
    let names: Vec<&str> = expression
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|token| token.starts_with(|c: char| c.is_alphabetic()))
        .collect();

    assert!(
        !names.contains(&"m"),
        "the bound {expression} mentions the divisor. Division is anti-monotone \
         in its second operand while every Bound is weakly monotone, so at m = 1 \
         the quotient is n while a bound over m evaluates to 1 - a claim the \
         program exceeds."
    );
    assert!(
        names.contains(&"n"),
        "the bound {expression} must still be a function of the dividend, or it \
         says nothing about how this loop scales."
    );
}

/// **A refusal the engine could NOT see still withholds the bound.**
///
/// The other half of the pair, and the reason the fix is a join rather than a
/// relaxation. `landav_its::Construct::PolynomialDegree` is raised while the
/// lowering builds its own representation; there is no node in the source arena,
/// so the engine never learns the function was refused and happily reports
/// `Theta(2)` - which is true about the cost and wrong about the run.
///
/// If this test fails, the invariant has been relaxed wholesale rather than
/// narrowed, and `x = a * b * ... * i` is being reported as fully analysed by a
/// toolchain that refused it.
#[test]
fn a_refusal_the_engine_could_not_see_still_withholds_the_bound() {
    let shown = bounds_of(
        "invisible",
        "\
def wide(a: int, b: int, c: int, d: int, e: int, f: int, g: int, h: int, i: int) -> int:
    x = a * b * c * d * e * f * g * h * i
    return 0
",
    );

    assert!(
        shown.contains("wide: no bound"),
        "the polynomial degree limit is raised by the lowering and leaves no \
         node in the arena, so the engine cannot know the function was refused. \
         Reporting its bound would resolve two disagreeing answers in favour of \
         the stronger one. Got:\n{shown}"
    );
}

/// **A statement the engine models fully still reports, even though it refuses.**
///
/// `raise NotImplementedError` is the third shape, and the most common of the
/// three in real code: an abstract-method stub. `landav_its::SourceStmt::Raise`
/// is a real statement with a real cost rule - one step, and an early exit - and
/// the engine walks and charges it. `landav_its::lower` refuses it anyway,
/// because a transition system's `Update` is a total map with no havoc and
/// cannot express a body abandoned partway through.
///
/// So the toolchain refused the function while the engine saw everything there
/// was to see, and `Exact(1)` is simply the truth. Withholding it cost nine
/// functions across the two measured corpora, every one of them a stub of this
/// shape.
#[test]
fn a_statement_the_engine_models_fully_still_reports_its_bound() {
    let shown = bounds_of(
        "stub",
        "\
def unimplemented(n: int) -> int:
    raise NotImplementedError
",
    );

    assert!(
        shown.contains("unimplemented: Theta(1)"),
        "a body that is one `raise` costs exactly one step, and the engine has \
         the whole program in front of it. Got:\n{shown}"
    );
}

/// **A set display is an upper bound, never an equality.**
///
/// `{1, 1, 2}` writes three elements and iterates two, because equal elements
/// collapse. Counting three is a sound `O` and a false `Theta`, and the
/// difference is exactly the distinction this project treats as load-bearing.
#[test]
fn a_set_display_is_counted_as_an_upper_bound() {
    let shown = bounds_of(
        "display",
        "\
def over_a_set(n: int) -> int:
    for x in {1, 1, 2}:
        y = 1
    return 0
",
    );

    assert!(
        shown.contains("over_a_set: O("),
        "equal elements collapse, so the element count dominates the iteration \
         count rather than equalling it. Got:\n{shown}"
    );
    assert!(
        !shown.contains("over_a_set: Theta("),
        "an equality here claims three iterations for a loop that runs twice. \
         Got:\n{shown}"
    );
}
