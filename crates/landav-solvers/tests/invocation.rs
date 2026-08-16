//! Crossing the process boundary.
//!
//! # Two populations of test, and only one of them needs a solver
//!
//! Everything about *reading* an answer is pure and lives in `koat_answers.rs`
//! and `loat_answers.rs`. What is left here is the part that spawns a process,
//! and it splits again:
//!
//! * **the failure modes** — a binary that is not installed, one that hangs,
//!   one that dies on a signal, one that exits non-zero, one that says
//!   nothing. Each is exercised against a three-line shell script this file
//!   writes itself, so every one of them runs on CI with neither solver
//!   present. These are the paths a user actually hits, and they are the ones
//!   most likely to be wrong, so they are not gated on anything.
//! * **the two live checks** — that a real KoAT answers a real system, and
//!   that its `Arg_i` numbering still means what this crate thinks it means.
//!   These need `koat2` on `PATH`.
//!
//! # Skipping is loud
//!
//! A gated test that quietly passes when its subject is absent is how a whole
//! class of defect hides: the suite is green, the count is unchanged, and
//! nobody learns that the interesting half never ran. So [`skip`] writes
//! directly to [`std::io::stderr`] rather than through `eprintln!`, which
//! libtest captures and discards on a passing test. The line appears in the CI
//! log unconditionally.
//!
//! Setting `LANDAV_REQUIRE_SOLVERS=1` turns every skip into a **failure**.
//! That is what a job which installs the solvers should set, so that a broken
//! installation is a red build rather than a silently narrower one.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write as _;
use std::path::{Path, PathBuf};

use landav_bound::Origin;
use landav_its::{ArithOp, CompareOp, Its, SourceProgramBuilder, VarName, lower};
use landav_solvers::{Answer, ArgMap, Config, Report, Solver, SolverError, Timeout, run};

/// Report that a test did not run, in a way the CI log keeps.
///
/// Returns `true` if the caller should skip. Panics — that is, fails the test —
/// when `LANDAV_REQUIRE_SOLVERS=1`, so a machine that is supposed to have the
/// solvers cannot lose them quietly.
fn skip(what: &str, why: &str) -> bool {
    let message = format!(
        "SKIPPED: {what}: {why}\n         set LANDAV_REQUIRE_SOLVERS=1 to make this a failure\n"
    );
    assert!(
        std::env::var("LANDAV_REQUIRE_SOLVERS").as_deref() != Ok("1"),
        "LANDAV_REQUIRE_SOLVERS=1 and {what} could not run: {why}"
    );
    // Not `eprintln!`: libtest captures the print macros and discards them for
    // a test that passes, which is exactly the case this line exists for.
    let _ = std::io::stderr().write_all(message.as_bytes());
    true
}

/// Whether `program` resolves to something executable on `PATH`.
fn installed(program: &str) -> bool {
    std::process::Command::new(program)
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// A throwaway directory for the stub scripts this file writes.
fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "landav-solvers-invocation-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Write an executable `/bin/sh` script that ignores its arguments.
#[cfg(unix)]
fn stub(name: &str, body: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    let path = scratch().join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).ok()?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).ok()?;
    Some(path)
}

/// `ETXTBSY`. Rust formats a raw OS error as `... (os error 26)`, and that
/// suffix is Rust's own rather than the platform's `strerror`, so it is stable
/// where the message text is not.
const TEXT_FILE_BUSY: &str = "os error 26";

/// The most attempts [`run_stub`] will make before giving up.
///
/// The race it works around is a fork-and-exec window measured in microseconds,
/// so a handful of retries is generous. A bound rather than a loop because a
/// genuinely un-executable stub must fail the test rather than hang it.
const BUSY_ATTEMPTS: usize = 20;

