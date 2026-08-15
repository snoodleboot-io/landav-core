//! LAN-61 criterion 3: every analysis outcome maps to exactly one exit code.
//!
//! This criterion was added after a design review found that a *partial*
//! result — analysed, but no conclusion reached — had no exit code of its own
//! and would fall through to `0`. That reports "clean" for a file about which
//! nothing was proven, which is the one failure mode the product cannot have:
//! a green build is a claim, and a claim nobody checked is worse than a red
//! build.
//!
//! `landav_bound::Verdict` already names three outcomes — `Proved`, `Partial`
//! and `Unreachable` — against two "everything is fine" codes, so the mapping
//! is not the identity and cannot be left implicit.
//!
//! # Which route to `Inconclusive` these tests exercise, and which they cannot
//!
//! There are two ways a run can end up with no conclusion about a file, and
//! they are not the same fact:
//!
//! 1. **The frontend could not read it as Python.** A Python 2 module, a
//!    template, a file using syntax the pinned parser predates. The unit was
//!    never analysed. This is `landav_python::PythonError::Parse`, and it is
//!    the route every test below takes.
//! 2. **The frontend read it, the engine analysed it, and could not discharge
//!    every assumption** — a partial bound naming the term it could not
//!    account for. This is `Verdict::Partial`, and it is the case criterion 3
//!    was actually written about after the design review.
//!
//! **Route 2 has no test here, because at M0 it has no implementation.**
//! Producing a partial bound requires bound inference (`landav-its` →
//! `landav-solvers`), which is not built. The previous fixture — a `while`
//! loop whose trip count the analyser could not constrain — reached
//! `Inconclusive` only because the old line-oriented scanner treated *every*
//! `while` as unbounded, which is also why it put 60% of the CPython standard
//! library into that state. Its replacement correctly says nothing, so the
//! fixture now reports clean and the test that used it was asserting a
//! heuristic's bug rather than the criterion.
//!
//! Route 1 is a real and sufficient guard against the specific hole criterion
//! 3 names — an outcome with no code of its own falling through to `0` — so
//! `Outcome::Inconclusive` is genuinely exercised end to end. But it is a
//! parser limitation standing where an analysis limitation belongs, and the
//! suite should not pretend otherwise. **Missing coverage, booked rather than
//! papered over: a test that a successfully-analysed unit with an undischarged
//! assumption does not exit `0`. It belongs with the lane that lands bound
//! inference, and it is the test that would have caught the original design
//! defect.**

mod common;

use std::io;

use common::{
    CLEAN_PY, EXIT_CLEAN, FINDINGS_PY, PEP701_FSTRING_PY, PYPROJECT_MALFORMED, PYPROJECT_NO_LANDAV,
    Project, UNREADABLE_AS_PYTHON_PY,
};

/// The criterion, stated directly, over both classes of unreadable source.
///
/// **Deliberately `!= 0` rather than an exact code.** The implementation has
/// since chosen `1` (`Outcome::Inconclusive => ExitCode::Findings`, on the
/// argument that the tool completed and the result is a fact about the code).
/// That argument is sound, but it is the implementer's call to make and a
/// later `--fail-on-partial` default could revisit it. What is not revisable
/// is that it must not be `0`: whichever code is chosen, the hole stays shut.
/// Tighten this if the decision is ever frozen; do not loosen it.
#[test]
fn inconclusive_analysis_does_not_report_clean() -> io::Result<()> {
    let project = Project::new()?;

    for (name, body) in [
        ("python2_module.py", UNREADABLE_AS_PYTHON_PY),
        ("pep701_fstring.py", PEP701_FSTRING_PY),
    ] {
        let target = project.write(name, body)?;

        let run = project.check(&target, &[])?;

        run.assert_did_not_crash();
        assert_ne!(
            run.code,
            EXIT_CLEAN,
            "`{name}` was never read as Python, so no conclusion was reached \
             about it, and the tool reported it as clean. An outcome with no \
             code of its own falling through to 0 asserts a property nobody \
             proved.\n{}",
            run.describe()
        );
        run.assert_code_is_sanctioned();
    }
    Ok(())
}

