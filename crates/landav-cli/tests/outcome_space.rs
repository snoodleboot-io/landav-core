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

mod common;

use std::io;

use common::{
    CLEAN_PY, EXIT_CLEAN, FINDINGS_PY, INCONCLUSIVE_PY, PYPROJECT_MALFORMED, PYPROJECT_NO_LANDAV,
    Project,
};

/// The criterion, stated directly.
///
/// **Open decision, deliberately not pre-empted here.** Whether an
/// inconclusive result is a finding (`1`) or a tool error (`2`) is a product
/// decision that has not been made: `Verdict::exit_code` takes a
/// `fail_on_partial` flag, and the default it should carry is exactly what is
/// undecided. This test therefore pins only the part that is settled — it must
/// not be `0` — so that whichever code is chosen, the "silent clean" hole
/// stays closed. Tighten this to an exact code once the decision lands; do not
/// loosen it.
#[test]
fn inconclusive_analysis_does_not_report_clean() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("inconclusive.py", INCONCLUSIVE_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "analysis reached no conclusion about this file, and the tool \
         reported it as clean. A partial result must have a code of its own; \
         falling through to 0 asserts a property that was never proven.\n{}",
        run.describe()
    );
    run.assert_code_is_sanctioned();
    Ok(())
}

/// An inconclusive result must also say so. Whichever code it gets, an
/// operator has to be able to tell it apart from a proven-clean run, and the
/// blame for what could not be discharged is the whole recovery path
/// (`CONTRIBUTING.md` rule 3: failure must carry blame).
#[test]
fn inconclusive_analysis_is_reported_in_the_output() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("inconclusive.py", INCONCLUSIVE_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert!(
        !run.output().trim().is_empty(),
        "an inconclusive run produced no output at all, so the exit code is \
         the only signal and the unaccounted term is unrecoverable.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("inconclusive.py"),
        "the output does not name the file nothing could be concluded \
         about.\n{}",
        run.describe()
    );
    Ok(())
}

/// A file the analyser cannot conclude anything about must not be silently
/// dropped from the run either. Skipping it and exiting 0 is the same bug
/// wearing a different hat.
#[test]
fn an_inconclusive_file_does_not_make_a_directory_clean() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/clean.py", CLEAN_PY)?;
    project.write("src/inconclusive.py", INCONCLUSIVE_PY)?;
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
    let inconclusive = project.write("inconclusive.py", INCONCLUSIVE_PY)?;
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
