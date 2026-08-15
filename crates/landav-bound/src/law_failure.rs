//! [`LawFailure`] - a law violation, named.

use crate::{law::Law, semiring_id::SemiringId};

/// A law the instance failed, with enough context to reproduce it.
///
/// The suite returns this rather than panicking, so that it can be run from
/// library code behind the `laws` feature without violating "never panic in
/// library code".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{semiring} violates {law}: {detail}")]
pub struct LawFailure {
    /// Which instance.
    pub semiring: SemiringId,
    /// Which law.
    pub law: Law,
    /// The offending elements and valuation, rendered.
    pub detail: String,
}

impl core::fmt::Display for SemiringId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod rendering {
    use super::LawFailure;
    use crate::{law::Law, semiring_id::SemiringId};

    /// The failure is what a developer reads when a future semiring breaks a
    /// law, so all three fields have to reach the message. A `Display` that
    /// dropped the detail would still satisfy `Debug`, and the suite returns
    /// `Result` rather than panicking precisely so this string is the whole
    /// diagnostic.
    #[test]
    fn display_names_the_instance_the_law_and_the_detail() {
        let failure = LawFailure {
            semiring: SemiringId::new("peak"),
            law: Law::Antisymmetry,
            detail: "a = Elem(3), b = Elem(5)".to_owned(),
        };
        assert_eq!(
            failure.to_string(),
            "peak violates L7: a = Elem(3), b = Elem(5)"
        );
    }

    /// `SemiringId`'s `Display` is the bare name, with no wrapper: the
    /// message above reads "peak violates ...", not "SemiringId(\"peak\")".
    #[test]
    fn semiring_id_displays_as_its_name() {
        assert_eq!(SemiringId::new("additive").to_string(), "additive");
        assert_eq!(SemiringId::new("").to_string(), "");
    }

    /// Two failures naming different laws are different values. The suite's
    /// own tests compare failures, so `PartialEq` may not ignore a field.
    #[test]
    fn equality_distinguishes_every_field() {
        let base = LawFailure {
            semiring: SemiringId::new("additive"),
            law: Law::NonDegeneracy,
            detail: "zero == one".to_owned(),
        };
        assert_eq!(base, base.clone());
        assert_ne!(
            base,
            LawFailure {
                law: Law::StarAtZero,
                ..base.clone()
            }
        );
        assert_ne!(
            base,
            LawFailure {
                semiring: SemiringId::new("peak"),
                ..base.clone()
            }
        );
        assert_ne!(
            base,
            LawFailure {
                detail: String::new(),
                ..base.clone()
            }
        );
    }
}
