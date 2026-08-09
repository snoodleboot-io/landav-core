//! LAN-61: inputs chosen to break the tool rather than to be analysed.
//!
//! Every case here must exit `2` with a diagnostic naming what went wrong.
//! Three outcomes are forbidden for all of them:
//!
//! * a panic — `CONTRIBUTING.md` rule 2; a panic carries no blame, so there is
//!   nothing for the operator to act on;
//! * a silent `0` — the tool could not look, and saying "clean" is a claim it
//!   has no basis for;
//! * a `1` — that would put "we could not read the file" in the same bucket as
//!   "your code has a problem", and the first team to hit it disables the gate.

mod common;

use std::io;

use common::{CLEAN_PY, EXIT_CLEAN, EXIT_FINDINGS, EXIT_TOOL_ERROR, PYPROJECT_MALFORMED, Project};

/// A target with nothing analysable in it.
///
/// This is the misconfigured-path case: a CI job whose glob stopped matching
/// after a directory move analyses nothing and, if that reports `0`, goes
/// green forever while checking no code at all. "We analysed nothing" is not
/// "we found nothing".
#[test]
fn an_empty_directory_is_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.mkdir("src")?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "analysing nothing reported clean; a CI path that stopped matching \
         now passes forever.\n{}",
        run.describe()
    );
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "an empty target is a tool error: there was nothing to look at.\n{}",
        run.describe()
    );
    run.assert_explains("src");
    Ok(())
}

/// A directory that exists but contains no source the frontend recognises.
/// Same argument as the empty directory, one step less obvious.
#[test]
fn a_directory_with_no_analysable_source_is_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/README.md", "# nothing to see\n")?;
    project.write("src/data.csv", "a,b\n1,2\n")?;
    let target = project.root().join("src");

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "a directory with no analysable source reported clean.\n{}",
        run.describe()
    );
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_symlink_loop_is_a_tool_error_and_terminates() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let project = Project::new()?;
    let a = project.root().join("a.py");
    let b = project.root().join("b.py");
    symlink(&b, &a)?;
    symlink(&a, &b)?;

    let run = project.check(&a, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_FINDINGS,
        "a path that cannot be resolved is not a finding about the code.\n{}",
        run.describe()
    );
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "a symlink loop must be reported as a tool error.\n{}",
        run.describe()
    );
    run.assert_explains("a.py");
    Ok(())
}

/// A directory that contains a symlink back to itself. A naive recursive walk
/// never returns; the assertion is that the process terminates at all, with a
/// code, and does not claim the tree is clean.
#[cfg(unix)]
#[test]
fn a_directory_symlink_cycle_does_not_hang_or_report_clean() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let project = Project::new()?;
    let src = project.mkdir("src")?;
    project.write("src/clean.py", CLEAN_PY)?;
    symlink(&src, src.join("loop"))?;

    let run = project.check(&src, &[])?;

    run.assert_did_not_crash();
    run.assert_code_is_sanctioned();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "the walk met a cycle it could not fully traverse and still reported \
         the tree clean.\n{}",
        run.describe()
    );
    Ok(())
}

/// A source file the process cannot read. The bytes were never seen, so no
/// statement about the file is available — including "clean".
#[cfg(unix)]
#[test]
fn an_unreadable_file_is_a_tool_error() -> io::Result<()> {
    use std::fs::{Permissions, set_permissions};
    use std::os::unix::fs::PermissionsExt;

    let project = Project::new()?;
    let target = project.write("secret.py", CLEAN_PY)?;
    set_permissions(&target, Permissions::from_mode(0o000))?;

    // Root ignores the mode bits, which would make this test assert nothing.
    // Fail loudly rather than quietly weaken.
    assert!(
        std::fs::read(&target).is_err(),
        "precondition not met: the file is still readable. This test cannot \
         run as root."
    );

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "a file whose bytes were never read was reported clean.\n{}",
        run.describe()
    );
    assert_ne!(
        run.code,
        EXIT_FINDINGS,
        "an unreadable file is not a finding about its contents.\n{}",
        run.describe()
    );
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    run.assert_explains("secret.py");
    Ok(())
}

