//! Size envelope providers and their registry: the `LAN-13` pattern,
//! re-instantiated for sizes.
//!
//! # The requirement this exists to meet
//!
//! `F-016` says the provider must be a trait with runtime registration from
//! the start, or the OSS/EE split has failed: the OSS sources (declaration,
//! config, schema) and the EE source (production telemetry percentiles) must
//! reach the analysis through one interface, and the EE one must be a
//! *registration*, not a bolt-on. [`crate::SignaturePack`] solved the same
//! problem once - a typed interface, packs loaded as data, overrides allowed
//! and recorded, provenance on every row - and this mirrors it beat for beat.
//!
//! # Last registered wins, and every override is on the record
//!
//! [`SizeEnvelopes::register`] appends; [`SizeEnvelopes::lookup`] asks the
//! most recently registered provider first. Overriding is the point of
//! registering a second provider at all - telemetry correcting a declaration -
//! so it is allowed, and it is never silent: every key a new provider answers
//! that an earlier one already answered is an [`Overridden`] record, with both
//! origins, exactly as [`crate::Shadowed`] records a replaced signature row.
//!
//! # No default, ever
//!
//! A key no provider answers is `None`. There is no fallback envelope and no
//! way to register one: a concrete estimate for a size nobody bounded is
//! *awaiting* that bound, and reporting a guessed number instead would invert
//! the product. The `resource::awaiting` precedent in the driver is the shape
//! the caller takes.

use std::{collections::BTreeMap, fmt, path::Path};

use serde::Deserialize;
use thiserror::Error;

use crate::size_envelope::{EnvelopeOrigin, SizeEnvelope, SizeKey};

/// A source of size envelopes.
///
/// Object-safe on purpose: the registry holds providers by `Box<dyn>`, and
/// the EE telemetry provider is compiled in another crate.
pub trait SizeEnvelopeProvider {
    /// A short name for provenance (`declaration`, `telemetry`).
    fn name(&self) -> &str;

    /// The envelope this source has for `key`, if any. `None` means this
    /// source says nothing about the size - never that the size is zero.
    fn envelope(&self, key: &SizeKey) -> Option<SizeEnvelope>;

    /// Every key this source answers for. The registry reads it once at
    /// registration to record what the new source overrides.
    fn keys(&self) -> Vec<SizeKey>;
}

/// A key a later provider answers that an earlier one already answered.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Overridden {
    /// The size.
    pub key: SizeKey,
    /// Who answered before, and what they said.
    pub overridden: EnvelopeOrigin,
    /// Who answers now.
    pub overridden_by: EnvelopeOrigin,
    /// The envelope that no longer wins.
    pub replaced: SizeEnvelope,
}

/// What a lookup found: the envelope and which provider answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer<'a> {
    /// The envelope.
    pub envelope: SizeEnvelope,
    /// [`SizeEnvelopeProvider::name`] of the provider that answered.
    pub provider: &'a str,
}

/// The registered providers, in registration order.
#[derive(Default)]
pub struct SizeEnvelopes {
    providers: Vec<Box<dyn SizeEnvelopeProvider>>,
    overridden: Vec<Overridden>,
}

impl fmt::Debug for SizeEnvelopes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SizeEnvelopes")
            .field(
                "providers",
                &self
                    .providers
                    .iter()
                    .map(|p| p.name().to_owned())
                    .collect::<Vec<_>>(),
            )
            .field("overridden", &self.overridden)
            .finish()
    }
}

impl SizeEnvelopes {
    /// No providers: every lookup is `None`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `provider` after every provider so far, recording each key it
    /// takes over from an earlier one.
    pub fn register(&mut self, provider: Box<dyn SizeEnvelopeProvider>) {
        for key in provider.keys() {
            if let (Some(previous), Some(now)) = (self.lookup(&key), provider.envelope(&key)) {
                self.overridden.push(Overridden {
                    key,
                    overridden: previous.envelope.origin().clone(),
                    overridden_by: now.origin().clone(),
                    replaced: previous.envelope,
                });
            }
        }
        self.providers.push(provider);
    }

    /// The envelope for `key` from the most recently registered provider that
    /// has one, or `None` - and `None` is *awaiting*, not zero.
    #[must_use]
    pub fn lookup(&self, key: &SizeKey) -> Option<Answer<'_>> {
        self.providers.iter().rev().find_map(|provider| {
            provider.envelope(key).map(|envelope| Answer {
                envelope,
                provider: provider.name(),
            })
        })
    }

    /// Every override registration produced, in order.
    #[must_use]
    pub fn overridden(&self) -> &[Overridden] {
        &self.overridden
    }

    /// The registered providers' names, in registration order.
    #[must_use]
    pub fn providers(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.name()).collect()
    }
}

