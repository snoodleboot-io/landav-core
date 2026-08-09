//! [`Rule`] — a rule's static identity and its documentation.

use crate::rule_code::RuleCode;

/// The static description of one pattern rule.
///
/// Documentation is a field rather than a doc comment because it is *output*:
/// `landav explain LAV003` has to print it, and a doc comment is not
/// reachable at run time. Acceptance criterion 1 of `LAN-65` — "each rule has
/// a code and documentation" — is therefore checkable by a test rather than by
/// a reviewer's memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    code: RuleCode,
    name: &'static str,
    summary: &'static str,
    documentation: &'static str,
}

impl Rule {
    /// Declares a rule. The registry is the only intended caller.
    #[must_use]
    pub const fn new(
        code: RuleCode,
        name: &'static str,
        summary: &'static str,
        documentation: &'static str,
    ) -> Self {
        Self {
            code,
            name,
            summary,
            documentation,
        }
    }

    /// The stable identifier, e.g. `LAV001`.
    #[must_use]
    pub const fn code(&self) -> RuleCode {
        self.code
    }

    /// A short `kebab-case` slug, e.g. `list-index-in-loop`.
    ///
    /// The fixture directory for a rule is `{code}_{name with '-' as '_'}`, so
    /// the corpus and the registry cannot drift apart unnoticed.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// One line, no trailing full stop, no newline — the text that appears on
    /// the finding line itself.
    #[must_use]
    pub const fn summary(&self) -> &'static str {
        self.summary
    }

    /// The long form: why the pattern is superlinear, and what to write
    /// instead. Printed by `landav explain`.
    #[must_use]
    pub const fn documentation(&self) -> &'static str {
        self.documentation
    }
}
