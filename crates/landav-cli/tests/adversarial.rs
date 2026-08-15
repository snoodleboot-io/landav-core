//! LAN-61 adversarial regression suite: attempts to make the exit code lie.
//!
//! Every test here started life as an attack on the contract in
//! `crate::outcome`. The ones that pass pin behaviour that is correct today
//! and would be easy to regress. The ones marked `#[ignore]` are **live
//! counterexamples**: they describe the behaviour the contract requires, they
//! fail against the implementation as it stands, and the `ignore` reason names
//! the defect. Run them with `cargo test -p landav-cli -- --ignored`.
//!
//! An `#[ignore]`d test is a bug that cannot be lost. Deleting one is a
//! decision to accept the defect; deleting the `#[ignore]` is the fix landing.

mod common;

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use common::{CLEAN_PY, EXIT_CLEAN, EXIT_FINDINGS, EXIT_TOOL_ERROR, FINDINGS_PY, Project};

// ---------------------------------------------------------------------------
// Pinned: behaviour that is correct today and must stay correct
// ---------------------------------------------------------------------------

/// A file the tool could not read must beat a finding, whichever order the
/// sorted walk reaches them in.
///
/// The aggregation is a claim about the whole target. "One file has a
/// quadratic loop" and "one file was never opened" are not comparable
/// severities: the second means the verdict does not cover the tree, so it has
/// to win. The two sub-cases differ only in filename, which is what decides
/// sort order, so a precedence that depended on which file was seen first
/// would show up as a disagreement between them.
#[cfg(unix)]
#[test]
fn an_unreadable_file_outranks_a_finding_in_both_walk_orders() -> io::Result<()> {
    use std::fs::{Permissions, set_permissions};
    use std::os::unix::fs::PermissionsExt;

    for (findings_name, unreadable_name) in [
        ("a_findings.py", "z_unreadable.py"),
        ("z_findings.py", "a_unreadable.py"),
    ] {
        let project = Project::new()?;
        let src = project.mkdir("src")?;
        project.write(&format!("src/{findings_name}"), FINDINGS_PY)?;
        let blocked = project.write(&format!("src/{unreadable_name}"), CLEAN_PY)?;
        set_permissions(&blocked, Permissions::from_mode(0o000))?;
        if std::fs::read(&blocked).is_ok() {
            // Running as root, or on a filesystem that ignores the mode bits.
            continue;
        }

        let run = project.check(&src, &[])?;
        set_permissions(&blocked, Permissions::from_mode(0o644))?;

        run.assert_did_not_crash();
        assert_eq!(
            run.code,
            EXIT_TOOL_ERROR,
            "a file whose bytes were never read must outrank a finding, so \
             that the verdict is never a claim about a subset of the tree \
             wearing the whole tree's name.\n{}",
            run.describe()
        );
        run.assert_explains(unreadable_name);
    }
    Ok(())
}

/// A `.py` symlink whose target is gone. The walk resolved it, failed, and
/// must blame the link rather than quietly analysing its clean neighbour and
/// reporting the directory clean.
#[cfg(unix)]
#[test]
fn a_dangling_python_symlink_is_not_absorbed_by_a_clean_neighbour() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let project = Project::new()?;
    let src = project.mkdir("src")?;
    project.write("src/clean.py", CLEAN_PY)?;
    symlink(src.join("moved_away.py"), src.join("broken.py"))?;

    let run = project.check(&src, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "a `.py` path the walk could not resolve was drowned by a clean \
         neighbour.\n{}",
        run.describe()
    );
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    run.assert_explains("broken.py");
    Ok(())
}

