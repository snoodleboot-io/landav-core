//! `LAN-95`: **every file is analysed once, whatever it is named.**
//!
//! # The defect
//!
//! The directory walk resolved each entry with [`std::fs::metadata`], which
//! follows symbolic links, so a directory reached through one was descended
//! into as if it were its own. A CPython virtual environment contains
//! `lib64 -> lib`, which every project with a venv has, so every file under
//! `.venv/lib` was analysed twice - once under each path. Measured on a typed
//! backend: 3,895 `.py` files on disk, 7,689 walked.
//!
//! It was never unsound - the *ratios* are all preserved, so every proportional
//! conclusion drawn from such a run still holds - but every absolute number a
//! consumer reads doubled with it, and a consumer had no way to know:
//!
//! * findings reported twice, at two paths for one source line, so a gate
//!   counting findings sees double;
//! * the coverage denominator stated over a corpus that does not exist;
//! * `--resource` totals doubled, so a budget gate on `queries` sees twice the
//!   calls the program can issue;
//! * analysis time roughly doubled on any project with a venv.
//!
//! # Two rules, and the second is the one that makes the count correct
//!
//! **A directory symlink is not descended into.** The ordinary expectation for
//! a source tool - `ruff`, `mypy` and `pytest` all decline - and it fixes the
//! venv case outright, which is also what stops the wasted work.
//!
//! **A file is analysed under one name.** The walk still follows a `.py`
//! symlink deliberately, because it is source, and a bind mount or a hard link
//! reaches one file by two paths with no symlink involved. Keying on the
//! canonical path is what makes the count correct rather than usually-correct.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use serde_json::Value;

use common::{CLEAN_PY, EXIT_CLEAN, Project, Run};

