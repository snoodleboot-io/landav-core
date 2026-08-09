//! LAN-61 criteria 1, 4 and 5: zero config, `--config`, and `[tool.landav]`.
//!
//! Zero config is an adoption requirement: `landav check PATH` in a checkout
//! that has never heard of landav must produce a verdict, not a lecture about
//! configuration. The two escape hatches on top of that are an explicit file
//! (`--config`) and a `[tool.landav]` section in `pyproject.toml`, and the
//! explicit file wins.
//!
//! These tests are written to be **schema-free**. The set of keys under
//! `[tool.landav]` has not been decided, so asserting on key names here would
//! freeze a design the implementer is entitled to make. What is asserted
//! instead is observable regardless of schema: whether a configuration source
//! is *consulted at all*, which is the part the criteria actually name.

mod common;

use std::io;

use common::{
    CLEAN_PY, EXIT_CLEAN, EXIT_TOOL_ERROR, PYPROJECT_EMPTY_LANDAV, PYPROJECT_LANDAV_NOT_A_TABLE,
    PYPROJECT_MALFORMED, PYPROJECT_NO_LANDAV, Project,
};

/// Criterion 1. No `pyproject.toml`, no `landav.toml`, no flags.
#[test]
fn check_works_with_no_configuration_at_all() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_TOOL_ERROR,
        "the absence of configuration is not a tool error; zero config is the \
         default path, not a degraded one.\n{}",
        run.describe()
    );
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a clean file with no configuration must exit 0.\n{}",
        run.describe()
    );
    Ok(())
}

/// Criterion 1, directory form. Whole-tree analysis must work unconfigured
/// too, because that is what a CI job actually runs.
#[test]
fn check_works_on_a_directory_with_no_configuration() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/clean.py", CLEAN_PY)?;
    project.write("src/pkg/also_clean.py", CLEAN_PY)?;
    let target = project.root().join("src");

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a tree of clean files with no configuration must exit 0.\n{}",
        run.describe()
    );
    Ok(())
}

/// Criterion 5, negative half. A `pyproject.toml` belonging entirely to other
/// tools is the overwhelmingly common case; refusing to run on it, or warning
/// about it, would make zero config a fiction.
#[test]
fn pyproject_without_a_tool_landav_section_is_not_an_error() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    project.write("pyproject.toml", PYPROJECT_NO_LANDAV)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a pyproject.toml that says nothing about landav must behave exactly \
         like no pyproject.toml at all.\n{}",
        run.describe()
    );
    Ok(())
}

/// Criterion 5, identity half. An empty `[tool.landav]` section declares
/// nothing, so it must be indistinguishable from declaring nothing.
#[test]
fn an_empty_tool_landav_section_behaves_like_no_section() -> io::Result<()> {
    let with_section = Project::new()?;
    let a = with_section.write("clean.py", CLEAN_PY)?;
    with_section.write("pyproject.toml", PYPROJECT_EMPTY_LANDAV)?;

    let without_section = Project::new()?;
    let b = without_section.write("clean.py", CLEAN_PY)?;
    without_section.write("pyproject.toml", PYPROJECT_NO_LANDAV)?;

    let sectioned = with_section.check(&a, &[])?;
    let plain = without_section.check(&b, &[])?;

    sectioned.assert_did_not_crash();
    plain.assert_did_not_crash();
    assert_eq!(
        sectioned.code,
        plain.code,
        "an empty [tool.landav] section changed the verdict.\nwith: {}\n\
         without: {}",
        sectioned.describe(),
        plain.describe()
    );
    Ok(())
}