/// A subdirectory that cannot be listed. The files under it were never
/// enumerated, so the run covers less than the target it names.
#[cfg(unix)]
#[test]
fn an_unlistable_subdirectory_is_not_absorbed_by_a_clean_neighbour() -> io::Result<()> {
    use std::fs::{Permissions, set_permissions};
    use std::os::unix::fs::PermissionsExt;

    let project = Project::new()?;
    let src = project.mkdir("src")?;
    project.write("src/clean.py", CLEAN_PY)?;
    let hidden = project.mkdir("src/hidden")?;
    project.write("src/hidden/also_clean.py", CLEAN_PY)?;
    set_permissions(&hidden, Permissions::from_mode(0o000))?;
    if std::fs::read_dir(&hidden).is_ok() {
        return Ok(());
    }

    let run = project.check(&src, &[])?;
    set_permissions(&hidden, Permissions::from_mode(0o755))?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "a subtree that was never enumerated must not be reported as \
         analysed.\n{}",
        run.describe()
    );
    run.assert_explains("hidden");
    Ok(())
}

/// Byte-for-byte determinism, not just exit-code determinism.
///
/// CI diffs logs. `read_dir` order is filesystem-dependent and hash-map
/// iteration order is not stable between runs of the same binary, so a report
/// that is merely "the same findings" is not enough.
#[test]
fn stdout_and_stderr_are_byte_for_byte_identical_across_runs() -> io::Result<()> {
    let project = Project::new()?;
    let src = project.mkdir("src")?;
    project.write("src/a_clean.py", CLEAN_PY)?;
    project.write("src/b_findings.py", FINDINGS_PY)?;
    project.write("src/nested/c_findings.py", FINDINGS_PY)?;
    project.write("src/nested/deeper/d_clean.py", CLEAN_PY)?;

    let first = project.check(&src, &[])?;
    for attempt in 1..5 {
        let again = project.check(&src, &[])?;
        assert_eq!(
            first.stdout, again.stdout,
            "run {attempt} produced different stdout for identical input"
        );
        assert_eq!(
            first.stderr, again.stderr,
            "run {attempt} produced different stderr for identical input"
        );
        assert_eq!(
            first.code, again.code,
            "run {attempt} disagreed on the code"
        );
    }
    Ok(())
}

/// A glob the shell did not expand, because it matched nothing.
///
/// This is the "CI path stopped matching" case arriving as an argument rather
/// than as an empty directory: `landav check src/**/*.py` under a shell
/// without `nullglob` hands the pattern through verbatim. Reporting clean here
/// would make the job go green while checking no code at all.
#[test]
fn an_unexpanded_glob_is_a_tool_error_not_a_clean_run() -> io::Result<()> {
    let project = Project::new()?;
    project.write("src/clean.py", CLEAN_PY)?;

    for pattern in ["src/*.py", "src/**/*.py", "does_not_exist/*.py"] {
        let run = project.run(&["check", pattern])?;

        run.assert_did_not_crash();
        run.assert_code_is_sanctioned();
        assert_eq!(
            run.code,
            EXIT_TOOL_ERROR,
            "`{pattern}` is not a path that exists; landav does not expand \
             globs, so it must say so rather than report a verdict.\n{}",
            run.describe()
        );
        run.assert_explains(pattern);
    }
    Ok(())
}

/// Valid TOML chosen to break the parser rather than to configure anything.
///
/// Each of these must come back as a blamed `2`. A stack overflow on the
/// nested cases would be a signal death with no exit code at all.
#[test]
fn hostile_toml_is_refused_with_blame_and_never_crashes() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;

    let deep_arrays = format!("a = {}{}\n", "[".repeat(20_000), "]".repeat(20_000));
    let deep_tables = format!("[{}a]\n", "a.".repeat(20_000));
    let huge_array = format!("a = [{}]\n", "1,".repeat(200_000));
    let cases: [(&str, &str); 5] = [
        ("deep_arrays.toml", &deep_arrays),
        ("deep_tables.toml", &deep_tables),
        ("huge_array.toml", &huge_array),
        ("duplicate_keys.toml", "x = 1\nx = 2\n"),
        ("landav_is_a_scalar.toml", "[tool]\nlandav = 42\n"),
    ];

    for (name, body) in cases {
        let config = project.write(name, body)?;
        let run = project.check(&target, &["--config", &config.to_string_lossy()])?;

        run.assert_did_not_crash();
        run.assert_code_is_sanctioned();
        assert_eq!(
            run.code,
            EXIT_TOOL_ERROR,
            "`{name}` is configuration landav cannot honour, so it must be \
             refused rather than silently replaced by defaults.\n{}",
            run.describe()
        );
        run.assert_explains(name);
    }
    Ok(())
}

