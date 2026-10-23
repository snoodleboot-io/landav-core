//! `LAN-123`: **`landav bench` runs the declared corpora and keeps the
//! baselines by commit, so `diff` has something to diff.**
//!
//! Process-level, like every other subcommand: a corpora file in a temp
//! directory points at a temp directory of Python files, `run` writes a
//! baseline under a label, `diff` compares two labels, `list` shows them, and
//! what is missing is named.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use common::{EXIT_CLEAN, EXIT_TOOL_ERROR, Project};

/// A corpora file naming `demo` at `root/corpus`.
fn setup(project: &Project) -> io::Result<(String, String)> {
    let corpus = project.mkdir("corpus")?;
    let config = project.write(
        "corpora.toml",
        &format!("[corpus.demo]\npath = \"{}\"\n", corpus.display()),
    )?;
    let baselines = project.mkdir("baselines")?;
    Ok((
        config.to_str().unwrap().to_owned(),
        baselines.to_str().unwrap().to_owned(),
    ))
}

#[test]
fn run_writes_a_baseline_under_the_label_and_diff_reads_two_back() -> io::Result<()> {
    let project = Project::new()?;
    let (config, baselines) = setup(&project)?;
    project.write(
        "corpus/w.py",
        "def g(x, n: int) -> int:\n    for i in range(n):\n        f(x)\n    return 1\n",
    )?;
    let first = project.run(&[
        "bench",
        "--config",
        &config,
        "--baselines",
        &baselines,
        "run",
        "demo",
        "--label",
        "t1",
    ])?;
    assert_eq!(first.code, EXIT_CLEAN, "{}", first.describe());
    assert!(
        project.root().join("baselines/t1-demo.json").is_file(),
        "{}",
        first.describe()
    );
    assert!(
        first.stdout.contains("demo: 1 functions, 0 exact"),
        "run says what it wrote:\n{}",
        first.describe()
    );

    project.write(
        "corpus/w.py",
        "def g(x, n: int) -> int:\n    for i in range(n):\n        pass\n    return 1\n",
    )?;
    let second = project.run(&[
        "bench",
        "--config",
        &config,
        "--baselines",
        &baselines,
        "run",
        "--label",
        "t2",
    ])?;
    assert_eq!(
        second.code,
        EXIT_CLEAN,
        "all corpora when none named: {}",
        second.describe()
    );

    let diff = project.run(&[
        "bench",
        "--config",
        &config,
        "--baselines",
        &baselines,
        "diff",
        "t1",
        "t2",
    ])?;
    assert_eq!(diff.code, EXIT_CLEAN, "{}", diff.describe());
    assert!(
        diff.stdout.contains("===== demo: t1 -> t2 =====")
            && diff.stdout.contains("partial -> exact: 1"),
        "{}",
        diff.describe()
    );

    let list = project.run(&["bench", "--baselines", &baselines, "list"])?;
    assert_eq!(list.code, EXIT_CLEAN, "{}", list.describe());
    assert!(
        list.stdout.contains("t1") && list.stdout.contains("t2") && list.stdout.contains("demo"),
        "{}",
        list.describe()
    );
    Ok(())
}

#[test]
fn the_default_label_is_the_commit_the_binary_was_built_from() -> io::Result<()> {
    let project = Project::new()?;
    let (config, baselines) = setup(&project)?;
    project.write("corpus/w.py", "def g() -> int:\n    return 1\n")?;
    let run = project.run(&[
        "bench",
        "--config",
        &config,
        "--baselines",
        &baselines,
        "run",
    ])?;
    assert_eq!(run.code, EXIT_CLEAN, "{}", run.describe());
    let written: Vec<String> = std::fs::read_dir(project.root().join("baselines"))?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(written.len(), 1, "{written:?}");
    let name = &written[0];
    assert!(
        name.ends_with("-demo.json") && name.len() > "-demo.json".len(),
        "{name}"
    );
    assert!(
        !name.starts_with("-") && !name.contains('/'),
        "a label is a plain word: {name}"
    );
    Ok(())
}

#[test]
fn what_is_missing_is_named() -> io::Result<()> {
    let project = Project::new()?;
    let (config, baselines) = setup(&project)?;

    let unknown = project.run(&[
        "bench",
        "--config",
        &config,
        "--baselines",
        &baselines,
        "run",
        "nope",
    ])?;
    assert_eq!(unknown.code, EXIT_TOOL_ERROR, "{}", unknown.describe());
    assert!(
        unknown.stderr.contains("corpora.toml")
            && unknown.stderr.contains("`nope`")
            && unknown.stderr.contains("`demo`"),
        "the file, the missing name and the names it has:\n{}",
        unknown.describe()
    );

    let no_config = project.run(&[
        "bench",
        "--config",
        "nowhere/corpora.toml",
        "--baselines",
        &baselines,
        "run",
    ])?;
    assert_eq!(no_config.code, EXIT_TOOL_ERROR, "{}", no_config.describe());
    assert!(
        no_config.stderr.contains("nowhere/corpora.toml")
            && no_config.stderr.contains("[corpus.NAME]"),
        "{}",
        no_config.describe()
    );

    let no_baseline = project.run(&[
        "bench",
        "--config",
        &config,
        "--baselines",
        &baselines,
        "diff",
        "t1",
        "t2",
    ])?;
    assert_eq!(
        no_baseline.code,
        EXIT_TOOL_ERROR,
        "{}",
        no_baseline.describe()
    );
    assert!(
        no_baseline.stderr.contains("t1-demo.json"),
        "{}",
        no_baseline.describe()
    );
    Ok(())
}
