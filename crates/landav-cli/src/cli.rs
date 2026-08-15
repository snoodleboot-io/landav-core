//! Argument parsing and dispatch.
//!
//! # Usage errors are tool errors
//!
//! A misspelled subcommand or an unrecognised flag exits `2`, the same code as
//! any other "the tool could not do what you asked". The alternative —
//! ignoring the flag and analysing anyway — runs under settings the caller did
//! not choose and then reports a verdict for them, which is the silent-config
//! failure wearing an argv hat. `clap` reaches the same number by the same
//! reasoning, but the mapping is written out here rather than inherited, since
//! the exit contract is ours and not a dependency's.

use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{Args, Parser, Subcommand};

use crate::outcome::Outcome;

/// Landav derives cost bounds for your code and fails the build when one is
/// exceeded.
#[derive(Debug, Parser)]
#[command(
    name = "landav",
    version,
    about = "Derive and check cost bounds.",
    long_about = None,
)]
struct Cli {
    /// What to do.
    #[command(subcommand)]
    command: Command,
}

/// The subcommands. `check` is the whole surface at M0.
#[derive(Debug, Subcommand)]
enum Command {
    /// Analyse PATH and report what could be concluded about it.
    Check(CheckArgs),
}

/// Arguments to `landav check`.
#[derive(Debug, Args)]
struct CheckArgs {
    /// File or directory to analyse.
    #[arg(value_name = "PATH")]
    path: PathBuf,

    /// Read configuration from FILE, instead of discovering `pyproject.toml`.
    ///
    /// The file replaces discovery outright: a `[tool.landav]` section
    /// elsewhere is not merged into it and cannot rescue it.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
}

/// Parse `argv` and run, returning the outcome to be mapped to an exit code.
///
/// Never panics and never exits the process itself — the exit code is decided
/// in exactly one place, and this is not it.
pub fn dispatch() -> Outcome {
    match Cli::try_parse() {
        Ok(cli) => match cli.command {
            Command::Check(args) => crate::check::run(&args.path, args.config.as_deref()),
        },
        Err(error) => usage(&error),
    }
}

/// Render a `clap` outcome and classify it.
///
/// `--help` and `--version` are not failures: the caller asked for output and
/// got it. Everything else is a usage error, and `clap` has already written a
/// message naming the offending argument to stderr.
fn usage(error: &clap::Error) -> Outcome {
    let _ = error.print();
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => Outcome::Clean,
        _ => Outcome::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory as _;

    /// `clap`'s own consistency checks: duplicate flags, bad value names, and
    /// so on. A malformed command definition otherwise only shows up at
    /// runtime, as a panic, in a binary that has promised never to panic.
    #[test]
    fn the_command_definition_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn the_binary_is_named_landav_not_pycost() {
        assert_eq!(Cli::command().get_name(), "landav");
    }
}
