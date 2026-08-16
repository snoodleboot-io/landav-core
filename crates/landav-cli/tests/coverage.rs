//! `LAN-68` at the process boundary: **a partly-lowered file must be
//! impossible to mistake for a fully-analysed one.**
//!
//! # What is being defended
//!
//! A Python construct landav cannot lower produces **no transition**. An
//! integer transition system missing a transition admits fewer executions than
//! the program has, so a bound derived from it can be exceeded — the analysis
//! is unsound *by omission*, and on the terminal it looks exactly like a clean
//! result.
//!
//! The whole defence is that the refusal is visible, and "visible" is a
//! property of the **process output**, not of a library type. So every test
//! here drives the built binary, exactly as `LAN-61`'s suite does, and asserts
//! against what an operator reads and what CI branches on.
//!
//! # The three questions a user must be able to answer
//!
//! `LAN-68` says a user must be told **what** was out of scope, **where**, and
//! **what it means for the result**. Each has tests below, plus the two
//! decisions this lane had to make deliberately:
//!
//! * the coverage ratio is on the summary line of **every** run, including the
//!   runs with nothing to report — the same argument `LAN-66` makes for the
//!   suppression counts, that a number nobody can see change is a number
//!   nobody watches;
//! * asking about the lowering (`--coverage`) and getting a partial answer is
//!   [`Outcome::Inconclusive`], never clean — the same argument `LAN-60` makes
//!   for `--resource`.

mod common;

use std::io;

use common::{CLEAN_PY, EXIT_CLEAN, EXIT_FINDINGS, Project};

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

/// A function entirely inside the numeric fragment: annotated integer
/// parameter, a counted `range` loop, integer arithmetic.
///
/// This is the control. Every assertion about a *partial* run below is only
/// worth something if a complete run is distinguishable, and this is the file
/// that has to come out complete.
const LOWERS_PY: &str = r"
def steps(n: int) -> int:
    total = 0
    for i in range(n):
        total = total + i
    return total
";

/// A function that leaves the fragment in one specific, nameable way:
/// `sorted(...)` is a call, and a call has an unknown effect on the integer
/// state as well as an unknown value.
///
/// Deliberately *one* refusal. The call sits in the `return` rather than in
/// the accumulator, so it cannot also make `total` a non-integer and turn one
/// construct into two — the tests below are about the report, and a fixture
/// that refuses two different things at once would let a wrong count pass.
const REFUSES_PY: &str = r"
def ordered(n: int) -> int:
    total = 0
    for i in range(n):
        total = total + i
    return sorted(n)
";

/// One file, one function that lowers and one that does not.
///
/// The exact shape the story is about: half the file analysed, half not. A
/// report that names only the half that worked is the omission.
const HALF_PY: &str = r"
def good(n: int) -> int:
    total = 0
    for i in range(n):
        total = total + i
    return total


def bad(n: int) -> int:
    return sorted(n)
";

/// A module holding statements but no `def` at all.
const NO_FUNCTION_PY: &str = r"
VALUE = 1
OTHER = VALUE + 1
";

/// The construct tag `sorted(...)` must be reported under. Written out rather
/// than imported, for the same reason the exit codes are: this is a
/// machine-readable diagnostic code that reports group by and baselines pin,
/// so a rename must fail a test rather than pass one.
const CALL_TAG: &str = "call";

// ---------------------------------------------------------------------------
// the bar: a partial run cannot read as a whole one
// ---------------------------------------------------------------------------

/// **A file whose analysis refused a construct is not reported as clean.**
///
/// The central assertion of the story. `--coverage` asks how much of the file
/// became a transition system; when the answer is "not all of it", exit `0` —
/// "analysis ran and every bound held" — is a claim about code no transition
/// was ever emitted for.
#[test]
fn a_partly_lowered_file_is_not_reported_as_clean() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("half.py", HALF_PY)?;

    let run = project.check(&target, &["--coverage"])?;

    run.assert_did_not_crash();
    run.assert_code_is_sanctioned();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "a file with a refused construct exited clean, so a reader cannot tell \
         it from a fully-analysed one.\n{}",
        run.describe()
    );
    assert_eq!(
        run.code,
        EXIT_FINDINGS,
        "a refusal is a result about the code, so it belongs with the other \
         findings and not with `the tool could not look`.\n{}",
        run.describe()
    );
    Ok(())
}

/// **A file entirely inside the fragment is still clean.**
///
/// The control for the test above. Without it, "not clean" could be satisfied
/// by a `--coverage` flag that fails everything, which would be a gate nobody
/// keeps switched on.
#[test]
fn a_fully_lowered_file_is_still_clean() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("lowers.py", LOWERS_PY)?;

    let run = project.check(&target, &["--coverage"])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a file that lowered completely must not be failed by asking about \
         coverage.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("1 of 1"),
        "the complete run must still state the ratio.\n{}",
        run.describe()
    );
    Ok(())
}

