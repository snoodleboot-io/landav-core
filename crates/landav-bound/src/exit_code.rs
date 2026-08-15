//! [`ExitCode`] - the process exit contract.

/// The process exit contract, frozen here so the CLI layer cannot invent one.
///
/// This enum is the code **space**, and nothing else. The mapping from an
/// analysis outcome onto it is *policy* - it prices an outcome - and this
/// crate does not decide policy, for the same reason [`crate::bound::Bound`]
/// implements no [`Ord`]: a decision made here cannot be revisited downstream.
/// The mapping lives in exactly one place, `landav_cli::outcome::Outcome::exit_code`,
/// and there is deliberately no second one on [`crate::verdict::Verdict`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExitCode {
    /// Nothing to report.
    Clean = 0,
    /// Analysis ran and has something to say about the code: a rule fired, or
    /// a unit was analysed without reaching a conclusion.
    ///
    /// **Reachable today**, on both of those routes. What is not yet decidable
    /// is the narrower reading this variant was first written for - "a bound
    /// exceeded its budget" - because comparing two symbolic bounds is
    /// semantic domination, which is F-018, which this crate deliberately does
    /// not decide. Recording that distinction now is what stops it being
    /// discovered during the first release.
    Findings = 1,
    /// The tool could not complete.
    ToolError = 2,
}

impl ExitCode {
    /// The numeric code.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::Clean => 0,
            Self::Findings => 1,
            Self::ToolError => 2,
        }
    }
}