/// [`run`], retrying only while the stub is still held open for writing.
///
/// # The race
///
/// `stub` writes a file and then the test execs it. Meanwhile a sibling test
/// thread forks to spawn its own solver, and that child **inherits the still-open
/// write descriptor**. `CLOEXEC` closes it at `exec`, but the window between
/// `fork` and `exec` is real, and a kernel asked to execute a file that some
/// process holds open for writing refuses with `ETXTBSY`.
///
/// Measured at roughly one failure in seven full-suite runs. It only appears
/// under the harness's parallelism, which is why running the test alone always
/// passed and made it look like a flaky assertion rather than a flaky spawn.
///
/// # Why retry rather than remove the race
///
/// The obvious fixes do not work here. Renaming a freshly written file into
/// place does not help - `ETXTBSY` is a property of the inode, which `rename`
/// preserves. Probe-executing the stub before handing it over is worse: two of
/// these stubs are `sleep 600` and `kill -SEGV $$`, so the probe would hang the
/// suite or dump core. Creating every stub up front narrows the window but
/// cannot close it, because the tests that spawn a real solver fork during that
/// same burst.
///
/// So the race is inherent to `fork`/`exec` from a multi-threaded process and
/// is tolerated rather than eliminated. **The retry is confined to the tests.**
/// Putting it in `landav-solvers` would mean shipping a workaround for a
/// condition users effectively never meet, in the crate whose job is to report
/// spawn failures faithfully.
///
/// Only `ETXTBSY` is retried. Every other spawn failure - a missing binary
/// above all - is returned on the first attempt, so the tests that assert on
/// those still see them immediately.
#[cfg(unix)]
fn run_stub(solver: Solver, its: &Its, config: &Config) -> Result<Report, SolverError> {
    for attempt in 1..=BUSY_ATTEMPTS {
        let outcome = run(solver, its, config);
        let busy = matches!(
            &outcome,
            Err(SolverError::Spawn { detail, .. }) if detail.contains(TEXT_FILE_BUSY)
        );
        if !busy {
            return outcome;
        }
        assert!(
            attempt < BUSY_ATTEMPTS,
            "the stub was still held open for writing after {BUSY_ATTEMPTS} attempts, \
             which is far longer than the fork-and-exec window this works around. \
             Something is holding the file open, and that is a real defect rather \
             than the race: {outcome:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    unreachable!("the loop returns or asserts on its final attempt")
}

/// `def countdown(n): i = 0; while i < n: i = i + 1`, and optionally a leading
/// parameter that nothing in the body reads.
///
/// The dead parameter sorts first — the ITS holds its variables in name order
/// — so it occupies `Arg_0` and pushes `n` to `Arg_2`. That is what makes
/// [`the_argument_numbering_survives_koats_preprocessing`] able to see a
/// numbering that has shifted.
fn countdown(dead_first_parameter: bool) -> Option<Its> {
    let at = || Origin::new("probe.py:1");
    let mut params = vec![VarName::new("n")];
    if dead_first_parameter {
        params.insert(0, VarName::new("aaa"));
    }
    let mut build = SourceProgramBuilder::new("countdown", at(), params);

    let zero = build.int(0, at());
    let init = build.assign(VarName::new("i"), zero, at());

    let read_i = build.var(VarName::new("i"), at());
    let read_n = build.var(VarName::new("n"), at());
    let guard = build.compare(CompareOp::Lt, read_i, read_n, at());

    let again = build.var(VarName::new("i"), at());
    let one = build.int(1, at());
    let sum = build.arith(ArithOp::Add, again, one, at());
    let step = build.assign(VarName::new("i"), sum, at());

    let repeat = build.while_loop(guard, vec![step], at());
    lower(&build.build(vec![init, repeat])).ok()
}

/// The positional map the ITS declares.
fn map_of(its: &Its) -> ArgMap {
    ArgMap::for_its(its).unwrap_or_else(|_| ArgMap::empty())
}

// ---------------------------------------------------------------------------
// the failure modes, none of which needs a solver
// ---------------------------------------------------------------------------

/// A missing binary is the single most likely thing to go wrong on a user's
/// machine, and it must produce a sentence naming the executable and what to
/// install — not a panic, not a bare "unknown", and not a bound.
#[test]
fn a_solver_that_is_not_installed_names_itself_and_what_to_install() {
    let Some(its) = countdown(false) else {
        panic!("the countdown fragment must lower");
    };
    let config =
        Config::default().with_program(Solver::Koat, "landav-no-such-solver-binary-9f3a2b");
    let failed = run(Solver::Koat, &its, &config);
    let Err(SolverError::NotInstalled {
        solver,
        program,
        hint,
    }) = failed
    else {
        panic!("a missing binary must be NotInstalled, got {failed:?}");
    };
    assert_eq!(solver, Solver::Koat);
    assert!(program.contains("landav-no-such-solver-binary-9f3a2b"));
    assert!(
        hint.to_lowercase().contains("install"),
        "the hint must be actionable: {hint}"
    );
}

/// A missing binary must carry blame, so a caller can publish `omega` with a
/// named reason instead of dropping the function from the report.
#[test]
fn a_missing_solver_still_carries_blame() {
    let error = SolverError::NotInstalled {
        solver: Solver::Koat,
        program: "koat2".to_owned(),
        hint: Solver::Koat.install_hint(),
    };
    let blames = error.blames("countdown", &Origin::new("probe.py:1"));
    assert_eq!(blames.len(), 1);
    assert!(
        blames
            .as_slice()
            .iter()
            .any(|b| b.unaccounted.as_str() == "countdown"),
        "blame must name the function it is about: {blames:?}"
    );
}

/// An analyser that hangs on a user's CI is worse than one that declines. The
/// wall clock is this crate's, not the solver's, because a solver that ignores
/// its own `--timeout` — or has none, as LoAT does — must still stop.
#[cfg(unix)]
#[test]
fn a_solver_that_never_finishes_is_killed_at_the_deadline() {
    let Some(script) = stub("hangs.sh", "sleep 600") else {
        panic!("the stub script must be writable");
    };
    let Some(its) = countdown(false) else {
        panic!("the countdown fragment must lower");
    };
    let Ok(timeout) = Timeout::new(1) else {
        panic!("a one-second timeout is inside the permitted range");
    };
    let config = Config::default()
        .with_program(Solver::Koat, &script)
        .with_timeout(timeout);

    let started = std::time::Instant::now();
    let failed = run_stub(Solver::Koat, &its, &config);
    let elapsed = started.elapsed();

    assert!(
        matches!(failed, Err(SolverError::TimedOut { seconds, .. }) if seconds == 1),
        "a hung solver must time out, got {failed:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "the deadline was not enforced: the call took {elapsed:?}"
    );
}

/// A subprocess that dies on a signal must not take landav with it. The child
/// segfaults; the parent reports it by name and returns.
#[cfg(unix)]
#[test]
fn a_solver_that_dies_on_a_signal_is_reported_rather_than_propagated() {
    let Some(script) = stub("crashes.sh", "kill -SEGV $$") else {
        panic!("the stub script must be writable");
    };
    let Some(its) = countdown(false) else {
        panic!("the countdown fragment must lower");
    };
    let config = Config::default().with_program(Solver::Koat, &script);
    let failed = run_stub(Solver::Koat, &its, &config);
    assert!(
        matches!(failed, Err(SolverError::Killed { .. })),
        "a signalled child must be Killed, got {failed:?}"
    );
    let message = failed.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(
        message.contains("koat") || message.contains("KoAT"),
        "the failure must name the solver: {message}"
    );
}

/// A non-zero exit is how KoAT reports a system it could not parse. The exit
/// status and the first of what it said go into the message, because a bare
/// "the solver failed" sends the reader nowhere.
#[cfg(unix)]
#[test]
fn a_solver_that_exits_non_zero_reports_its_status_and_its_complaint() {
    let Some(script) = stub(
        "refuses.sh",
        "echo 'KoatParser error at line 1' >&2\nexit 125",
    ) else {
        panic!("the stub script must be writable");
    };
    let Some(its) = countdown(false) else {
        panic!("the countdown fragment must lower");
    };
    let config = Config::default().with_program(Solver::Koat, &script);
    let failed = run_stub(Solver::Koat, &its, &config);
    let Err(SolverError::Failed { status, detail, .. }) = failed else {
        panic!("a non-zero exit must be Failed, got {failed:?}");
    };
    assert_eq!(status, 125);
    assert!(
        detail.contains("KoatParser error"),
        "what the solver said must reach the message: {detail:?}"
    );
}

/// A solver that exits cleanly and says nothing has not answered. That is
/// distinct from answering `inf`, and distinct again from saying something
/// unreadable.
#[cfg(unix)]
#[test]
fn a_solver_that_says_nothing_is_not_treated_as_an_answer() {
    let Some(script) = stub("silent.sh", "exit 0") else {
        panic!("the stub script must be writable");
    };
    let Some(its) = countdown(false) else {
        panic!("the countdown fragment must lower");
    };
    let config = Config::default().with_program(Solver::Koat, &script);
    let failed = run_stub(Solver::Koat, &its, &config);
    assert!(
        matches!(failed, Err(SolverError::NoAnswer { .. })),
        "silence is not a bound, got {failed:?}"
    );
}

/// The system this crate hands the solver is the one `landav-its` emits,
/// unmodified. A stub that echoes its own input back proves the file reached
/// the child through the command line rather than through a shared address
/// space — which is the licence boundary as well as the interface.
#[cfg(unix)]
#[test]
fn the_system_reaches_the_solver_as_a_file_named_on_the_command_line() {
    let Some(script) = stub("echoes.sh", "cat \"$5\"") else {
        panic!("the stub script must be writable");
    };
    let Some(its) = countdown(false) else {
        panic!("the countdown fragment must lower");
    };
    let config = Config::default().with_program(Solver::Koat, &script);
    // The stub prints the ITS rather than a bound, so the parse fails — but it
    // fails *quoting what it read*, which is the assertion.
    let failed = run_stub(Solver::Koat, &its, &config);
    let message = failed.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(
        message.contains("GOAL COMPLEXITY") || message.contains("STARTTERM"),
        "the child did not receive the emitted system as its `-i` argument: {message}"
    );
}

// ---------------------------------------------------------------------------
// the two live checks
// ---------------------------------------------------------------------------

/// The whole bridge, end to end: a numeric function becomes an ITS, KoAT
/// bounds it, and the answer comes back as a `Bound` over the function's own
/// parameter.
#[test]
fn koat_bounds_a_countdown_loop_in_terms_of_its_parameter() {
    if !installed(Solver::Koat.program())
        && skip("koat_bounds_a_countdown_loop", "koat2 is not on PATH")
    {
        return;
    }
    let Some(its) = countdown(false) else {
        panic!("the countdown fragment must lower");
    };
    let report = match run(Solver::Koat, &its, &Config::default()) {
        Ok(report) => report,
        Err(error) => panic!("KoAT must answer the countdown loop: {error}"),
    };
    let Answer::Symbolic { bound, growth } = report.answer() else {
        panic!(
            "KoAT must find a bound for a counted loop, got {:?}",
            report.answer()
        );
    };
    assert_eq!(
        *growth,
        landav_solvers::Growth::Polynomial(1),
        "a countdown loop is linear; KoAT said {}",
        report.raw()
    );
    let named: Vec<String> = bound.vars().iter().map(ToString::to_string).collect();
    assert_eq!(
        named,
        vec!["n".to_owned()],
        "the bound must be in terms of the parameter `n` and nothing else; KoAT said {}",
        report.raw()
    );
}

/// The check that pins `Arg_i` to a position, against the one KoAT behaviour
/// that silently breaks it.
///
/// KoAT's default preprocessing includes `eliminate`, which drops variables
/// that "do not contribute to the problem" **and renumbers the survivors**. A
/// system declaring `(VAR vaaa vi vn)` whose `vaaa` is never read answers about
/// `Arg_1` where this crate expects `Arg_2`, and the bound is then attributed
/// to the loop counter instead of to the parameter — a wrong answer that looks
/// entirely right.
///
/// `Solver::argv` therefore omits `eliminate`. This test is the only thing
/// that can tell whether the flag still works: it builds a system with a dead
/// leading parameter and asserts the bound names `n`.
#[test]
fn the_argument_numbering_survives_koats_preprocessing() {
    if !installed(Solver::Koat.program())
        && skip(
            "the_argument_numbering_survives_koats_preprocessing",
            "koat2 is not on PATH, so the Arg_i mapping is pinned only by the argv test",
        )
    {
        return;
    }
    let Some(its) = countdown(true) else {
        panic!("the countdown fragment with a dead parameter must lower");
    };
    let declared: Vec<String> = its.vars().iter().map(ToString::to_string).collect();
    assert_eq!(
        declared,
        vec!["aaa".to_owned(), "i".to_owned(), "n".to_owned()],
        "the dead parameter must sort first, or this test is not testing anything"
    );

    let report = match run(Solver::Koat, &its, &Config::default()) {
        Ok(report) => report,
        Err(error) => panic!("KoAT must answer the countdown loop: {error}"),
    };
    let Answer::Symbolic { bound, .. } = report.answer() else {
        panic!("KoAT must find a bound, got {:?}", report.answer());
    };
    let named: Vec<String> = bound.vars().iter().map(ToString::to_string).collect();
    assert_eq!(
        named,
        vec!["n".to_owned()],
        "the bound must name `n`. Naming `i` or `aaa` means KoAT eliminated the dead \
         variable and renumbered, so every Arg_i in this crate is off by one. KoAT said {}",
        report.raw()
    );
    assert_eq!(map_of(&its).len(), 3);
}

/// LoAT is invoked the same way KoAT is, and whatever it answers, the answer
/// is a `Result` rather than a panic. LoAT 0.9.10 has no reader for the KoAT
/// ITS format and rejects the file outright; that is a named failure, and this
/// test asserts only that it *is* one.
#[test]
fn loat_is_invoked_across_the_same_process_boundary() {
    if !installed(Solver::Loat.program())
        && skip(
            "loat_is_invoked_across_the_same_process_boundary",
            "loat is not on PATH",
        )
    {
        return;
    }
    let Some(its) = countdown(false) else {
        panic!("the countdown fragment must lower");
    };
    let outcome = run(Solver::Loat, &its, &Config::default());
    match outcome {
        Ok(report) => assert_eq!(report.direction(), landav_solvers::Direction::Lower),
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("loat") || message.contains("LoAT"),
                "a LoAT failure must name LoAT: {message}"
            );
        }
    }
}