/// **The two runs are visibly different.**
///
/// The story's bar, stated directly: a reader holding the output of a
/// partly-lowered file and the output of a fully-lowered one must be able to
/// tell which is which. Identical output would satisfy every other assertion
/// here and still fail the story.
#[test]
fn a_partial_run_reads_differently_from_a_complete_one() -> io::Result<()> {
    let project = Project::new()?;
    let whole = project.write("lowers.py", LOWERS_PY)?;
    let partial = project.write("half.py", HALF_PY)?;

    let complete = project.check(&whole, &["--coverage"])?;
    let incomplete = project.check(&partial, &["--coverage"])?;

    complete.assert_did_not_crash();
    incomplete.assert_did_not_crash();
    assert_ne!(
        complete.code,
        incomplete.code,
        "the two runs are indistinguishable to CI.\n{}\n{}",
        complete.describe(),
        incomplete.describe()
    );
    assert!(
        incomplete.mentions("1 of 2"),
        "the partial run does not say how much of the file it covered.\n{}",
        incomplete.describe()
    );
    assert!(
        !complete.mentions("out of scope"),
        "the complete run reports something as out of scope.\n{}",
        complete.describe()
    );
    assert!(
        incomplete.mentions("out of scope"),
        "the partial run does not say anything was out of scope.\n{}",
        incomplete.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// what, where, and what it means
// ---------------------------------------------------------------------------

/// **The report names the construct, the position, and the consequence.**
///
/// Three separate assertions because they fail separately: a report can name a
/// construct with no position (send the reader to grep), or a position with no
/// consequence (a list of complaints nobody has a reason to act on).
#[test]
fn the_report_says_what_where_and_what_it_means() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("refuses.py", REFUSES_PY)?;

    let run = project.check(&target, &["--coverage"])?;

    run.assert_did_not_crash();
    assert!(
        run.mentions(CALL_TAG),
        "the report does not name what was out of scope.\n{}",
        run.describe()
    );
    assert!(
        run.output()
            .lines()
            .any(|line| line.contains("refuses.py:6:") && line.contains(CALL_TAG)),
        "the report does not say where the refused construct is.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("no transition system") && run.mentions("no bound"),
        "the report does not say what a refusal means for the result.\n{}",
        run.describe()
    );
    assert!(
        !run.mentions("unknown"),
        "the report falls back on a bare `unknown`.\n{}",
        run.describe()
    );
    Ok(())
}

/// **The report lists the constructs that were never met.**
///
/// The half of a coverage report that says what the number is out of.
/// `Construct::all()` is published for exactly this, and without it a reader
/// cannot tell "we never met a comprehension here" from "we have no name for
/// comprehensions".
#[test]
fn the_report_lists_the_constructs_that_were_never_met() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("refuses.py", REFUSES_PY)?;

    let run = project.check(&target, &["--coverage"])?;

    run.assert_did_not_crash();
    assert!(
        run.mentions("never met"),
        "the report does not list the vocabulary it did not meet.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("comprehension") && run.mentions("pattern-match"),
        "constructs the run never met are missing from the report.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// aggregation across a run
// ---------------------------------------------------------------------------

/// **One construct refused in many places is reported as a count.**
///
/// One refusal is a footnote; the same construct four hundred times across a
/// tree is the headline and the next thing to implement. That only reads off a
/// report that aggregates, so the count has to survive the walk over files.
#[test]
fn refusals_are_aggregated_across_files() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/one.py", REFUSES_PY)?;
    project.write("src/two.py", REFUSES_PY)?;
    project.write("src/three.py", HALF_PY)?;
    let target = project.root().join("src");

    let run = project.check(&target, &["--coverage"])?;

    run.assert_did_not_crash();
    assert!(
        run.mentions("1 of 4"),
        "the run does not state the ratio over the whole tree.\n{}",
        run.describe()
    );
    assert!(
        run.output()
            .lines()
            .any(|line| line.contains(CALL_TAG) && line.contains("×3")),
        "the same construct refused in three files is not aggregated into one \
         count.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("one.py") && run.mentions("two.py") && run.mentions("three.py"),
        "not every file that refused something is named.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// the default run
// ---------------------------------------------------------------------------

/// **Every run's summary carries the coverage ratio, flag or no flag.**
///
/// The summary line is the one line a CI log reliably keeps. A run whose
/// summary says only "3 files analysed" invites exactly the reading this story
/// forbids, and a clause that appears only when something went wrong is a
/// clause nobody has a baseline for.
#[test]
fn the_summary_carries_the_coverage_ratio_on_every_run() -> io::Result<()> {
    let project = Project::new()?;
    let whole = project.write("lowers.py", LOWERS_PY)?;
    let partial = project.write("half.py", HALF_PY)?;

    let summary = |run: &common::Run| {
        run.stdout
            .lines()
            .find(|line| line.starts_with("landav:"))
            .map(str::to_owned)
            .unwrap_or_default()
    };

    let complete = project.check(&whole, &[])?;
    let incomplete = project.check(&partial, &[])?;

    assert!(
        summary(&complete).contains("coverage: 1 of 1"),
        "a complete run's summary does not state the ratio.\n{}",
        complete.describe()
    );
    assert!(
        summary(&incomplete).contains("coverage: 1 of 2"),
        "a partial run's summary does not state the ratio.\n{}",
        incomplete.describe()
    );
    Ok(())
}

/// **The default run names the dominant construct without being asked.**
///
/// A ratio alone says how much was lost, not what took it. The most frequent
/// construct is the actionable half and it costs one clause.
#[test]
fn the_default_run_names_the_dominant_construct() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("half.py", HALF_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert!(
        run.mentions(CALL_TAG),
        "the default run does not say what stopped the lowering.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("--coverage"),
        "the default run does not say where the detail is.\n{}",
        run.describe()
    );
    Ok(())
}

/// **The default run's exit code is unchanged by a refusal.**
///
/// Pinned deliberately, so that changing it is a decision somebody makes
/// rather than a side effect. At M0 no bound is derived from the lowering, so
/// the default verdict does not rest on it and failing every real Python file
/// on a milestone limitation would produce a gate that gets switched off. The
/// escalation is real and it is reached by asking about the lowering — see
/// [`a_partly_lowered_file_is_not_reported_as_clean`]. When bound inference
/// lands, this test is the one that has to change.
#[test]
fn a_refusal_alone_does_not_change_the_default_exit_code() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "asking nothing about the lowering must not fail the run.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("coverage:"),
        "but it must still be reported.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// nothing to lower
// ---------------------------------------------------------------------------

/// **A file with no function is not reported as partly lowered.**
///
/// Zero of zero is not a failure, and a report that treated it as one would
/// fail every `__init__.py` in a tree — after which nobody would pass
/// `--coverage`.
#[test]
fn a_file_with_no_function_is_not_partly_lowered() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("constants.py", NO_FUNCTION_PY)?;

    let run = project.check(&target, &["--coverage"])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a file holding no function refused nothing.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("no function"),
        "the run must say there was nothing to lower rather than printing a \
         ratio with no denominator.\n{}",
        run.describe()
    );
    assert!(
        !run.mentions("100%"),
        "a run that lowered nothing must not claim complete coverage.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// the contract around it
// ---------------------------------------------------------------------------

/// **`--coverage` is advertised, and says what it does.**
#[test]
fn the_flag_is_documented_in_help() -> io::Result<()> {
    let project = Project::new()?;

    let run = project.run(&["check", "--help"])?;

    run.assert_did_not_crash();
    assert!(
        run.mentions("--coverage"),
        "the flag is not advertised.\n{}",
        run.describe()
    );
    Ok(())
}

/// **Nothing in this feature can crash the tool or produce a fourth exit
/// code.**
///
/// Non-negotiable 2, applied to the paths this lane added: a coverage report
/// is built from untrusted source by way of a frontend, and a panic carries no
/// blame at all.
#[test]
fn a_coverage_run_never_crashes_and_never_leaves_the_contract() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/lowers.py", LOWERS_PY)?;
    project.write("src/half.py", HALF_PY)?;
    project.write("src/constants.py", NO_FUNCTION_PY)?;
    project.write("src/python2.py", common::UNREADABLE_AS_PYTHON_PY)?;
    project.write("src/empty.py", "")?;
    let target = project.root().join("src");

    for extra in [
        vec!["--coverage"],
        vec![],
        vec!["--coverage", "--resource", "ops"],
    ] {
        let run = project.check(&target, &extra)?;
        run.assert_did_not_crash();
        run.assert_code_is_sanctioned();
    }
    Ok(())
}

/// **A file the frontend cannot parse is not counted as a refused construct.**
///
/// Two different failures with two different owners. Folding an unparsable
/// file into the construct counts would send somebody to write a lowering rule
/// for a construct that was never there, and would inflate the number the team
/// uses to decide what to build next.
#[test]
fn an_unparsable_file_is_not_a_refused_construct() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("python2.py", common::UNREADABLE_AS_PYTHON_PY)?;

    let run = project.check(&target, &["--coverage"])?;

    run.assert_did_not_crash();
    assert!(
        run.mentions("no function"),
        "a file that never parsed offered no unit to lower.\n{}",
        run.describe()
    );
    assert!(
        !run.mentions("out of scope, by construct"),
        "an unparsable file was filed under a language construct.\n{}",
        run.describe()
    );
    Ok(())
}
