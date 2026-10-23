//! A size envelope: an upper bound on a size a bound names, and where it came
//! from.
//!
//! # Scope
//!
//! Story 1 of `F-016` (`LAN-17`, `LAN-125`). `O(n²)` is unactionable without
//! `n`; in a real system `n` is bounded and knowable - a pagination cap, a
//! schema constraint, a production percentile - and an envelope is that
//! knowledge, declared.
//!
//! # Where it plugs in - not into the bound
//!
//! A derived bound is a symbolic polynomial over size variables and stays so.
//! An envelope is a partial *valuation* of those variables, applied at
//! concrete-estimation time (`F-017`, `LAN-18`) to turn `Θ(len(rows))` into
//! "≤ 10,000 steps at `len(rows) ≤ 10,000`", as a separate, labelled
//! projection. The symbolic answer is the truth; the concrete one names every
//! input it used, and this module is the type of those inputs.
//!
//! # Three rules
//!
//! * An envelope is an **upper bound**, never an equality, unless its source
//!   declares exactness. A percentile is `≤` at a confidence; a schema maximum
//!   is `≤`. A concrete number resting on one is therefore itself a bound and
//!   is reported as `≤`.
//! * An envelope is an **assumption**, so every concrete estimate resting on
//!   one carries a premise naming the source - [`SizePremise`], the same
//!   contract as a supplied signature's premise (`LAN-13`).
//! * A size with **no envelope is awaiting**, never a default. There is no
//!   constructor for "assume something"; see [`crate::SizeEnvelopes::lookup`].

use std::{fmt, path::PathBuf};

/// The stable identity of a size a bound names.
///
/// A bound mentions `len(rows)` or `n`; a provider has to answer for *that*
/// size and no other, across runs and across sources. The first key is the
/// one the declaration and config sources reach directly: a parameter of a
/// function in a module. A schema-keyed size (a table's row count) joins when
/// the frontend can tie a query's result to a parameter - story 3.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SizeKey {
    /// A parameter of a function: its length if it is a collection, its value
    /// if it is an integer.
    Parameter {
        /// The module, as the frontend names it (`app.orders`).
        module: String,
        /// The function, qualified for a method (`Processor.run`).
        function: String,
        /// The parameter's name.
        name: String,
    },
}

impl SizeKey {
    /// The parameter `name` of `function` in `module`.
    #[must_use]
    pub fn parameter(
        module: impl Into<String>,
        function: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self::Parameter {
            module: module.into(),
            function: function.into(),
            name: name.into(),
        }
    }
}

impl fmt::Display for SizeKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parameter {
                module,
                function,
                name,
            } => write!(formatter, "{name} of {module}.{function}"),
        }
    }
}

/// Where an envelope came from.
///
/// Kept beside the envelope rather than declared by it, for the reason
/// [`crate::PackOrigin`] gives: a source that could name its own origin could
/// name someone else's. The variants are the four sources `F-016` names; only
/// the declaration source exists in story 1, and the EE telemetry source plugs
/// in through the same enum and the same trait.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum EnvelopeOrigin {
    /// An explicit declaration in a file the user wrote.
    Declaration(PathBuf),
}

impl fmt::Display for EnvelopeOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Declaration(path) => write!(formatter, "declared in {}", path.display()),
        }
    }
}

/// An upper bound on one size, with its provenance and its reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeEnvelope {
    at_most: u64,
    exact: bool,
    origin: EnvelopeOrigin,
    because: String,
}

impl SizeEnvelope {
    /// A size that is at most `at_most`.
    #[must_use]
    pub fn at_most(at_most: u64, origin: EnvelopeOrigin, because: impl Into<String>) -> Self {
        Self {
            at_most,
            exact: false,
            origin,
            because: because.into(),
        }
    }

    /// A size that is exactly `value` - only when the source says so.
    #[must_use]
    pub fn exactly(value: u64, origin: EnvelopeOrigin, because: impl Into<String>) -> Self {
        Self {
            at_most: value,
            exact: true,
            origin,
            because: because.into(),
        }
    }

    /// The ceiling.
    #[must_use]
    pub const fn ceiling(&self) -> u64 {
        self.at_most
    }

    /// Whether the ceiling is also the floor. `false` unless the source
    /// declared it, and a consumer reports `≤` whenever this is `false`.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        self.exact
    }

    /// Where this came from.
    #[must_use]
    pub const fn origin(&self) -> &EnvelopeOrigin {
        &self.origin
    }

    /// Why the source believes it, in one sentence.
    #[must_use]
    pub fn because(&self) -> &str {
        &self.because
    }
}

/// The trust name a concrete estimate publishes for an envelope.
pub const SIZE_ENVELOPE_TRUST: &str = "size-envelope";

/// What a concrete estimate resting on an envelope is believed on.
///
/// Defined here, once, so that the consumer (`LAN-18`) cannot invent a second
/// shape: the subject is the size, the trust is [`SIZE_ENVELOPE_TRUST`], and
/// the reason is the envelope's own, with its origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizePremise {
    /// The size, as [`SizeKey`] displays it.
    pub subject: String,
    /// [`SIZE_ENVELOPE_TRUST`].
    pub trust: &'static str,
    /// The envelope's reason and origin, in one sentence.
    pub because: String,
}

impl SizePremise {
    /// The premise for `envelope` answering for `key`.
    #[must_use]
    pub fn of(key: &SizeKey, envelope: &SizeEnvelope) -> Self {
        let relation = if envelope.is_exact() {
            "exactly"
        } else {
            "at most"
        };
        Self {
            subject: key.to_string(),
            trust: SIZE_ENVELOPE_TRUST,
            because: format!(
                "{key} is taken as {relation} {}, {}: {}",
                envelope.ceiling(),
                envelope.origin(),
                envelope.because()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_envelope_is_an_upper_bound_unless_declared_exact() {
        let origin = EnvelopeOrigin::Declaration(PathBuf::from("sizes.toml"));
        let upper = SizeEnvelope::at_most(10, origin.clone(), "a page");
        assert!(!upper.is_exact());
        let exact = SizeEnvelope::exactly(10, origin, "a fixed-width record");
        assert!(exact.is_exact());
        assert_eq!(upper.ceiling(), exact.ceiling());
    }

    #[test]
    fn the_premise_names_the_size_the_relation_and_the_source() {
        let key = SizeKey::parameter("app.orders", "process", "rows");
        let envelope = SizeEnvelope::at_most(
            10_000,
            EnvelopeOrigin::Declaration(PathBuf::from("sizes.toml")),
            "pagination caps a page at 10k rows",
        );
        let premise = SizePremise::of(&key, &envelope);
        assert_eq!(premise.trust, "size-envelope");
        assert_eq!(premise.subject, "rows of app.orders.process");
        assert!(premise.because.contains("at most 10000"));
        assert!(premise.because.contains("declared in sizes.toml"));
        assert!(premise.because.contains("pagination caps"));
    }
}