/// An inconclusive result must be *attributed to the file it is about*.
///
/// Two earlier versions of this test passed for the wrong reason and both are
/// worth naming, because the same trap is easy to walk back into:
///
/// * checking only for the filename passed on the run summary, which names the
///   target whatever happened;
/// * checking the whole output for the word "inconclusive" passed on the
///   summary too, which ends `— 0 finding(s), 0 inconclusive` on *every* run,
///   including a clean one.
///
/// So the assertion is per line, over a **directory** target — which puts the
/// directory in the summary and leaves the filename to the diagnostic — and it
/// checks both directions: the unreadable file is named as inconclusive, and
/// its clean neighbour is not. An exit code with no attribution tells an
/// operator that something in the tree could not be read but not what, and the
/// blame is the whole recovery path (`CONTRIBUTING.md` rule 3).
#[test]
fn an_inconclusive_result_is_attributed_to_the_file_it_is_about() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/clean.py", CLEAN_PY)?;
    project.write("src/python2_module.py", UNREADABLE_AS_PYTHON_PY)?;
    let target = project.root().join("src");

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();

    let blamed = |name: &str| {
        run.output().lines().any(|line| {
            let line = line.to_lowercase();
            line.contains(name) && line.contains("inconclusive")
        })
    };

    assert!(
        blamed("python2_module.py"),
        "no line names python2_module.py as inconclusive, so the operator \
         knows only that something in the tree could not be read.\n{}",
        run.describe()
    );
    assert!(
        !blamed("clean.py"),
        "clean.py parsed and was analysed; blaming it for the neighbour's \
         inconclusive result sends the reader to the wrong file.\n{}",
        run.describe()
    );
    Ok(())
}

/// Aggregation, which is a property of the driver and not of the frontend: an
/// inconclusive unit must not be absorbed by its clean neighbours. A clean
/// file does not license a claim about the file next to it.
///
/// This one is unaffected by which route reached `Inconclusive` — it is about
/// what the run does with the outcome once it has it — so it is fully covered
/// today.
#[test]
fn an_inconclusive_file_does_not_make_a_directory_clean() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/clean.py", CLEAN_PY)?;
    project.write("src/python2_module.py", UNREADABLE_AS_PYTHON_PY)?;
    let target = project.root().join("src");

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "a directory containing one file nothing could be concluded about \
         reported clean; the inconclusive result was absorbed by its clean \
         neighbour.\n{}",
        run.describe()
    );
    Ok(())
}

/// The outcome space is closed: whatever happens, the process exits with one
/// of the three sanctioned codes. A fourth code — an `anyhow` bail escaping
/// `main` as `1`, a clap error as `2`, a panic as `101` — is a contract break
/// even when the number happens to look plausible.
#[test]
fn every_outcome_maps_into_the_frozen_code_set() -> io::Result<()> {
    let project = Project::new()?;
    let clean = project.write("clean.py", CLEAN_PY)?;
    let findings = project.write("quadratic.py", FINDINGS_PY)?;
    let inconclusive = project.write("python2_module.py", UNREADABLE_AS_PYTHON_PY)?;
    let empty_file = project.write("empty.py", "")?;
    let not_python = project.write("notes.txt", "this is not source code\n")?;
    let empty_dir = project.mkdir("nothing_here")?;
    let missing = project.root().join("absent.py");
    let bad_config = project.write("bad.toml", PYPROJECT_MALFORMED)?;
    let good_config = project.write("good.toml", PYPROJECT_NO_LANDAV)?;

    let targets = [
        clean,
        findings,
        inconclusive,
        empty_file,
        not_python,
        empty_dir,
        missing,
        project.root().to_path_buf(),
    ];

    let bad = bad_config.to_string_lossy().into_owned();
    let good = good_config.to_string_lossy().into_owned();
    let variants: [Vec<String>; 3] = [
        Vec::new(),
        vec!["--config".to_owned(), bad],
        vec!["--config".to_owned(), good],
    ];

    for target in &targets {
        for extra in &variants {
            let borrowed: Vec<&str> = extra.iter().map(String::as_str).collect();
            let run = project.check(target, &borrowed)?;
            run.assert_did_not_crash();
            run.assert_code_is_sanctioned();
        }
    }
    Ok(())
}
