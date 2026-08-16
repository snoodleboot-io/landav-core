//! [`SolverError`] - why no bound was obtained, always with a subject.

use landav_bound::{Assumption, Blame, Blames, Origin, Symbol};
use thiserror::Error;

use crate::{growth::Growth, solver::Solver};

/// Why an external solver produced no usable bound.
///
/// # Every variant names its subject
///
/// Non-negotiable 3 says a failure must name what it is about and why. There
/// is no `SolverError::Unknown`, no variant carrying only a `String`, and no
/// constructor that omits the solver. [`SolverError::blames`] turns any of
/// them into a [`Blames`] ledger naming the analysed function, so a caller
/// publishes `omega` **with** a reason rather than dropping the function from
/// its report - which is the difference between a partial answer and a bare
/// "unknown".
///
/// # Refusing is always available; guessing never is
///
/// Several variants exist only to say "this crate did not understand what the
/// solver said". [`SolverError::Unparsable`], [`SolverError::ClassMismatch`]
/// and [`SolverError::ArgIndexOutOfRange`] are all reachable from output a
/// future solver build could produce, and every one of them is preferred to
/// the alternative. An upper bound parsed from text this crate has not
/// verified may be smaller than the one the solver proved, and a reported
/// bound the program can exceed is the one failure class with a zero target.
///
/// `#[non_exhaustive]` because adding a reason should not break consumers.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SolverError {
    /// The binary is not on `PATH`, or the configured path does not resolve.
    #[error("{solver} is not installed: `{program}` could not be run. {hint}")]
    NotInstalled {
        /// Which solver was wanted.
        solver: Solver,
        /// The program name or path that was tried.
        program: String,
        /// What to install, and where from.
        hint: &'static str,
    },

    /// The binary resolved but could not be started.
    #[error("{solver}: `{program}` could not be started: {detail}")]
    Spawn {
        /// Which solver.
        solver: Solver,
        /// The program name or path that was tried.
        program: String,
        /// The operating system's complaint.
        detail: String,
    },

    /// The child outlived this crate's wall clock and was killed.
    #[error(
        "{solver} did not finish within {seconds}s and was stopped; \
         raise the timeout or reduce the function"
    )]
    TimedOut {
        /// Which solver.
        solver: Solver,
        /// The budget it exceeded.
        seconds: u64,
    },

    /// The solver stopped on its *own* clock and said so.
    ///
    /// Distinct from [`SolverError::TimedOut`], which is this crate killing
    /// the child. This one is the solver declining in an orderly way, and it
    /// means the analysis is genuinely hard rather than that the process was
    /// wedged.
    #[error("{solver} stopped on its own time limit before finding a bound")]
    SolverTimedOut {
        /// Which solver.
        solver: Solver,
    },

    /// The child exited with a non-zero status.
    #[error("{solver} exited with status {status}: {detail}")]
    Failed {
        /// Which solver.
        solver: Solver,
        /// The exit status.
        status: i32,
        /// The first of what the solver wrote to stderr.
        detail: String,
    },

    /// The child died on a signal.
    ///
    /// A subprocess that dies on a signal must not take landav with it: this
    /// is an observation about the child, made after the fact, and the parent
    /// returns normally.
    #[error("{solver} was killed before it answered: {detail}")]
    Killed {
        /// Which solver.
        solver: Solver,
        /// How it died, as far as the platform reports.
        detail: String,
    },

    /// The solver exited cleanly and printed nothing that could be an answer.
    ///
    /// Not the same as answering `inf`: that is a solver saying "no bound",
    /// which is a result. This is a solver saying nothing at all.
    #[error("{solver} produced no answer")]
    NoAnswer {
        /// Which solver.
        solver: Solver,
    },

    /// The solver's output is outside the grammar this crate has verified.
    #[error("{solver} printed something this build cannot read ({detail}): {at}")]
    Unparsable {
        /// Which solver.
        solver: Solver,
        /// An excerpt of what it printed.
        at: String,
        /// Which rule it broke.
        detail: &'static str,
    },

    /// The bound's own degree contradicts the growth class printed beside it.
    #[error(
        "{solver} announced {announced} but its bound `{text}` is {derived}; \
         the answer was not published"
    )]
    ClassMismatch {
        /// Which solver.
        solver: Solver,
        /// The class the solver stated.
        announced: Growth,
        /// The class the parsed expression actually has.
        derived: Growth,
        /// The expression as the solver wrote it.
        text: String,
    },

    /// The solver named a positional argument the system did not declare.
    ///
    /// The only way to reach this is for the solver and this crate to disagree
    /// about the variable tuple, which is the disagreement that would
    /// otherwise attribute a bound to the wrong variable.
    #[error(
        "{solver} named `Arg_{index}` but the system declares only {declared} \
         variable(s); the answer was not published"
    )]
    ArgIndexOutOfRange {
        /// Which solver.
        solver: Solver,
        /// The index it named.
        index: u32,
        /// How many variables the system declares.
        declared: usize,
    },

    /// An exponent past [`crate::MAX_EXPONENT`].
    #[error("an exponent of {got} exceeds the limit of {limit}; the answer was not published")]
    ExponentTooLarge {
        /// The exponent the solver printed.
        got: u64,
        /// The limit.
        limit: u32,
    },

    /// The solver printed more than [`crate::MAX_ANSWER_BYTES`].
    ///
    /// Refused rather than truncated: the prefix of `Arg_0^2+3` is `Arg_0`,
    /// which is a *smaller* upper bound than the one the solver stated.
    #[error("{solver} printed {got} bytes, past the {limit} byte limit; nothing was read")]
    OutputTooLarge {
        /// How much it printed, as far as was read.
        got: usize,
        /// The limit.
        limit: usize,
        /// Which solver.
        solver: Solver,
    },

    /// A system with more variables than [`crate::MAX_ARGS`].
    #[error("the system declares {got} variables, past the limit of {limit}")]
    TooManyVariables {
        /// How many it declares.
        got: usize,
        /// The limit.
        limit: usize,
    },

    /// A timeout outside the permitted range.
    #[error("a timeout of {got}s is outside the permitted range {min}s..={max}s")]
    TimeoutOutOfRange {
        /// The rejected value.
        got: u64,
        /// The smallest permitted value.
        min: u64,
        /// The largest permitted value.
        max: u64,
    },

    /// The working directory could not be created, written or read.
    #[error("the solver working directory under `{root}` could not be used: {detail}")]
    Workspace {
        /// Where it was being created.
        root: String,
        /// What went wrong.
        detail: String,
    },

    /// A report was filed under the direction its solver does not bound in.
    #[error("{solver} bounds {actual} bounds, and was filed as the {expected} bound")]
    DirectionMismatch {
        /// The misfiled solver.
        solver: Solver,
        /// The slot it was put in.
        expected: crate::direction::Direction,
        /// The direction it actually bounds in.
        actual: crate::direction::Direction,
    },

    /// The lower bound is above the upper bound.
    ///
    /// Impossible, so one of the two solvers is wrong and nothing in the
    /// output says which. Reported rather than reconciled: keeping the upper
    /// bound would publish a number there is positive evidence the program
    /// exceeds. See [`crate::Analysis`].
    #[error(
        "the lower bound Omega({lower}) exceeds the upper bound O({upper}); \
         one of the two solvers is wrong and nothing was published"
    )]
    Contradiction {
        /// The lower class.
        lower: Growth,
        /// The upper class.
        upper: Growth,
    },

    /// A parsed answer could not be published as a verdict.
    ///
    /// Reachable only if an `omega`-bearing bound reaches
    /// [`landav_bound::Verdict::classify`] with an empty ledger, which is a
    /// bug in this crate rather than a property of the solver's output.
    #[error("the answer could not be published: {detail}")]
    Unpublishable {
        /// What the bound algebra objected to.
        detail: String,
    },
}