/// The working directory a run creates is removed when the run ends, whether
/// it succeeded or not. A gate that leaks one directory per analysed function
/// fills `/tmp` on a monorepo, and the input it leaks is the user's source in
/// another form.
#[cfg(unix)]
#[test]
fn a_run_leaves_no_working_directory_behind() {
    let Some(script) = stub("silent2.sh", "exit 0") else {
        panic!("the stub script must be writable");
    };
    let Some(its) = countdown(false) else {
        panic!("the countdown fragment must lower");
    };
    let root = scratch().join("workspaces");
    let _ = std::fs::remove_dir_all(&root);
    let Ok(()) = std::fs::create_dir_all(&root) else {
        panic!("the workspace root must be creatable");
    };

    let config = Config::default()
        .with_program(Solver::Koat, &script)
        .with_workspace_root(&root);
    for _ in 0..3 {
        let _ = run(Solver::Koat, &its, &config);
    }

    let left = std::fs::read_dir(&root)
        .map(|listing| listing.filter_map(Result::ok).count())
        .unwrap_or(usize::MAX);
    assert_eq!(
        left,
        0,
        "three runs left {left} working directories under {}",
        root.display()
    );
}

/// A workspace root that cannot be written to is a named failure, not a
/// panic — a read-only or full filesystem is an ordinary CI condition.
#[test]
fn an_unusable_workspace_root_is_a_named_failure() {
    let Some(its) = countdown(false) else {
        panic!("the countdown fragment must lower");
    };
    let config =
        Config::default().with_workspace_root("/landav-no-such-root-4c1d/definitely/not/here");
    let failed = run(Solver::Koat, &its, &config);
    assert!(
        matches!(failed, Err(SolverError::Workspace { .. })),
        "an unusable workspace root must be Workspace, got {failed:?}"
    );
}