/// The names of the functions the run reported, in order.
fn reported(run: &Run) -> Vec<String> {
    let parsed: Value = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not valid JSON ({why}): {}", run.describe()));
    parsed["functions"]
        .as_array()
        .map(|functions| {
            functions
                .iter()
                .map(|entry| entry["name"].as_str().unwrap_or("?").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The number of files the run says it analysed.
fn files_analysed(run: &Run) -> u64 {
    let parsed: Value = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not valid JSON ({why}): {}", run.describe()));
    parsed["summary"]["files_analysed"].as_u64().unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 1 · the venv shape
// ---------------------------------------------------------------------------

/// **`lib64 -> lib` does not double the corpus.**
///
/// The exact shape every CPython virtual environment has, reduced to two files.
#[cfg(unix)]
#[test]
fn a_directory_symlink_beside_its_target_analyses_each_file_once() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let project = Project::new()?;
    let root = project.mkdir("root")?;
    project.write("root/lib/one.py", CLEAN_PY)?;
    symlink("lib", root.join("lib64"))?;

    let run = project.check(&root, &["--json"])?;

    run.assert_did_not_crash();
    assert_eq!(
        files_analysed(&run),
        1,
        "`lib64` and `lib` are one directory holding one file, and a count \
         stated over a corpus twice its real size is one no consumer can \
         correct.\n{}",
        run.describe()
    );
    assert_eq!(
        reported(&run).len(),
        1,
        "the function was reported twice, at two paths for one source line - \
         which is what a CI gate counting findings sees double.\n{}",
        run.describe()
    );
    Ok(())
}

/// **A directory symlink pointing outside the target is not a way in.**
///
/// The security half of the same rule: `landav check .` must not be induced to
/// read files outside what it was pointed at. A tree that links to its
/// neighbour analyses its own file and not the neighbour's.
#[cfg(unix)]
#[test]
fn a_directory_symlink_out_of_the_target_is_not_followed() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let project = Project::new()?;
    let inside = project.mkdir("inside")?;
    project.write("inside/own.py", CLEAN_PY)?;
    project.write("outside/secret.py", CLEAN_PY)?;
    symlink(project.root().join("outside"), inside.join("escape"))?;

    let run = project.check(&inside, &["--json"])?;

    run.assert_did_not_crash();
    assert_eq!(
        files_analysed(&run),
        1,
        "only the file inside the target is this run's business.\n{}",
        run.describe()
    );
    assert!(
        !run.stdout.contains("secret.py"),
        "a symbolic link out of the target read a file outside it.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 2 · one file, two names
// ---------------------------------------------------------------------------

/// **A `.py` symlink to a file inside the target does not analyse it twice.**
///
/// The walk follows a `.py` symlink deliberately - it is source, and a tree
/// that links a module into place is an ordinary shape - so the file is
/// reachable twice and the deduplication is what keeps it counted once.
#[cfg(unix)]
#[test]
fn a_file_reachable_by_two_names_is_analysed_once() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let project = Project::new()?;
    let root = project.mkdir("root")?;
    project.write("root/real.py", CLEAN_PY)?;
    symlink(root.join("real.py"), root.join("alias.py"))?;

    let run = project.check(&root, &["--json"])?;

    run.assert_did_not_crash();
    assert_eq!(
        files_analysed(&run),
        1,
        "`alias.py` and `real.py` are one file.\n{}",
        run.describe()
    );
    Ok(())
}

/// **A hard link is the same file too.**
///
/// No symbolic link is involved, so the walk policy cannot see this one and
/// only the canonical-path key catches it. It is the case that makes the
/// deduplication a rule about *files* rather than a second spelling of the
/// walk rule.
#[cfg(unix)]
#[test]
fn a_hard_link_is_analysed_once() -> io::Result<()> {
    let project = Project::new()?;
    let root = project.mkdir("root")?;
    let real = project.write("root/real.py", CLEAN_PY)?;
    std::fs::hard_link(&real, root.join("linked.py"))?;

    let run = project.check(&root, &["--json"])?;

    run.assert_did_not_crash();
    assert_eq!(
        files_analysed(&run),
        1,
        "two names for one inode are one file to analyse.\n{}",
        run.describe()
    );
    Ok(())
}

/// **The surviving name does not depend on directory order.**
///
/// Two runs over one tree must report the same path, or a consumer diffing
/// them sees a file move between runs that never changed. The list is sorted
/// before deduplication, so the lexicographically first name survives.
#[cfg(unix)]
#[test]
fn the_surviving_name_is_stable() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let project = Project::new()?;
    let root = project.mkdir("root")?;
    project.write("root/zeta.py", CLEAN_PY)?;
    symlink(root.join("zeta.py"), root.join("alpha.py"))?;

    let first = project.check(&root, &["--json"])?;
    let second = project.check(&root, &["--json"])?;

    assert_eq!(files_analysed(&first), 1, "{}", first.describe());
    assert!(
        first.stdout.contains("alpha.py") && !first.stdout.contains("zeta.py"),
        "the first name in sorted order is the one reported.\n{}",
        first.describe()
    );
    assert_eq!(
        reported(&first),
        reported(&second),
        "two runs over one tree reported different files"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 3 · what must not change
// ---------------------------------------------------------------------------

/// **An ordinary tree is untouched.**
///
/// The fence. A rule that deduplicated too eagerly - by name, by size, by
/// content - would collapse the two identical modules every project has, and
/// the loss would look exactly like this ticket's fix working.
#[test]
fn two_distinct_files_with_identical_contents_are_both_analysed() -> io::Result<()> {
    let project = Project::new()?;
    let root = project.mkdir("root")?;
    project.write("root/one.py", CLEAN_PY)?;
    project.write("root/two.py", CLEAN_PY)?;

    let run = project.check(&root, &["--json"])?;

    run.assert_did_not_crash();
    assert_eq!(
        files_analysed(&run),
        2,
        "two files that happen to hold the same source are two files.\n{}",
        run.describe()
    );
    assert_eq!(run.code, EXIT_CLEAN, "{}", run.describe());
    Ok(())
}

/// **A real subdirectory is still descended into.**
///
/// The other half of the same fence: the walk must still walk.
#[test]
fn a_real_subdirectory_is_still_walked() -> io::Result<()> {
    let project = Project::new()?;
    let root = project.mkdir("root")?;
    project.write("root/top.py", CLEAN_PY)?;
    project.write("root/pkg/nested.py", CLEAN_PY)?;
    project.write("root/pkg/deeper/further.py", CLEAN_PY)?;

    let run = project.check(&root, &["--json"])?;

    run.assert_did_not_crash();
    assert_eq!(
        files_analysed(&run),
        3,
        "every real directory beneath the target is still walked.\n{}",
        run.describe()
    );
    Ok(())
}
