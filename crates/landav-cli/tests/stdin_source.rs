//! `LAN-85` at the process boundary: **a code block must be analysable without
//! becoming a file first.**
//!
//! # What is being defended
//!
//! `landav check` took a file or a directory, and both worked. A caller
//! holding a *block* — an agent, an editor, a hook working from a buffer — had
//! to write a temporary file and invent a name for it, when the capability
//! existed one layer down: `analyze_source` has always taken source text.
//!
//! # The property that matters most
//!
//! A snippet must take **the same path** as a file, not a parallel one. A
//! second code path for the same question is a second thing to keep correct,
//! and it would eventually answer differently. So the central test here is not
//! that stdin works — it is that stdin and a file agree.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use common::{EXIT_CLEAN, EXIT_TOOL_ERROR, Project};

const SNIPPET: &str = r"
def counted(n: int) -> int:
    x = 0
    for i in range(n):
        x = i
    return x
";

/// The property the whole feature rests on: same source, same conclusions,
/// whichever way it arrived.
#[test]
fn a_snippet_and_a_file_agree() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("same.py", SNIPPET)?;

    let from_file = project.check(&target, &["--bounds"])?;
    let from_stdin = project.run_with_stdin(&["check", "--stdin", "--bounds"], SNIPPET)?;

    assert_eq!(
        from_file.code, from_stdin.code,
        "the verdict must not depend on how the source arrived"
    );

    // Compared on the part that is genuinely the same. The position prefix
    // differs by design — one is a real path and the other is `<stdin>` — and
    // asserting on it would be asserting that they are *not* different, which
    // is the opposite of the point.
    let bound_of = |run: &common::Run| {
        run.output()
            .lines()
            .find(|line| line.contains("counted:"))
            .and_then(|line| line.split_once("counted:"))
            .map(|(_, rest)| rest.trim().to_owned())
            .unwrap_or_default()
    };
    assert_eq!(
        bound_of(&from_file),
        bound_of(&from_stdin),
        "the same source gave different bounds through different doors"
    );
    Ok(())
}

/// A snippet is bounded, not merely accepted.
#[test]
fn a_snippet_gets_a_bound() -> io::Result<()> {
    let project = Project::new()?;
    let run = project.run_with_stdin(&["check", "--stdin", "--bounds"], SNIPPET)?;

    assert_eq!(run.code, EXIT_CLEAN, "{}", run.describe());
    assert!(
        run.mentions("Theta("),
        "a counted loop from stdin must still be derived exactly: {}",
        run.describe()
    );
    Ok(())
}

/// The default name is honest about not being a file. A name that looks like a
/// path invites a reader to go and open it.
#[test]
fn the_default_name_is_not_a_path() -> io::Result<()> {
    let project = Project::new()?;
    let run = project.run_with_stdin(&["check", "--stdin", "--bounds"], SNIPPET)?;

    assert!(
        run.mentions("<stdin>"),
        "positions must be attributed to something: {}",
        run.describe()
    );
    Ok(())
}

/// A caller that knows what it sent can say so, and diagnostics follow.
#[test]
fn a_caller_can_name_its_snippet() -> io::Result<()> {
    let project = Project::new()?;
    let run = project.run_with_stdin(
        &["check", "--stdin", "--stdin-name", "buffer.py", "--bounds"],
        SNIPPET,
    )?;

    assert!(
        run.mentions("buffer.py:"),
        "the supplied name must be used: {}",
        run.describe()
    );
    assert!(
        !run.mentions("<stdin>"),
        "the default must not leak through once a name was given: {}",
        run.describe()
    );
    Ok(())
}

/// Positions start at line 1 of what was sent. A caller passing an excerpt
/// knows its own offset; landav inventing one it was not told about would be
/// worse than reporting honestly against the snippet.
#[test]
fn positions_are_relative_to_what_was_sent() -> io::Result<()> {
    let project = Project::new()?;
    let run = project.run_with_stdin(
        &["check", "--stdin", "--bounds"],
        "def first(n: int) -> int:\n    return n\n",
    )?;
    assert!(
        run.mentions("<stdin>:1:"),
        "the first line of the snippet is line 1: {}",
        run.describe()
    );
    Ok(())
}

/// The silence problem, in its sharpest form. A broken pipeline sends nothing;
/// a run that reported clean would be green for having analysed no code, and
/// there is not even a path to go and inspect.
#[test]
fn empty_input_is_a_named_failure_not_a_clean_run() -> io::Result<()> {
    let project = Project::new()?;
    let run = project.run_with_stdin(&["check", "--stdin"], "")?;

    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "empty input must not report clean: {}",
        run.describe()
    );
    assert!(
        run.mentions("empty"),
        "the failure must say what was wrong: {}",
        run.describe()
    );
    Ok(())
}

/// Whitespace parses as a valid module with no statements, so it would
/// otherwise be indistinguishable from a successful analysis of nothing.
#[test]
fn whitespace_only_input_counts_as_empty() -> io::Result<()> {
    let project = Project::new()?;
    let run = project.run_with_stdin(&["check", "--stdin"], "   \n\n\t\n")?;
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    Ok(())
}

/// Exactly one of PATH and `--stdin`. Neither leaves nothing to look at; both
/// would force the tool to silently prefer one, and a caller that supplied
/// both has a bug it should be told about.
#[test]
fn a_path_and_stdin_together_are_refused() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("same.py", SNIPPET)?;
    let run = project.run_with_stdin(&["check", &target.to_string_lossy(), "--stdin"], SNIPPET)?;
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "supplying both must be a usage error: {}",
        run.describe()
    );
    Ok(())
}

#[test]
fn neither_a_path_nor_stdin_is_refused() -> io::Result<()> {
    let project = Project::new()?;
    let run = project.run(&["check"])?;
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "a run with nothing to look at is a usage error: {}",
        run.describe()
    );
    Ok(())
}

/// Structured output works from a snippet too, which is the combination an
/// agent actually uses: hand over a block, get machine-readable conclusions.
#[test]
fn a_snippet_can_be_reported_as_json() -> io::Result<()> {
    let project = Project::new()?;
    let run = project.run_with_stdin(&["check", "--stdin", "--json"], SNIPPET)?;

    let parsed: serde_json::Value = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not JSON ({why}): {}", run.describe()));
    assert_eq!(parsed["summary"]["functions"], 1);
    assert_eq!(parsed["functions"][0]["bound_kind"], "exact");
    Ok(())
}
