//! LAN-61 criterion 2: the exit code contract.
//!
//! `0` clean, `1` findings, `2` tool error. These three integers are a public
//! interface — CI integrations and the EE platform both branch on them, so
//! changing one is a breaking change and the numbers are asserted exactly
//! rather than through a helper that could be redefined.
//!
//! The `1` versus `2` distinction is the load-bearing one. A tool error that
//! reports as a finding gets the CI gate switched off by the first team that
//! hits it; a finding that reports as a tool error gets the finding ignored.
//! Both directions are asserted separately from the positive cases.

mod common;

use std::io;

use common::{
    CLEAN_PY, EXIT_CLEAN, EXIT_FINDINGS, EXIT_TOOL_ERROR, FINDINGS_PY, PYPROJECT_MALFORMED, Project,
};

#[test]
fn clean_file_exits_zero() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a file with nothing to report must exit 0.\n{}",
        run.describe()
    );
    Ok(())
}

#[test]
fn file_with_findings_exits_one() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("quadratic.py", FINDINGS_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_FINDINGS,
        "a file the analyser has something to report about must exit 1.\n{}",
        run.describe()
    );
    Ok(())
}

#[test]
fn nonexistent_path_exits_two() -> io::Result<()> {
    let project = Project::new()?;
    let missing = project.root().join("no_such_file.py");

    let run = project.check(&missing, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "a path that does not exist is a tool error, not a clean result.\n{}",
        run.describe()
    );
    run.assert_explains("no_such_file.py");
    Ok(())
}

#[test]
fn malformed_pyproject_exits_two() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    project.write("pyproject.toml", PYPROJECT_MALFORMED)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "configuration that cannot be parsed means the tool could not run \
         as asked; falling back to defaults and exiting 0 would report clean \
         under a configuration nobody chose.\n{}",
        run.describe()
    );
    run.assert_explains("pyproject.toml");
    Ok(())
}

/// The direction that matters most: "we could not look" must never be
/// indistinguishable from "we looked and found something". If it is, the first
/// team whose build breaks on a tool error turns the gate off.
#[test]
fn tool_errors_never_report_as_findings() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;

    // Each of these is a reason the tool could not complete, not a property of
    // the analysed code.
    let broken_config = project.write("broken.toml", PYPROJECT_MALFORMED)?;
    let missing = project.root().join("absent.py");
    let cases: Vec<(&str, common::Run)> = vec![
        ("nonexistent target", project.check(&missing, &[])?),
        (
            "unparseable explicit config",
            project.check(&target, &["--config", &broken_config.to_string_lossy()])?,
        ),
        (
            "explicit config that does not exist",
            project.check(&target, &["--config", "does_not_exist.toml"])?,
        ),
    ];

    for (label, run) in cases {
        run.assert_did_not_crash();
        assert_ne!(
            run.code,
            EXIT_FINDINGS,
            "{label}: a tool error reported as a finding.\n{}",
            run.describe()
        );
        assert_eq!(
            run.code,
            EXIT_TOOL_ERROR,
            "{label}: expected the tool-error code.\n{}",
            run.describe()
        );
    }
    Ok(())
}

/// The other direction: a real finding must not be dressed up as a tool
/// failure, or the finding gets triaged as flakiness and ignored.
#[test]
fn findings_never_report_as_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("quadratic.py", FINDINGS_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_TOOL_ERROR,
        "the analyser ran to completion, so its result is a finding, not a \
         tool error.\n{}",
        run.describe()
    );
    Ok(())
}

/// A clean result must not be reachable by accident from a failure path.
#[test]
fn tool_errors_never_report_clean() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    project.write("pyproject.toml", PYPROJECT_MALFORMED)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "a run that could not be configured reported clean.\n{}",
        run.describe()
    );
    Ok(())
}

/// The same input must produce the same exit code. A gate that flips between
/// 0 and 1 on identical input is worse than no gate.
#[test]
fn exit_code_is_deterministic_for_the_same_input() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;

    let first = project.check(&target, &[])?;
    let second = project.check(&target, &[])?;

    first.assert_did_not_crash();
    second.assert_did_not_crash();
    assert_eq!(
        first.code,
        second.code,
        "two identical invocations disagreed.\nfirst: {}\nsecond: {}",
        first.describe(),
        second.describe()
    );
    Ok(())
}
