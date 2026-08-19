//! [`Outcome`] — the total outcome space of a `check` run, and its mapping
//! onto the frozen exit-code contract.
//!
//! # Why this is an enum and not an integer
//!
//! LAN-61 criterion 3 requires that *every* analysis outcome map to exactly
//! one exit code, including "analysed, but could not reach a conclusion".
//! Integration tests can assert that the code is always in `{0, 1, 2}` and
//! that the known outcomes map correctly, but **totality is a property of the
//! match, not of any finite set of examples**. It is therefore expressed here
//! as an exhaustive `match` with no wildcard arm: adding a variant to
//! [`Outcome`] is a compile error until somebody decides which of the three
//! codes it earns.
//!
//! A `_ => ExitCode::Clean` fallback would turn that compile error into a
//! green build for an outcome nobody classified, which is the exact failure
//! mode the criterion exists to prevent.
//!
//! # This is the only mapping onto [`ExitCode`], anywhere
//!
//! [`landav_bound`] declares the code space and, since LAN-61, prices nothing
//! with it: `Verdict::exit_code` used to answer the same question this module
//! answers - what is "analysed, but no conclusion reached" worth? - and
//! answered it differently, with `Clean` or `ToolError` against this module's
//! `Findings`. Two mappings for one state is not a mapping. The library keeps
//! the *fact* (`Verdict::is_conclusive`, `Verdict::blames`); the price is set
//! here, and here only, because this is the layer that has a process to exit.

use landav_bound::ExitCode;

/// Everything a `check` run can end as.
///
/// The variants are the outcome space, not the code space: two of them share
/// [`ExitCode::ToolError`] and two share [`ExitCode::Findings`]. Collapsing
/// them at this level would lose the distinction the diagnostics are written
/// against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Analysis ran over at least one unit and every bound held.
    Clean,
    /// Analysis ran and at least one rule fired against the analysed code.
    Findings,
    /// Analysis ran but could not discharge every assumption: a bound exists
    /// for some unit only as a partial result carrying blame.
    ///
    /// This is the variant criterion 3 was written for. It must never share a
    /// code with [`Outcome::Clean`].
    Inconclusive,
    /// The target resolved, but contained nothing to analyse.
    ///
    /// Distinct from [`Outcome::Clean`] on purpose. A CI path that stops
    /// matching after a directory move analyses nothing; if that reported
    /// clean the job would go green forever while checking no code at all.
    NothingAnalysed,
    /// The tool could not look: unreadable input, unusable configuration, or a
    /// usage error.
    Failed,
}

impl Outcome {
    /// A stable name for machine-readable output.
    ///
    /// Deliberately not `Debug`: a derived representation is a rendering
    /// detail that a refactor may change, and this is a value consumers branch
    /// on. Spelled out so that renaming a variant is a visible decision here
    /// rather than a silent break in someone's CI gate.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Findings => "findings",
            Self::Inconclusive => "inconclusive",
            Self::NothingAnalysed => "nothing-analysed",
            Self::Failed => "failed",
        }
    }
}

impl Outcome {
    /// The process exit code for this outcome.
    ///
    /// **Total, and deliberately written with one arm per variant and no
    /// wildcard.** See the module documentation.
    ///
    /// [`Outcome::Inconclusive`] maps to [`ExitCode::Findings`] rather than
    /// [`ExitCode::ToolError`]. The analyser ran to completion and produced a
    /// result *about the code* — a partial bound naming the term it could not
    /// account for — so the person who can act on it is the author of the
    /// code, the same audience as any other finding. [`ExitCode::ToolError`]
    /// is reserved for "the tool could not complete", and putting a `while`
    /// loop the analyser could not bound into the same bucket as an unreadable
    /// file would page the wrong team and, worse, teach them that code `2` is
    /// noise to be switched off.
    pub const fn exit_code(self) -> ExitCode {
        match self {
            Self::Clean => ExitCode::Clean,
            Self::Findings => ExitCode::Findings,
            Self::Inconclusive => ExitCode::Findings,
            Self::NothingAnalysed => ExitCode::ToolError,
            Self::Failed => ExitCode::ToolError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Outcome;
    use landav_bound::ExitCode;

    /// Every outcome lands inside the frozen contract.
    #[test]
    fn every_outcome_maps_into_the_contract() {
        for outcome in [
            Outcome::Clean,
            Outcome::Findings,
            Outcome::Inconclusive,
            Outcome::NothingAnalysed,
            Outcome::Failed,
        ] {
            assert!(matches!(
                outcome.exit_code(),
                ExitCode::Clean | ExitCode::Findings | ExitCode::ToolError
            ));
        }
    }

    /// The hole criterion 3 closes: nothing but a proven-clean run reports 0.
    #[test]
    fn only_a_clean_run_reports_clean() {
        for outcome in [
            Outcome::Findings,
            Outcome::Inconclusive,
            Outcome::NothingAnalysed,
            Outcome::Failed,
        ] {
            assert_ne!(outcome.exit_code(), ExitCode::Clean, "{outcome:?}");
        }
        assert_eq!(Outcome::Clean.exit_code(), ExitCode::Clean);
    }

    /// The `1` versus `2` split, in both directions.
    #[test]
    fn tool_failures_and_findings_do_not_share_a_code() {
        assert_eq!(Outcome::Findings.exit_code(), ExitCode::Findings);
        assert_eq!(Outcome::Failed.exit_code(), ExitCode::ToolError);
        assert_eq!(Outcome::NothingAnalysed.exit_code(), ExitCode::ToolError);
        assert_ne!(
            Outcome::Failed.exit_code(),
            Outcome::Findings.exit_code(),
            "a tool error must not be indistinguishable from a finding"
        );
    }
}