/// The empty-directory rule, applied to every shape of "nothing".
///
/// The rule exists so that a CI path which stops matching after a directory
/// move cannot go green forever. That argument does not distinguish between
/// the ways a target can turn out to hold no code, so neither may the exit
/// code.
#[test]
fn every_shape_of_nothing_analysed_is_a_tool_error() -> io::Result<()> {
    /// Populates a project with one shape of "nothing to analyse".
    type Shape = dyn Fn(&Project) -> io::Result<()>;

    let cases: [(&str, &Shape); 6] = [
        ("an empty directory", &|p: &Project| {
            p.mkdir("src").map(drop)
        }),
        ("only non-Python files", &|p: &Project| {
            p.write("src/README.md", "# nothing\n").map(drop)
        }),
        ("only a zero-byte .py", &|p: &Project| {
            p.write("src/empty.py", "").map(drop)
        }),
        ("only comments and blank lines", &|p: &Project| {
            p.write("src/comments.py", "\n\n# a comment\n\n").map(drop)
        }),
        ("only an empty subdirectory", &|p: &Project| {
            p.mkdir("src/nested").map(drop)
        }),
        ("only a directory that is named .py", &|p: &Project| {
            p.mkdir("src/package.py").map(drop)
        }),
    ];

    for (label, build) in cases {
        let project = Project::new()?;
        project.mkdir("src")?;
        build(&project)?;
        let src = project.root().join("src");

        let run = project.check(&src, &[])?;

        run.assert_did_not_crash();
        assert_ne!(
            run.code,
            EXIT_CLEAN,
            "{label}: analysing nothing reported clean.\n{}",
            run.describe()
        );
        assert_eq!(
            run.code,
            EXIT_TOOL_ERROR,
            "{label}: nothing analysed is a tool error, not a finding.\n{}",
            run.describe()
        );
        run.assert_explains("src");
    }
    Ok(())
}