/// `Path` is only used through the config, so a program given as a bare name
/// is resolved on `PATH` and one given as a path is not.
#[test]
fn a_program_may_be_named_or_given_as_a_path() {
    let config = Config::default();
    assert_eq!(config.program(Solver::Koat), Path::new("koat2"));
    assert_eq!(config.program(Solver::Loat), Path::new("loat"));
    let overridden = Config::default().with_program(Solver::Loat, "/opt/loat/bin/loat");
    assert_eq!(
        overridden.program(Solver::Loat),
        Path::new("/opt/loat/bin/loat")
    );
    assert_eq!(
        overridden.program(Solver::Koat),
        Path::new("koat2"),
        "overriding one program must not move the other"
    );
}

/// The retry above is a workaround for a race that is, by nature, hard to
/// reproduce on demand - it was measured at roughly one full-suite run in
/// seven, and it stops appearing entirely when the machine is quiet. A fix for
/// a defect that will not reproduce is a fix nobody can check.
///
/// So the condition is created deliberately rather than waited for. Holding the
/// stub open for writing is exactly what a forked sibling's inherited
/// descriptor does, and the kernel refuses to execute it for the same reason.
///
/// This asserts two things the timing-based runs cannot: that `ETXTBSY`
/// **does** reach this crate as a `Spawn` error carrying the OS detail, and
/// that [`run_stub`] recovers once the descriptor is released.
#[cfg(unix)]
#[test]
fn the_busy_retry_recovers_from_a_deliberately_held_descriptor() {
    let Some(script) = stub("held.sh", "exit 0") else {
        panic!("the stub script must be writable");
    };
    let Some(its) = countdown(false) else {
        panic!("the countdown fragment must lower");
    };
    let config = Config::default().with_program(Solver::Koat, &script);

    // First, prove the condition is real: while a write handle is open, the
    // spawn must fail with exactly the error the retry keys on. Without this
    // the test below could pass because nothing was ever busy.
    let holder = std::fs::OpenOptions::new()
        .write(true)
        .open(&script)
        .expect("the stub must be re-openable for writing");
    let blocked = run(Solver::Koat, &its, &config);
    let Err(SolverError::Spawn { detail, .. }) = &blocked else {
        drop(holder);
        panic!(
            "holding a write descriptor must make the spawn fail, got {blocked:?}. \
             If this platform no longer refuses, the retry is dead code and should go."
        );
    };
    assert!(
        detail.contains(TEXT_FILE_BUSY),
        "the busy condition must arrive as `{TEXT_FILE_BUSY}`, or the retry keys \
         on the wrong thing and will never fire: {detail:?}"
    );

    // Now release it on a timer and confirm the retry rides it out. The delay
    // is comfortably inside the retry budget and comfortably longer than one
    // attempt, so a `run` without the retry would still be blocked.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(25));
        drop(holder);
    });

    let recovered = run_stub(Solver::Koat, &its, &config);
    assert!(
        matches!(recovered, Err(SolverError::NoAnswer { .. })),
        "once the descriptor is released the stub should run and say nothing, \
         which is `NoAnswer`; got {recovered:?}"
    );
}