impl SolverError {
    /// Which solver this failure is about, when it is about one.
    #[must_use]
    pub const fn solver(&self) -> Option<Solver> {
        match self {
            Self::NotInstalled { solver, .. }
            | Self::Spawn { solver, .. }
            | Self::TimedOut { solver, .. }
            | Self::SolverTimedOut { solver }
            | Self::Failed { solver, .. }
            | Self::Killed { solver, .. }
            | Self::NoAnswer { solver }
            | Self::Unparsable { solver, .. }
            | Self::ClassMismatch { solver, .. }
            | Self::ArgIndexOutOfRange { solver, .. }
            | Self::OutputTooLarge { solver, .. }
            | Self::DirectionMismatch { solver, .. } => Some(*solver),
            Self::ExponentTooLarge { .. }
            | Self::TooManyVariables { .. }
            | Self::TimeoutOutOfRange { .. }
            | Self::Workspace { .. }
            | Self::Contradiction { .. }
            | Self::Unpublishable { .. } => None,
        }
    }

    /// This failure as a blame ledger naming `function`.
    ///
    /// The `F-015` seam, and the reason a solver failure does not have to end
    /// the analysis of a file. A caller holding this can publish
    /// `Verdict::Partial` over `omega` with the reason attached, which is a
    /// partial answer; without it the only options are a bare "unknown" and
    /// silently omitting the function, and both are worse.
    ///
    /// Always exactly one record, and never empty.
    #[must_use]
    pub fn blames(&self, function: impl Into<Symbol>, origin: &Origin) -> Blames {
        let unaccounted = function.into();
        Blames::new(Blame {
            unaccounted,
            // `ResourceNotModelled` rather than `TerminationNotProved`: this
            // is the *bridge* failing, not the analysed program failing to be
            // provably terminating, and a coverage report that conflated the
            // two would send someone to rewrite a loop over a missing binary.
            assumption: Assumption::ResourceNotModelled {
                detail: Symbol::from(self.to_string()),
            },
            origin: origin.clone(),
        })
    }
}
