//! [`run`] - the process boundary itself.

use std::fs::File;
use std::io::Read as _;
use std::path::Path;
use std::process::{Command, Stdio};

use landav_its::Its;

use crate::{
    MAX_ANSWER_BYTES, POLL_INTERVAL_MILLIS, arg_map::ArgMap, config::Config, report::Report,
    solver::Solver, solver_error::SolverError, timeout::poll_budget, workspace::Workspace,
};

/// The file the emitted system is written to, inside the run's workspace.
const INPUT_STEM: &str = "system";
/// Where the child's standard output is captured.
const STDOUT_FILE: &str = "stdout.txt";
/// Where the child's standard error is captured.
const STDERR_FILE: &str = "stderr.txt";

/// Ask `solver` about `its`.
///
/// # The whole of the interface, in one function
///
/// Render the system, write it to a private directory, `exec` the solver with
/// the file named on its command line, wait for it under a wall clock, read
/// what it printed, and parse that. Nothing is linked, nothing is loaded into
/// this address space, and nothing about the solver's internals is visible
/// here. That is a licence requirement for LoAT and it is applied uniformly;
/// see [`Solver`].
///
/// # Why the child's output goes to files rather than pipes
///
/// A child writing to a pipe blocks when the pipe fills, and a parent that is
/// polling `try_wait` rather than draining the pipe never unblocks it - a
/// deadlock that looks exactly like a slow solver and is only caught by the
/// timeout. Redirecting to files removes the failure mode entirely rather than
/// managing it with reader threads, and the workspace they are written into is
/// removed on drop either way.
///
/// # Errors
///
/// Every way of not getting a bound: [`SolverError::NotInstalled`],
/// [`SolverError::Spawn`], [`SolverError::TimedOut`], [`SolverError::Killed`],
/// [`SolverError::Failed`], [`SolverError::Workspace`], and everything the
/// answer parser can refuse. All of them carry the solver's name, and
/// [`SolverError::blames`] turns any of them into a ledger, so a caller can
/// publish `omega` with a reason rather than dropping the function.
pub fn run(solver: Solver, its: &Its, config: &Config) -> Result<Report, SolverError> {
    let map = ArgMap::for_its(its)?;
    let workspace = Workspace::create(&config.workspace_root(), solver.program())?;

    let input = workspace.path(&format!("{INPUT_STEM}.{}", solver.input_extension()));
    write(&input, its.to_koat().as_bytes())?;

    let out_path = workspace.path(STDOUT_FILE);
    let err_path = workspace.path(STDERR_FILE);
    let program = config.program(solver);
    let mut child = Command::new(program)
        .args(solver.argv(&input, config.timeout()))
        .stdin(Stdio::null())
        .stdout(capture(&out_path)?)
        .stderr(capture(&err_path)?)
        .spawn()
        .map_err(|error| spawn_failure(solver, program, &error))?;

    let status = wait(&mut child, solver, config)?;
    let stdout = read_capped(&out_path);
    let stderr = read_capped(&err_path);

    match status.code() {
        Some(0) => {}
        Some(code) => {
            return Err(SolverError::Failed {
                solver,
                status: code,
                detail: excerpt(if stderr.trim().is_empty() {
                    &stdout
                } else {
                    &stderr
                }),
            });
        }
        // No code on Unix means the child died on a signal. It is an
        // observation made after the fact: the parent has already reaped the
        // child and returns normally.
        None => {
            return Err(SolverError::Killed {
                solver,
                detail: format!("{status}"),
            });
        }
    }

    let answer = solver.parse(&stdout, &map)?;
    Ok(Report::new(
        solver,
        answer,
        stdout.trim(),
        its.name().clone(),
        its.origin().clone(),
        &map,
    ))
}

/// Wait for `child`, killing it once the budget is spent.
///
/// The loop counts to [`poll_budget`] rather than looping on a clock. A loop
/// whose only exit is a time comparison becomes an infinite loop the moment
/// that comparison is weakened, and a hang is invisible to the panic lints and
/// indistinguishable from slow CI - the exact class of surviving mutant
/// `landav-bound/tests/frozen_invariants.rs` was written for. The clock is
/// still read, to stop early when the child finishes; it is simply no longer
/// the only thing that makes the loop end.
fn wait(
    child: &mut std::process::Child,
    solver: Solver,
    config: &Config,
) -> Result<std::process::ExitStatus, SolverError> {
    let budget = poll_budget(config.timeout());
    let deadline = std::time::Instant::now() + config.timeout().duration();

    for _ in 0..budget {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                return Err(SolverError::Spawn {
                    solver,
                    program: config.program(solver).display().to_string(),
                    detail: error.to_string(),
                });
            }
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MILLIS));
    }

    // One last look before killing: the child may have exited during the final
    // sleep, and reporting a timeout for a solver that answered would be a
    // lost bound.
    if let Ok(Some(status)) = child.try_wait() {
        return Ok(status);
    }
    let _ = child.kill();
    // Reap it, so the process table does not fill with zombies on a repository
    // where many functions time out.
    let _ = child.wait();
    Err(SolverError::TimedOut {
        solver,
        seconds: config.timeout().seconds(),
    })
}

/// Distinguish "the program is not there" from "the program is there and would
/// not start".
fn spawn_failure(solver: Solver, program: &Path, error: &std::io::Error) -> SolverError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return SolverError::NotInstalled {
            solver,
            program: program.display().to_string(),
            hint: solver.install_hint(),
        };
    }
    SolverError::Spawn {
        solver,
        program: program.display().to_string(),
        detail: error.to_string(),
    }
}

/// Create a capture file, refusing anything already at the path.
fn capture(path: &Path) -> Result<Stdio, SolverError> {
    // `create_new`, not `create`: the workspace was created atomically and is
    // ours, so anything already at this path is a surprise rather than a
    // leftover.
    File::create_new(path)
        .map(Stdio::from)
        .map_err(|error| SolverError::Workspace {
            root: path.display().to_string(),
            detail: error.to_string(),
        })
}

/// Write `contents` to a path that must not already exist.
fn write(path: &Path, contents: &[u8]) -> Result<(), SolverError> {
    use std::io::Write as _;
    File::create_new(path)
        .and_then(|mut file| file.write_all(contents))
        .map_err(|error| SolverError::Workspace {
            root: path.display().to_string(),
            detail: error.to_string(),
        })
}

/// Read at most [`MAX_ANSWER_BYTES`] of a capture file.
///
/// A read that fails yields the empty string, which the parsers report as
/// [`SolverError::NoAnswer`]. The cap is on the *read* as well as on the
/// parse: a solver that decided to print a gigabyte should not be able to make
/// this process allocate one.
fn read_capped(path: &Path) -> String {
    let mut buffer = Vec::new();
    if let Ok(file) = File::open(path) {
        // `MAX_ANSWER_BYTES + 1`, so that output *at* the cap is read whole
        // and output past it is visibly past it rather than silently truncated
        // to exactly the limit.
        let limit = MAX_ANSWER_BYTES.saturating_add(1);
        let _ = std::io::Read::take(file, limit as u64).read_to_end(&mut buffer);
    }
    String::from_utf8_lossy(&buffer).into_owned()
}

/// A bounded excerpt of what a failing solver said.
fn excerpt(text: &str) -> String {
    /// Enough to identify the complaint, not enough to paste a log into a
    /// diagnostic.
    const LIMIT: usize = 400;
    text.trim().chars().take(LIMIT).collect()
}
