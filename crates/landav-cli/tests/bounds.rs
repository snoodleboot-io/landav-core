//! `LAN-75` at the process boundary: **an exact bound must be distinguishable
//! from a cautious one.**
//!
//! # What is being defended
//!
//! The native engine reports whether a bound is an equality or only an upper
//! limit, and those are different claims. `Theta(n^2)` says the cost *is*
//! quadratic; `O(n^2)` says it is at most quadratic and may be far less. A user
//! who cannot tell them apart cannot tell an analysis that succeeded from one
//! that gave up politely, which is most of what this tool is for.
//!
//! "Distinguishable" is a property of the **process output**, not of a library
//! type, so every test here drives the built binary.
//!
//! # The silence problem
//!
//! A function the engine cannot bound must be *named* as unbounded rather than
//! omitted. An omitted function reads as a bound of zero - the most attractive
//! possible answer - which is the failure mode `LAN-68` established for the
//! coverage report and which applies here for the same reason.

mod common;

use std::io;

use common::{EXIT_CLEAN, Project};

/// One function of each shape the engine treats differently.
///
/// The `while` loop is not padding: it is the case the engine has no mechanism
/// for, and its line is what proves the tool says so rather than staying quiet.
const SHAPES_PY: &str = r"
def single(n: int) -> int:
    x = 0
    for i in range(n):
        x = i
    return x


def rectangular(n: int, m: int) -> int:
    x = 0
    for i in range(n):
        for j in range(m):
            x = j
    return x


def triangular(n: int) -> int:
    x = 0
    for i in range(n):
        for j in range(i):
            x = j
    return x


def unbounded(n: int) -> int:
    i = 0
    while i < n:
        i = i + 1
    return i
";

/// The flag is opt-in, so a run without it must look exactly as it did before.
///
/// Bound reporting is per function and would otherwise bury the findings a
/// default run exists to surface.
#[test]
fn bounds_are_not_printed_unless_asked_for() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("shapes.py", SHAPES_PY)?;

    let run = project.check(&target, &[])?;
    assert!(
        !run.mentions("Theta("),
        "a run without --bounds printed bounds: {}",
        run.describe()
    );
    Ok(())
}

/// A counted loop is derived **exactly**, and the output says so with a
/// quantifier rather than leaving the reader to infer it.
#[test]
fn an_exactly_derived_bound_is_reported_as_theta() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("shapes.py", SHAPES_PY)?;

    let run = project.check(&target, &["--bounds"])?;
    assert!(
        run.mentions("single: Theta("),
        "a counted loop must report an exact bound: {}",
        run.describe()
    );
    assert!(
        run.mentions("derived exactly"),
        "the exactness must be stated in words, not only in a symbol: {}",
        run.describe()
    );
    Ok(())
}

/// Triangular nesting is the case the summation was built for, and the one
/// where an approximation would be visibly different: `n^2` against the
/// `2n^2 + n` the engine reported before it could sum.
///
/// Asserting the shape rather than the exact string, because the bound
/// algebra's normal form is allowed to change and this test should not fail
/// for that.
#[test]
fn triangular_nesting_reports_a_quadratic_equality() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("shapes.py", SHAPES_PY)?;

    let run = project.check(&target, &["--bounds"])?;
    let line = run
        .output()
        .lines()
        .find(|line| line.contains("triangular:"))
        .map(str::to_owned)
        .unwrap_or_default();

    assert!(
        line.contains("Theta("),
        "triangular nesting is exact and must not be reported as a mere upper \
         bound: {line:?} in {}",
        run.describe()
    );
    assert!(
        line.contains("n * n"),
        "triangular nesting costs n^2; an approximation would show a larger \
         expression here: {line:?}",
    );
    Ok(())
}

/// The silence problem. A `while` loop has no mechanism in this engine, and the
/// function must be **named** as unbounded rather than left out.
#[test]
fn a_function_with_no_derivable_bound_is_named_rather_than_omitted() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("shapes.py", SHAPES_PY)?;

    let run = project.check(&target, &["--bounds"])?;
    assert!(
        run.mentions("unbounded:"),
        "a function the engine could not bound must still appear, or its \
         absence reads as a bound of zero: {}",
        run.describe()
    );
    assert!(
        run.mentions("no bound"),
        "the absence of a bound must be stated: {}",
        run.describe()
    );
    Ok(())
}

/// Every function that lowered gets a line. A report that covers three of four
/// functions and says nothing about the fourth is the coverage failure mode
/// wearing different clothes.
#[test]
fn every_function_gets_a_line() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("shapes.py", SHAPES_PY)?;

    let run = project.check(&target, &["--bounds"])?;
    for name in ["single", "rectangular", "triangular", "unbounded"] {
        assert!(
            run.mentions(&format!("{name}:")),
            "`{name}` had no line in the bounds report: {}",
            run.describe()
        );
    }
    Ok(())
}

/// Each line carries a position, so a bound in a large tree can be traced to
/// the function it describes.
#[test]
fn a_bound_names_where_its_function_is() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("shapes.py", SHAPES_PY)?;

    let run = project.check(&target, &["--bounds"])?;
    assert!(
        run.mentions("shapes.py:"),
        "a bound must be traceable to a position: {}",
        run.describe()
    );
    Ok(())
}

/// Reporting bounds is not a verdict. A clean file stays clean, because
/// deriving a bound says nothing about whether it breaches a budget - that is
/// what `--resource` and the configuration are for.
#[test]
fn asking_for_bounds_does_not_change_the_verdict() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("shapes.py", SHAPES_PY)?;

    let run = project.check(&target, &["--bounds"])?;
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "reporting a bound is not a finding: {}",
        run.describe()
    );
    Ok(())
}