/// Why a declaration file could not be read.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum EnvelopeError {
    /// Not TOML, or not this shape.
    #[error("{file}: size declarations are not readable: {reason}")]
    Malformed {
        /// The file.
        file: String,
        /// serde's words.
        reason: String,
    },
    /// Two rows for one size.
    #[error("{file}: declares `{key}` twice")]
    Duplicate {
        /// The file.
        file: String,
        /// The size.
        key: String,
    },
    /// A row with no reason.
    #[error("{file}: the row for `{key}` has an empty `because`")]
    Unexplained {
        /// The file.
        file: String,
        /// The size.
        key: String,
    },
}

/// The on-disk shape: `[[size]]` tables and nothing else.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclarationFile {
    #[serde(default)]
    size: Vec<DeclaredSize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclaredSize {
    module: String,
    function: String,
    parameter: String,
    at_most: u64,
    #[serde(default)]
    exact: bool,
    because: String,
}

/// The OSS manual source: sizes the user declared in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationProvider {
    envelopes: BTreeMap<SizeKey, SizeEnvelope>,
}

impl DeclarationProvider {
    /// Reads declarations from TOML, attributing every row to `file`.
    ///
    /// # Errors
    ///
    /// [`EnvelopeError::Malformed`] if the text is not a declaration file,
    /// [`EnvelopeError::Duplicate`] if two rows name one size, and
    /// [`EnvelopeError::Unexplained`] if a row's `because` is empty - a size
    /// bound nobody can say the reason for is not a bound anyone can review.
    pub fn parse(file: &Path, text: &str) -> Result<Self, EnvelopeError> {
        let name = file.display().to_string();
        let parsed: DeclarationFile =
            toml::from_str(text).map_err(|error| EnvelopeError::Malformed {
                file: name.clone(),
                reason: error.to_string(),
            })?;
        let origin = EnvelopeOrigin::Declaration(file.to_path_buf());
        let mut envelopes = BTreeMap::new();
        for row in parsed.size {
            let key = SizeKey::parameter(row.module, row.function, row.parameter);
            if row.because.trim().is_empty() {
                return Err(EnvelopeError::Unexplained {
                    file: name,
                    key: key.to_string(),
                });
            }
            if envelopes.contains_key(&key) {
                return Err(EnvelopeError::Duplicate {
                    file: name,
                    key: key.to_string(),
                });
            }
            let envelope = if row.exact {
                SizeEnvelope::exactly(row.at_most, origin.clone(), row.because)
            } else {
                SizeEnvelope::at_most(row.at_most, origin.clone(), row.because)
            };
            envelopes.insert(key, envelope);
        }
        Ok(Self { envelopes })
    }

    /// How many sizes the file declares.
    #[must_use]
    pub fn len(&self) -> usize {
        self.envelopes.len()
    }

    /// Whether the file declares nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.envelopes.is_empty()
    }
}

impl SizeEnvelopeProvider for DeclarationProvider {
    fn name(&self) -> &str {
        "declaration"
    }

    fn envelope(&self, key: &SizeKey) -> Option<SizeEnvelope> {
        self.envelopes.get(key).cloned()
    }