/// One unreadable file inside an otherwise clean tree must not be absorbed by
/// its neighbours. Partial coverage reported as full coverage is the same
/// failure as criterion 3, arriving through the filesystem instead of through
/// the solver.
#[cfg(unix)]
#[test]
fn an_unreadable_file_does_not_make_its_directory_clean() -> io::Result<()> {
    use std::fs::{Permissions, set_permissions};
    use std::os::unix::fs::PermissionsExt;

    let project = Project::new()?;
    project.write("src/clean.py", CLEAN_PY)?;
    let secret = project.write("src/secret.py", CLEAN_PY)?;
    set_permissions(&secret, Permissions::from_mode(0o000))?;
    assert!(
        std::fs::read(&secret).is_err(),
        "precondition not met: this test cannot run as root."
    );
    let target = project.root().join("src");

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "a tree containing a file that could not be read reported clean.\n{}",
        run.describe()
    );
    Ok(())
}

#[test]
fn a_pyproject_that_is_not_valid_toml_is_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    project.write("pyproject.toml", PYPROJECT_MALFORMED)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_FINDINGS,
        "broken configuration is not a finding about the analysed code.\n{}",
        run.describe()
    );
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "an unparseable pyproject.toml must stop the run.\n{}",
        run.describe()
    );
    run.assert_explains("pyproject.toml");
    Ok(())
}

/// A source file that is not valid UTF-8. The parser cannot see the text, so
/// there is nothing to conclude about it.
#[test]
fn a_non_utf8_source_file_is_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.root().join("binary.py");
    std::fs::write(&target, [0xF0, 0x28, 0x8C, 0x28, 0xFF, 0xFE, 0x00])?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "a file that could not be decoded was reported clean.\n{}",
        run.describe()
    );
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    run.assert_explains("binary.py");
    Ok(())
}

/// A UTF-8 byte-order mark at the start of a clean file.
///
/// Python's own tokeniser accepts a leading BOM, and every source written by a
/// Windows editor that defaults to "UTF-8 with signature" carries one. The
/// M0 line scanner did not, so the first statement of such a file was reported
/// as unrecognised and the file exited `1`: a false positive on correct input,
/// affecting a whole platform's worth of it. That is the failure mode a gate
/// does not survive, so it is pinned at the process boundary rather than left
/// to the frontend's unit tests.
#[test]
fn a_utf8_bom_does_not_make_a_clean_file_inconclusive() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("bom.py", &format!("\u{feff}{CLEAN_PY}"))?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a byte-order mark is legal Python and must not change the verdict \
         about the code after it.\n{}",
        run.describe()
    );
    Ok(())
}

/// `--config` pointing at a directory rather than a file.
#[test]
fn a_config_flag_pointing_at_a_directory_is_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    let dir = project.mkdir("conf")?;

    let run = project.check(&target, &["--config", &dir.to_string_lossy()])?;

    run.assert_did_not_crash();
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    run.assert_explains("conf");
    Ok(())
}

/// Usage errors are tool errors. `check` with no target cannot be a verdict
/// about anything, and clap's own convention for a usage error is `2`, which
/// is the same code by the same reasoning.
#[test]
fn check_with_no_target_is_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;

    let run = project.run(&["check"])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "a usage error reported clean.\n{}",
        run.describe()
    );
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    Ok(())
}

/// An unrecognised subcommand is a usage error, not a verdict.
#[test]
fn an_unknown_subcommand_is_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;

    let run = project.run(&["chekc", "."])?;

    run.assert_did_not_crash();
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    Ok(())
}

/// An unrecognised flag must not be ignored. Silently dropping `--fail-on-x`
/// because it was misspelled runs the analysis under settings the caller did
/// not ask for and reports a verdict for them.
#[test]
fn an_unknown_flag_is_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;

    let run = project.check(&target, &["--not-a-real-flag"])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "an unrecognised flag was ignored and the run reported clean.\n{}",
        run.describe()
    );
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    Ok(())
}