/// A path with a newline in it. The tool must still terminate with a
/// sanctioned code and must still blame the right file.
///
/// The report format is `path:line: kind: rule: message`, one record per line,
/// so an embedded newline splits one record into two. That is a defect in the
/// *format*, filed as a NIT — but it must not become a defect in the *code*.
#[cfg(unix)]
#[test]
fn a_path_containing_a_newline_still_produces_a_sanctioned_code() -> io::Result<()> {
    let project = Project::new()?;
    let dir = project.mkdir("odd\nname")?;
    project.write("odd\nname/quadratic.py", FINDINGS_PY)?;

    let run = project.check(&dir, &[])?;

    run.assert_did_not_crash();
    run.assert_code_is_sanctioned();
    assert_eq!(
        run.code,
        EXIT_FINDINGS,
        "a hostile filename changed the verdict about the code inside \
         it.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Counterexamples: these fail, and the reason is the bug
// ---------------------------------------------------------------------------

/// A `.py` file that is not Python must not exit `0`.
///
/// Written as a blocker against the M0 line-and-indentation scanner, which
/// produced no observations for a file it could not make sense of and landed
/// it in [`Outcome::Clean`] — the one code that is both silent and trusted.
/// Now fixed: the frontend parses, and a file that does not parse is
/// `PythonError::Parse` → `Outcome::Inconclusive` → exit `1`, with the
/// position named.
///
/// # One case was dropped, deliberately
///
/// This test used to include `actually_json.py`, holding
/// `{"this": "is json", "not": "python"}`. Against a real parser that is a
/// dict display expression statement — **valid Python** — so the assertion's
/// stated reason, that the file "was never parsed", became false, and keeping
/// it would have meant a test passing for a reason its own message denied.
///
/// The instinct behind it survives the case: a `.py` whose entire body is one
/// collection display is almost certainly a renamed data file, and calling it
/// clean is a weak claim. But that is a judgement about Python, and
/// `CONTRIBUTING.md` non-negotiable 4 puts every language fact behind the
/// frontend. It belongs in `landav-python` as a rule with a corpus behind it,
/// not as an assertion in the CLI acceptance suite, and it has been routed
/// there as a proposal. Nothing in this suite depends on the answer.
#[test]
fn a_file_that_is_not_python_must_not_report_clean() -> io::Result<()> {
    let project = Project::new()?;
    let cases: [(&str, &str); 2] = [
        ("syntax_error.py", "def f(:\n    return ]]] @@@\n"),
        ("prose.py", "This file is a README that someone renamed.\n"),
    ];

    for (name, body) in cases {
        let target = project.write(name, body)?;

        let run = project.check(&target, &[])?;

        run.assert_did_not_crash();
        assert_ne!(
            run.code,
            EXIT_CLEAN,
            "`{name}` was never parsed, so `0` claims a property nobody \
             proved.\n{}",
            run.describe()
        );
    }
    Ok(())
}

/// BLOCKER. A `.py` path that is not a regular file is silently dropped by the
/// directory walk, and a clean neighbour then carries the whole tree to `0`.
///
/// `sources::walk` keeps an entry only when `resolved.is_file()`, so a FIFO, a
/// socket, or a symlink to a character device named `*.py` is neither analysed
/// nor blamed. The module documentation for that walk says the opposite in as
/// many words: "Everything the walk cannot resolve is a failure, not a skip."
///
/// The inconsistency is sharp in both directions. Naming the FIFO directly
/// exits `2` ("is neither a regular file nor a directory"); reaching the same
/// FIFO through its parent exits `0`. And a *dangling* `.py` symlink is a hard
/// error, while a `.py` symlink to `/dev/null` is dropped without a word.
#[cfg(unix)]
#[test]
fn a_python_path_that_is_not_a_regular_file_must_not_be_silently_skipped() -> io::Result<()> {
    let project = Project::new()?;
    let src = project.mkdir("src")?;
    project.write("src/clean.py", CLEAN_PY)?;
    make_fifo(&src.join("pipe.py"))?;
    std::os::unix::fs::symlink("/dev/null", src.join("devnull.py"))?;

    let run = project.check(&src, &[])?;

    run.assert_did_not_crash();
    assert_ne!(
        run.code,
        EXIT_CLEAN,
        "two `.py` paths were neither read nor blamed, and the run still \
         claimed the directory clean.\n{}",
        run.describe()
    );
    run.assert_explains("pipe.py");
    Ok(())
}

/// BLOCKER. A `pyproject.toml` that exists but cannot be `stat`ed is silently
/// discarded, and the run then reports "defaults (no configuration file)".
///
/// `config::discover` tests each candidate with `Path::is_file`, which folds
/// every `stat` error into `false`. A `pyproject.toml` that is a dangling
/// symlink, a symlink loop, or a directory therefore does not exist as far as
/// discovery is concerned, and the ascent carries on past it.
///
/// This is the exact failure the configuration module says it exists to
/// prevent: "Silently discarding configuration the user wrote is worse than
/// rejecting it: the run still reports a verdict, but under settings nobody
/// chose, and nothing in the output says so." The control and the experiment
/// below carry byte-identical configuration and differ only in whether the
/// path can be `stat`ed: one exits `2`, the other exits `0`.
#[cfg(unix)]
#[test]
fn a_pyproject_that_cannot_be_stated_must_not_be_silently_discarded() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    const REFUSED: &str = "[tool.landav]\nfail-on-partial = true\n";

    // Control: the same bytes, reachable. Refused by name, exit 2.
    let control = Project::new()?;
    let control_target = control.write("clean.py", CLEAN_PY)?;
    control.write("pyproject.toml", REFUSED)?;
    let baseline = control.check(&control_target, &[])?;
    assert_eq!(
        baseline.code,
        EXIT_TOOL_ERROR,
        "control case: a reachable pyproject.toml with an unknown key must \
         be refused.\n{}",
        baseline.describe()
    );

    // Experiment: the same bytes behind a link whose target moved away.
    let project = Project::new()?;
    let target = project.write("clean.py", CLEAN_PY)?;
    project.write("landav.toml", REFUSED)?;
    symlink("landav.toml", project.root().join("pyproject.toml"))?;
    std::fs::remove_file(project.root().join("landav.toml"))?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert!(
        !run.mentions("no configuration file"),
        "a pyproject.toml is sitting right there; claiming there is no \
         configuration file is a false statement about the run.\n{}",
        run.describe()
    );
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "configuration the tool could not read was silently replaced by \
         defaults, and the run reported a verdict under settings nobody \
         chose.\n{}",
        run.describe()
    );
    Ok(())
}

