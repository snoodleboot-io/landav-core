//! `LAN-111`: **the walk does not enter a virtualenv, and says so.**
//!
//! # What is being defended
//!
//! On the one application tree landav was measured against, 42,831 of the
//! 43,203 functions in its coverage denominator were under `.venv/`. The
//! project's own 372 were 0.9% of every number on the summary line. A user
//! asking `landav check .` about their project was being answered about
//! `site-packages`.
//!
//! Two properties, and both matter. The default must skip what every other
//! Python tool skips. And the skip must be **visible**: a directory left out of
//! the denominator without a word is the same defect as one wrongly included,
//! seen from the other side. So every test here that asserts a skip also
//! asserts the report says so.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use common::{EXIT_CLEAN, EXIT_TOOL_ERROR, Project};
use serde_json::Value;

const APP: &str = "def app(n: int) -> int:\n    return n\n";
const DEP: &str = "def dep(n: int) -> int:\n    return n\n";

/// A project with its own source, a venv, a `__pycache__`, and a `build/`.
fn tree(project: &Project) -> io::Result<()> {
    project.write("src/app.py", APP)?;
    project.write(".venv/pyvenv.cfg", "home = /usr/bin\n")?;
    project.write(".venv/lib/python3.12/site-packages/dep.py", DEP)?;
    project.write("src/__pycache__/app.cpython-312.py", DEP)?;
    project.write("build/lib/app.py", DEP)?;
    Ok(())
}

fn summary(run: &common::Run) -> Value {
    let parsed: Value = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|error| panic!("not JSON: {error}\n{}", run.describe()));
    parsed["summary"].clone()
}

/// **The default: the project's own file, and nothing under the venv.**
#[test]
fn a_virtualenv_is_not_entered_by_default_and_the_summary_says_so() -> io::Result<()> {
    let project = Project::new()?;
    tree(&project)?;

    let run = project.check(project.root(), &["--json"])?;
    let summary = summary(&run);
    assert_eq!(
        summary["files_analysed"], 1,
        "the venv, the cache or the build directory leaked into the \
         denominator: {summary}"
    );
    let skipped = summary["skipped_directories"]
        .as_array()
        .expect("skipped_directories is listed when anything was skipped");
    let reasons: Vec<&str> = skipped
        .iter()
        .map(|entry| entry["reason"].as_str().unwrap())
        .collect();
    assert_eq!(skipped.len(), 3, "{summary}");
    assert!(
        reasons.iter().any(|r| r.contains("virtual environment"))
            && reasons.iter().any(|r| r.contains("cache"))
            && reasons.iter().any(|r| r.contains("dependency or build")),
        "each skip must carry the rule that matched: {reasons:?}"
    );

    let text = project.check(project.root(), &[])?;
    assert!(
        text.mentions("skipped:") && text.mentions(".venv") && text.mentions("--include-vendored"),
        "a skip nobody can see is a skip nobody can question:\n{}",
        text.describe()
    );
    assert!(
        text.mentions("1 virtual environment"),
        "the summary line must carry the count:\n{}",
        text.describe()
    );
    Ok(())
}

/// **`--include-vendored` enters the venv and the build directory.**
///
/// The caches stay out: nothing in a `__pycache__` was written by anyone.
#[test]
fn include_vendored_enters_the_venv_but_never_the_caches() -> io::Result<()> {
    let project = Project::new()?;
    tree(&project)?;

    let run = project.check(project.root(), &["--json", "--include-vendored"])?;
    let summary = summary(&run);
    assert_eq!(summary["files_analysed"], 3, "{summary}");
    let skipped = summary["skipped_directories"]
        .as_array()
        .expect("the cache is still skipped");
    assert_eq!(skipped.len(), 1, "{summary}");
    assert!(
        skipped[0]["path"]
            .as_str()
            .unwrap()
            .ends_with("__pycache__")
    );
    Ok(())
}

/// **A venv is a venv whatever it is called.**
///
/// `pyvenv.cfg` is what CPython writes at the root of every virtual
/// environment. A name list catches `.venv`; only the file catches `tooling`.
#[test]
fn a_directory_holding_pyvenv_cfg_is_skipped_under_any_name() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/app.py", APP)?;
    project.write("tooling/pyvenv.cfg", "home = /usr/bin\n")?;
    project.write("tooling/lib/dep.py", DEP)?;

    let run = project.check(project.root(), &["--json"])?;
    let summary = summary(&run);
    assert_eq!(summary["files_analysed"], 1, "{summary}");
    assert!(
        summary["skipped_directories"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("virtual environment"),
        "{summary}"
    );
    Ok(())
}

/// **Naming a directory is asking for it.**
///
/// The rules apply beneath the target, never to it. `landav check .venv` is a
/// request to analyse the venv, and the tool honours it without a flag.
#[test]
fn the_target_itself_is_never_skipped() -> io::Result<()> {
    let project = Project::new()?;
    tree(&project)?;

    let run = project.check(&project.root().join(".venv"), &["--json"])?;
    let summary = summary(&run);
    assert_eq!(
        summary["files_analysed"], 1,
        "the venv was named as the target and still not analysed: {summary}"
    );
    assert!(summary.get("skipped_directories").is_none(), "{summary}");
    Ok(())
}

/// **`exclude` in the configuration, in the waiver glob dialect.**
///
/// Applied whatever `--include-vendored` says: an exclusion the user wrote is
/// not vendoring, and the flag must not un-exclude it.
#[test]
fn a_configured_exclude_is_honoured_and_named_by_its_pattern() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/app.py", APP)?;
    project.write("legacy/old.py", DEP)?;
    project.write("src/generated/schema.py", DEP)?;
    project.write(
        "pyproject.toml",
        "[tool.landav]\nexclude = [\"legacy\", \"src/generated\"]\n",
    )?;

    for extra in [&["--json"][..], &["--json", "--include-vendored"][..]] {
        let run = project.check(project.root(), extra)?;
        let summary = summary(&run);
        assert_eq!(summary["files_analysed"], 1, "{extra:?}: {summary}");
        let reasons: Vec<String> = summary["skipped_directories"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["reason"].as_str().unwrap().to_owned())
            .collect();
        assert!(
            reasons.iter().any(|r| r.contains("`legacy`"))
                && reasons.iter().any(|r| r.contains("`src/generated`")),
            "each skip must name the pattern that caused it: {reasons:?}"
        );
    }

    let text = project.check(project.root(), &[])?;
    assert!(
        text.mentions("excluded by `legacy`") && text.mentions("2 directories by `exclude`"),
        "{}",
        text.describe()
    );
    Ok(())
}

/// **A malformed `exclude` is refused, not read as something else.**
#[test]
fn an_exclude_that_is_not_a_list_of_strings_is_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/app.py", APP)?;
    project.write("pyproject.toml", "[tool.landav]\nexclude = \".venv\"\n")?;

    let run = project.check(project.root(), &[])?;
    run.assert_did_not_crash();
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    assert!(
        run.mentions("`exclude` must be a list"),
        "{}",
        run.describe()
    );
    Ok(())
}

/// A run over a clean tree says nothing about skipping.
#[test]
fn a_tree_with_nothing_to_skip_reads_as_it_always_did() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/app.py", APP)?;

    let run = project.check(project.root(), &[])?;
    assert_eq!(run.code, EXIT_CLEAN, "{}", run.describe());
    assert!(!run.mentions("skipped"), "{}", run.describe());
    let json = project.check(project.root(), &["--json"])?;
    assert!(summary(&json).get("skipped_directories").is_none());
    Ok(())
}