    fn keys(&self) -> Vec<SizeKey> {
        self.envelopes.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::path::PathBuf;

    use super::*;

    const FILE: &str = "\
[[size]]
module = \"app.orders\"
function = \"process\"
parameter = \"rows\"
at_most = 10000
because = \"pagination caps a page at 10k rows\"

[[size]]
module = \"app.orders\"
function = \"Batch.run\"
parameter = \"n\"
at_most = 64
exact = true
because = \"the batch size is a fixed constant\"
";

    fn declared() -> DeclarationProvider {
        DeclarationProvider::parse(Path::new("sizes.toml"), FILE).expect("a valid file")
    }

    #[test]
    fn a_declaration_file_parses_to_keyed_envelopes() {
        let provider = declared();
        assert_eq!(provider.len(), 2);
        let rows = provider
            .envelope(&SizeKey::parameter("app.orders", "process", "rows"))
            .unwrap();
        assert_eq!(rows.ceiling(), 10_000);
        assert!(!rows.is_exact(), "at_most unless declared exact");
        assert_eq!(
            rows.origin(),
            &EnvelopeOrigin::Declaration(PathBuf::from("sizes.toml"))
        );
        let n = provider
            .envelope(&SizeKey::parameter("app.orders", "Batch.run", "n"))
            .unwrap();
        assert!(n.is_exact());
    }

    #[test]
    fn a_malformed_row_names_the_file_and_what_is_wrong() {
        let not_toml = DeclarationProvider::parse(Path::new("x.toml"), "[[size]\n");
        assert!(
            matches!(not_toml, Err(EnvelopeError::Malformed { ref file, .. }) if file == "x.toml")
        );
        let unknown_field = DeclarationProvider::parse(
            Path::new("x.toml"),
            "[[size]]\nmodule = \"m\"\nfunction = \"f\"\nparameter = \"p\"\nat_most = 1\nbecause = \"b\"\nmaybe = 2\n",
        );
        assert!(matches!(
            unknown_field,
            Err(EnvelopeError::Malformed { .. })
        ));
        let missing = DeclarationProvider::parse(
            Path::new("x.toml"),
            "[[size]]\nmodule = \"m\"\nfunction = \"f\"\nparameter = \"p\"\nbecause = \"b\"\n",
        );
        assert!(
            matches!(missing, Err(EnvelopeError::Malformed { ref reason, .. }) if reason.contains("at_most")),
            "{missing:?}"
        );
        let unexplained = DeclarationProvider::parse(
            Path::new("x.toml"),
            "[[size]]\nmodule = \"m\"\nfunction = \"f\"\nparameter = \"p\"\nat_most = 1\nbecause = \"  \"\n",
        );
        assert_eq!(
            unexplained,
            Err(EnvelopeError::Unexplained {
                file: "x.toml".to_owned(),
                key: "p of m.f".to_owned()
            })
        );
        let twice = DeclarationProvider::parse(Path::new("x.toml"), &format!("{FILE}\n{FILE}"));
        assert!(matches!(twice, Err(EnvelopeError::Duplicate { .. })));
    }

    /// A provider defined here, boxed and registered: the trait is object-safe
    /// and nothing about the registry knows the OSS providers by name.
    struct Fixed(SizeKey, u64);

    impl SizeEnvelopeProvider for Fixed {
        fn name(&self) -> &str {
            "fixed"
        }
        fn envelope(&self, key: &SizeKey) -> Option<SizeEnvelope> {
            (key == &self.0).then(|| {
                SizeEnvelope::at_most(
                    self.1,
                    EnvelopeOrigin::Declaration(PathBuf::from("later.toml")),
                    "measured",
                )
            })
        }
        fn keys(&self) -> Vec<SizeKey> {
            vec![self.0.clone()]
        }
    }

    #[test]
    fn the_last_registered_provider_wins_and_the_override_is_on_the_record() {
        let key = SizeKey::parameter("app.orders", "process", "rows");
        let mut registry = SizeEnvelopes::new();
        registry.register(Box::new(declared()));
        assert_eq!(registry.lookup(&key).unwrap().envelope.ceiling(), 10_000);
        assert!(registry.overridden().is_empty());

        registry.register(Box::new(Fixed(key.clone(), 500)));
        let answer = registry.lookup(&key).unwrap();
        assert_eq!(answer.envelope.ceiling(), 500);
        assert_eq!(answer.provider, "fixed");
        assert_eq!(registry.providers(), ["declaration", "fixed"]);

        let [record] = registry.overridden() else {
            panic!("one override: {:?}", registry.overridden());
        };
        assert_eq!(record.key, key);
        assert_eq!(record.replaced.ceiling(), 10_000);
        assert_eq!(
            record.overridden,
            EnvelopeOrigin::Declaration(PathBuf::from("sizes.toml"))
        );
        assert_eq!(
            record.overridden_by,
            EnvelopeOrigin::Declaration(PathBuf::from("later.toml"))
        );
        // The other key is untouched by the override.
        assert_eq!(
            registry
                .lookup(&SizeKey::parameter("app.orders", "Batch.run", "n"))
                .unwrap()
                .provider,
            "declaration"
        );
    }

    /// There is no default. A key nobody answers is `None`, from an empty
    /// registry and from a full one alike.
    #[test]
    fn a_size_nobody_bounded_is_awaiting_not_a_number() {
        let unknown = SizeKey::parameter("nowhere", "nothing", "x");
        assert!(SizeEnvelopes::new().lookup(&unknown).is_none());
        let mut registry = SizeEnvelopes::new();
        registry.register(Box::new(declared()));
        assert!(registry.lookup(&unknown).is_none());
    }
}