/// BLOCKER. Two paths to one directory are reported as a symlink loop.
///
/// `sources::walk` records every directory it has ever entered in a `visited`
/// set and never removes an entry on the way back out, so the set describes
/// "already seen" while the diagnostic claims "already being walked". A tree
/// with two routes to one directory — `pkg/` beside `alias -> pkg`, or two
/// symlinks into a shared library directory, both ordinary monorepo shapes —
/// is a DAG, not a cycle, and is completely traversable.
///
/// The result is a `2` on a healthy tree, with stderr asserting something
/// false about the user's filesystem, which is how a team decides the gate is
/// noise. Which of the two paths gets blamed is decided by `read_dir` order,
/// so the blame is not stable across filesystems either.
///
/// The fix is to make `visited` a stack of the directories currently on the
/// recursion path rather than a set of every directory ever entered.
#[cfg(unix)]
#[test]
fn two_paths_to_one_directory_are_not_a_symlink_loop() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let project = Project::new()?;
    let root = project.mkdir("root")?;
    project.write("root/shared/clean.py", CLEAN_PY)?;
    symlink("shared", root.join("alias"))?;

    let run = project.check(&root, &[])?;

    run.assert_did_not_crash();
    assert!(
        !run.mentions("cannot be traversed"),
        "the tree is a DAG and traverses fine; there is no loop to \
         report.\n{}",
        run.describe()
    );
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a fully traversable tree of clean code was declared \
         untraversable.\n{}",
        run.describe()
    );
    Ok(())
}

/// RISK. `--config` pointing at a FIFO with no writer never returns.
///
/// `config::read_toml` calls `read_to_string`, which blocks on open until a
/// writer appears. A process that never exits produces no exit code at all,
/// which is strictly worse than the wrong one: the CI job hangs until the
/// runner's own timeout kills it, and a killed job is triaged as
/// infrastructure flake rather than as a configuration error.
///
/// `--config /dev/zero` is the same defect with the opposite symptom: the read
/// never ends, so the process grows until the OOM killer takes it.
#[cfg(unix)]
#[test]
fn a_config_that_cannot_be_read_promptly_must_not_hang() -> io::Result<()> {
    let project = Project::new()?;
    project.write("clean.py", CLEAN_PY)?;
    let fifo = project.root().join("config.fifo");
    make_fifo(&fifo)?;

    let code = run_with_deadline(
        project.root(),
        &["check", "clean.py", "--config", &fifo.to_string_lossy()],
        Duration::from_secs(10),
    )?;

    assert!(
        code.is_some(),
        "the process was still running after 10s; a hang is not an exit code"
    );
    assert_eq!(
        code,
        Some(EXIT_TOOL_ERROR),
        "a configuration file that cannot be read promptly is a tool error"
    );
    Ok(())
}

