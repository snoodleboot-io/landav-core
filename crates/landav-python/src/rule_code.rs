//! [`RuleCode`] — the stable public identifier of a pattern rule.

use core::fmt;

/// The stable, permanent identifier of a Landav pattern rule, e.g. `LAV001`.
///
/// # Why `LAV` plus exactly three digits
///
/// * **`LAV` is unambiguous.** The codes appear in the same CI log as `ruff`
///   (`PERF`, `RUF`, `B`, `C4`), `pylint` (`R1702`) and `flake8` (`E501`).
///   A three-letter alphabetic prefix collides with none of them, and a reader
///   can attribute a code to Landav without a lookup table.
/// * **Fixed width means lexicographic order equals numeric order** for the
///   first thousand rules. Registry listings, `--explain` output and CI
///   baseline diffs are all sorted somewhere; a variable-width code makes that
///   sort disagree with the obvious one, and a baseline diff that reorders
///   itself is a baseline nobody trusts.
/// * **Blocks are reserved by concern**, so a code carries information before
///   it is looked up:
///
///   | Block | Concern |
///   |---|---|
///   | `LAV0xx` | superlinear / quadratic time patterns (`F-005`) |
///   | `LAV1xx` | reserved: memory-growth patterns |
///   | `LAV2xx` | reserved: findings derived from an inferred bound |
///
/// * **Codes are never reused and never renumbered.** A suppression comment
///   and a CI baseline both name a code; recycling `LAV004` for a different
///   rule silently changes what an existing suppression suppresses. Retiring a
///   rule retires its number with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleCode(&'static str);

impl RuleCode {
    /// Wraps a static code string.
    ///
    /// The registry is the only intended caller. There is deliberately no
    /// fallible parse from a runtime string: a code that is not in the
    /// registry is not a code, and [`crate::rule_for_code`] is the lookup.
    #[must_use]
    pub const fn new(code: &'static str) -> Self {
        Self(code)
    }

    /// The code as it appears in output and in suppression comments.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for RuleCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
