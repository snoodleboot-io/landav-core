//! [`ExitCode`] - the process exit contract.

/// The process exit contract, frozen here so the CLI layer cannot invent one.
///
/// `Findings` is **unreachable in M0**. Deciding "does this bound exceed the
/// budget" requires semantic domination on symbolic bounds, which is F-018,
/// which this crate deliberately does not decide. Recording it now is what
/// stops it being discovered during the first release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExitCode {
    /// Nothing to report.
    Clean = 0,
    /// A bound exceeded its budget. Unreachable until F-018 lands.
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