/// Criterion 5, positive half — asserted without naming a single key.
///
/// `tool.landav` is valid TOML here but is a scalar where the configuration
/// table belongs. An implementation that genuinely *reads* the section cannot
/// make sense of it and must say so. An implementation that looks for a table,
/// finds something else and shrugs will exit 0, having silently ignored
/// configuration the user wrote. That silence is the bug this pins: config
/// that is quietly discarded is worse than config that is rejected, because
/// the run still reports a verdict under settings nobody chose.
#[test]
fn a_tool_landav_section_is_actually_read() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    project.write("pyproject.toml", PYPROJECT_LANDAV_NOT_A_TABLE)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "the [tool.landav] section was unusable and the run continued \
         anyway, which means the section is not being consulted.\n{}",
        run.describe()
    );
    run.assert_explains("landav");
    Ok(())
}

/// Criterion 4. The explicit file must be loaded, not merely accepted and
/// discarded — same argument as above, applied to the flag.
#[test]
fn an_explicit_config_file_is_actually_read() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    let explicit = project.write("landav.toml", PYPROJECT_MALFORMED)?;

    let run = project.check(&target, &["--config", &explicit.to_string_lossy()])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "--config pointed at a file that is not valid TOML and the run \
         succeeded, so the file was never read.\n{}",
        run.describe()
    );
    run.assert_explains("landav.toml");
    Ok(())
}

/// Criterion 4. A `--config` path that does not exist is the user asking for
/// something the tool cannot provide. Falling back to discovery would run
/// under a configuration the user did not ask for and report a verdict for it.
#[test]
fn a_missing_explicit_config_file_is_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    project.write("pyproject.toml", PYPROJECT_EMPTY_LANDAV)?;

    let run = project.check(&target, &["--config", "nowhere/landav.toml"])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "a --config path that does not exist must fail rather than silently \
         fall back to discovery.\n{}",
        run.describe()
    );
    run.assert_explains("nowhere/landav.toml");
    Ok(())
}

/// Criterion 4 versus criterion 5: precedence, in the direction that proves
/// discovery is *replaced* rather than merged.
///
/// The `pyproject.toml` here carries a `[tool.landav]` the tool cannot use.
/// If discovery still ran, the invocation would fail. It must not: the
/// explicit file is the whole configuration.
#[test]
fn explicit_config_replaces_pyproject_discovery() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    project.write("pyproject.toml", PYPROJECT_LANDAV_NOT_A_TABLE)?;
    let explicit = project.write("landav.toml", "")?;

    let run = project.check(&target, &["--config", &explicit.to_string_lossy()])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "--config was given, so pyproject.toml is not the configuration \
         source and its [tool.landav] section must not affect the run.\n{}",
        run.describe()
    );
    Ok(())
}

/// The same precedence, from the other side: a usable `pyproject.toml` must
/// not rescue an unusable `--config`. If it does, precedence is really
/// "whichever parses", which is not a precedence rule anyone can predict.
#[test]
fn pyproject_does_not_rescue_a_broken_explicit_config() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    project.write("pyproject.toml", PYPROJECT_EMPTY_LANDAV)?;
    let explicit = project.write("landav.toml", PYPROJECT_MALFORMED)?;

    let run = project.check(&target, &["--config", &explicit.to_string_lossy()])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "a valid pyproject.toml masked a broken --config file.\n{}",
        run.describe()
    );
    Ok(())
}

/// The section ships as `[tool.landav]`. The delivery workbook says `pycost`,
/// which was the pre-rename working title; honouring it would ship the old
/// name as a supported interface by accident.
#[test]
fn the_pre_rename_pycost_section_is_not_honoured() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    project.write(
        "pyproject.toml",
        "[project]\nname = \"fixture\"\n\n[tool]\npycost = 42\n",
    )?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "[tool.pycost] is not landav configuration and must be ignored the \
         way any other tool's section is.\n{}",
        run.describe()
    );
    Ok(())
}

/// The binary is `landav`, and the subcommand is `check`. Named explicitly so
/// a rename shows up here rather than in a user's CI.
#[test]
fn the_binary_is_named_landav() {
    let path = std::path::Path::new(common::BIN);
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    assert_eq!(
        name.as_deref(),
        Some("landav"),
        "the binary ships as `landav`, not the `pycost` working title"
    );
}
