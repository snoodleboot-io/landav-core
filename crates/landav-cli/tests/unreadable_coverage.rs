//! `LAN-82`: **coverage says what it is over, when that is not the whole
//! target.**
//!
//! A file the frontend cannot parse contributes no functions to the coverage
//! denominator, because they cannot be counted without being parsed. So a
//! directory holding one clean file and one file the parser rejects used to
//! report `coverage: 1 of 1 function(s) lowered (100%)`, while a function
//! nobody read sat in the second file.
//!
//! The verdict was already honest: the run is `inconclusive`, exits non-zero,
//! and names the file with its position and reason. What was not honest was
//! the percentage beside it - coverage did not drop, it silently shrank its own
//! base, which is the failure mode `LAN-82` opens with.
//!
//! # Annotated, not withheld
//!
//! Withholding `coverage_percent` whenever a file is unreadable would erase the
//! figure for every file that *was* read. So the number stays, its denominator
//! is stated, and the count of unreadable files is published beside it, in the
//! JSON and in the text.
//!
//! The 3.12 standard library had three such files among 570 when `LAN-82`
//! measured it, every one a PEP 701 f-string; `LAN-130` took that to **zero**
//! by changing the parser. What is tested here is the mechanism, not the
//! count: a file above the declared level still makes the denominator smaller
//! than the target, and still has to say so.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use serde_json::Value;

use common::{CLEAN_PY, Project};

/// An unparenthesised multi-except (PEP 758): valid Python 3.14, above the
/// 3.12 level this build declares, so the frontend refuses the file.
///
/// What this test needs is *a* file the parser will not read; which class it
/// falls into is `outcome_space.rs`'s subject. Until `LAN-130` this was a PEP
/// 701 f-string, which the parser now reads.
const UNREADABLE_PY: &str = "def uses_758(xs: list) -> int:\n    try:\n        go(xs)\n    except ValueError, TypeError:\n        pass\n    return 1\n";

fn json_of(run: &common::Run) -> Value {
    serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not valid JSON ({why}): {}", run.describe()))
}

/// **An unreadable file is counted beside the percentage it is excluded from.**
#[test]
fn an_unreadable_file_is_counted_in_the_summary() -> io::Result<()> {
    let project = Project::new()?;
    let root = project.mkdir("root")?;
    project.write("root/clean.py", CLEAN_PY)?;
    project.write("root/unreadable.py", UNREADABLE_PY)?;

    let run = project.check(&root, &["--json"])?;
    let summary = &json_of(&run)["summary"];

    assert_eq!(
        summary["unreadable_files"], 1,
        "one file could not be read, and a gate reading the percentage must be \
         able to see that its functions are in no count: {summary}"
    );
    assert!(
        summary["coverage_percent"].is_u64(),
        "the percentage over the files that *were* read is still reported - \
         withholding it would erase the figure for everything that parsed: \
         {summary}"
    );
    Ok(())
}

/// **A tree with nothing unreadable reports zero, not an absent field.**
///
/// Always present, so a gate watching it can see it change.
#[test]
fn a_readable_tree_reports_zero_unreadable_files() -> io::Result<()> {
    let project = Project::new()?;
    let root = project.mkdir("root")?;
    project.write("root/clean.py", CLEAN_PY)?;

    let run = project.check(&root, &["--json"])?;
    let summary = &json_of(&run)["summary"];

    assert_eq!(summary["unreadable_files"], 0, "{summary}");
    Ok(())
}

/// **The text coverage clause says what it is over.**
///
/// The line a human reads beside the verdict. Before `LAN-82` it read `1 of 1
/// function(s) lowered (100%)` with nothing to say a file was left out of it.
#[test]
fn the_text_coverage_clause_names_the_unreadable_files() -> io::Result<()> {
    let project = Project::new()?;
    let root = project.mkdir("root")?;
    project.write("root/clean.py", CLEAN_PY)?;
    project.write("root/unreadable.py", UNREADABLE_PY)?;

    let run = project.check(&root, &[])?;

    assert!(
        run.mentions("over the files that could be read")
            && run.mentions("1 file could not be read as Python"),
        "the coverage clause must say it is over the readable files, and how \
         many were not.\n{}",
        run.describe()
    );
    Ok(())
}

/// **A readable tree's clause is unchanged.** The qualification appears only
/// when there is something to qualify; a clean run's line reads as it did.
#[test]
fn a_readable_tree_clause_is_unqualified() -> io::Result<()> {
    let project = Project::new()?;
    let root = project.mkdir("root")?;
    project.write("root/clean.py", CLEAN_PY)?;

    let run = project.check(&root, &[])?;

    assert!(
        !run.mentions("over the files that could be read"),
        "nothing was left out, so there is nothing to qualify.\n{}",
        run.describe()
    );
    Ok(())
}