/// RISK. The directory walk is roughly cubic in nesting depth.
///
/// `sources::walk` calls `canonicalize` on every directory it enters. Each
/// call costs one `readlink` per path component, and the component count grows
/// with depth, so the whole walk is `O(depth^3)` in syscalls. Measured on this
/// tree: depth 100 takes 0.04s, 200 takes 0.20s, 400 takes 1.4s, 800 takes
/// 10.5s, 1500 does not finish inside a minute.
///
/// That is a denial of service on the gate from attacker-controlled input — a
/// pull request only has to add a deep directory to make the check time out —
/// and a timed-out job carries no exit code. The `visited` set wants the
/// `(device, inode)` pair from the metadata already being read, not a
/// canonical path.
#[test]
fn the_directory_walk_is_not_cubic_in_depth() -> io::Result<()> {
    let project = Project::new()?;
    let mut nested = String::from("src");
    for _ in 0..700 {
        nested.push_str("/d");
    }
    project.write(&format!("{nested}/clean.py"), CLEAN_PY)?;
    let src = project.root().join("src");

    let started = Instant::now();
    let run = project.check(&src, &[])?;
    let elapsed = started.elapsed();

    run.assert_did_not_crash();
    assert_eq!(run.code, EXIT_CLEAN, "{}", run.describe());
    assert!(
        elapsed < Duration::from_secs(5),
        "walking a 700-deep tree took {elapsed:?}; nesting depth is \
         attacker-controlled and a timed-out gate has no exit code"
    );
    Ok(())
}

/// A line continuation must not hide a finding.
///
/// Written as a blocker against the M0 line-oriented scanner, which read
/// physical lines: `if x \` and `in known:` were two unrelated fragments and
/// neither matched the membership rule, so a file with both quadratic shapes
/// in it exited `0`. The module notes argued the direction was safe because it
/// only made the scan *less* likely to fire — the right instinct for a
/// false-positive budget, but the exit code has no "probably clean" value.
///
/// A real parser makes this structural rather than lexical, so the test now
/// guards a regression rather than reporting a defect: whatever tokenises the
/// source, the *logical* line is what a rule sees. The body is
/// `common::FINDINGS_PY` with continuations inserted at the two points that
/// used to break it, so this test and `exit_codes::file_with_findings_exits_one`
/// disagree only about whitespace.
#[test]
fn a_line_continuation_must_not_hide_a_finding() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write(
        "continued.py",
        "def summarise(rows, allowed):\n\
         \x20   known = list(allowed)\n\
         \x20   out = \"\"\n\
         \x20   for row in rows:\n\
         \x20       if row \\\n\
         \x20          in known:\n\
         \x20           out \\\n\
         \x20               += str(row)\n\
         \x20   return out\n",
    )?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_FINDINGS,
        "the same two quadratic shapes that exit 1 without continuations \
         exit 0 with them.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a FIFO at `path`, without pulling in a libc dependency.
#[cfg(unix)]
fn make_fifo(path: &Path) -> io::Result<()> {
    let status = std::process::Command::new("mkfifo").arg(path).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "mkfifo {} failed",
            path.display()
        )))
    }
}

/// Run the binary and give up after `deadline`, returning `None` if it was
/// still running. Output is inherited rather than captured, because capturing
/// it would make this wait on the pipes rather than on the process.
fn run_with_deadline(cwd: &Path, args: &[&str], deadline: Duration) -> io::Result<Option<i32>> {
    let mut child = std::process::Command::new(common::BIN)
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.code());
        }
        if started.elapsed() >= deadline {
            child.kill()?;
            let _ = child.wait()?;
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
