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
use landav_bound::ResourceKind;

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

    /// Which resource to bound. LAN-60.
    ///
    /// The value parser is [`ResourceKind::parse`] itself — the registry's own
    /// conversion, and the only place in the program where a string becomes a
    /// resource. A `clap` `ValueEnum` mirroring the four names would be a
    /// second one, spelling the set a second time and rejecting values with a
    /// second message; both would drift from the registry the day an instance
    /// is registered, which is exactly what criterion 3 is about.
    ///
    /// Both help strings are rendered from the registry for the same reason.
    /// They are set through the builder rather than written as doc comments
    /// because a doc comment cannot be generated from a list that is allowed to
    /// grow.
    #[arg(
        long,
        value_name = "RESOURCE",
        value_parser = ResourceKind::parse,
        help = crate::resource::summary(),
        long_help = crate::resource::detail(),
    )]
    resource: Option<ResourceKind>,

    /// Report which constructs were out of scope, where, and what that leaves
    /// unanalysed. LAN-68.
    ///
    /// # Why a flag changes the exit code
    ///
    /// Passing it turns a run that could not lower everything into
    /// [`Outcome::Inconclusive`], which `--resource` already does for the same
    /// reason: a question was asked and the honest answer is "not fully".
    ///
    /// The escalation is *not* unconditional today, and that is a decision
    /// rather than an oversight. At this milestone no bound is derived from the
    /// lowering, so the default verdict — the `LAV0xx` rules — does not rest on
    /// it, and failing every real Python file on the reach of an M0 fragment
    /// produces a gate that gets switched off, which is the argument
    /// `crate::check::classify` already makes about an empty `__init__.py`.
    /// What is unconditional is the *reporting*: the ratio is on every run's
    /// summary line whether or not this flag is passed.
    ///
    /// When bound inference lands and the verdict does rest on the lowering,
    /// the change is to make this the default, not to revisit which outcome a
    /// refusal earns.
    #[arg(
        long,
        long_help = "Report which constructs were out of scope, where, and what that \
                     leaves unanalysed.\n\n\
                     Lists every refused construct with a position, the count for each, \
                     and the constructs in the vocabulary that were never met. A run \
                     that could not lower everything it was given reports as \
                     inconclusive rather than clean: a function that did not lower \
                     produces no transition system, so no bound covers it.\n\n\
                     The coverage ratio itself is on every run's summary line, with or \
                     without this flag."
    )]
    coverage: bool,

    /// Print the derived bound for each function that lowered.
    ///
    /// Separate from `--coverage`, which answers "what was skipped". This
    /// answers "what did the analysis conclude about what was not".
    #[arg(
        long,
        long_help = "Print the derived bound for each function that lowered.\n\n\
                     A bound is reported as Theta when it is an equality and O when \
                     it is only an upper bound. The distinction is not cosmetic: \
                     Theta means the analysis derived the cost exactly, and O means \
                     the true cost may be lower than the number shown.\n\n\
                     Functions the analysis could not bound are listed too, saying \
                     so. Silence about a function would read as a bound of zero.\n\n\
                     This reports the native engine only, so it needs no solver \
                     installed."
    )]
    bounds: bool,
}

/// Parse `argv` and run, returning the outcome to be mapped to an exit code.
///
/// Never panics and never exits the process itself — the exit code is decided
/// in exactly one place, and this is not it.
pub fn dispatch() -> Outcome {
    match Cli::try_parse() {
        Ok(cli) => match cli.command {
            Command::Check(args) => crate::check::run(
                &args.path,
                args.config.as_deref(),
                args.resource,
                args.coverage,
                args.bounds,
            ),
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

    /// `--resource`'s help is the registry's rendering, byte for byte.
    ///
    /// The integration suite checks that `--help` lists every registered
    /// resource, which a hand-written string listing today's four also
    /// satisfies. This checks where the text *came from*, so replacing the
    /// generated help with a literal that happens to match fails here rather
    /// than the first time an instance is registered.
    #[test]
    fn the_resource_help_is_the_registry_rendering() {
        let check = Cli::command().find_subcommand("check").cloned();
        assert!(check.is_some(), "`check` is the whole surface at M0");
        let Some(check) = check else { return };

        let arg = check
            .get_arguments()
            .find(|arg| arg.get_id() == "resource")
            .cloned();
        assert!(arg.is_some(), "`--resource` is LAN-60 criterion 1");
        let Some(arg) = arg else { return };

        assert_eq!(
            arg.get_help().map(ToString::to_string),
            Some(crate::resource::summary())
        );
        assert_eq!(
            arg.get_long_help().map(ToString::to_string),
            Some(crate::resource::detail())
        );
        assert_eq!(arg.get_value_names(), Some(["RESOURCE".into()].as_slice()));
    }
}
