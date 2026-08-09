//! [`Finding`] — one rule firing at one place.

use crate::{location::Location, rule_code::RuleCode};

/// A pattern rule firing at a specific position, with a one-line reason.
///
/// Acceptance criterion 3 of `LAN-65` is encoded in the type: there is no way
/// to construct a `Finding` without a file, a line, a column and an
/// explanation, so "the finding had no position" is not a reachable state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    rule: RuleCode,
    location: Location,
    explanation: String,
}

impl Finding {
    /// Builds a finding.
    ///
    /// `explanation` must be a single line: it is printed on the same row as
    /// the position, and an embedded newline breaks every consumer that
    /// assumes one finding is one line. `tests/fixture_corpus.rs` asserts it.
    #[must_use]
    pub fn new(rule: RuleCode, location: Location, explanation: String) -> Self {
        Self {
            rule,
            location,
            explanation,
        }
    }

    /// Which rule fired.
    #[must_use]
    pub const fn rule(&self) -> RuleCode {
        self.rule
    }

    /// Where it fired.
    #[must_use]
    pub const fn location(&self) -> &Location {
        &self.location
    }

    /// Why, in one line, phrased so that it names the term that makes the cost
    /// superlinear rather than merely naming the pattern.
    #[must_use]
    pub fn explanation(&self) -> &str {
        &self.explanation
    }
}
