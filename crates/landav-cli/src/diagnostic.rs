//! [`ToolError`] — a failure that carries blame.
//!
//! `CONTRIBUTING.md` non-negotiable 3 says failure must carry blame, never a
//! bare "unknown". In CI the only artefact an operator sees is one line of
//! stderr, so the type is shaped to make a blameless error unrepresentable:
//! there is no constructor that does not take both a *subject* — the path,
//! flag or configuration key at fault — and a *reason*.

use std::fmt;
use std::path::Path;

use thiserror::Error;

/// A reason the tool could not complete, naming what is to blame.
///
/// `subject` answers "where", `reason` answers "what". Both are mandatory.
#[derive(Debug, Clone, Error)]
#[error("{subject}: {reason}")]
pub struct ToolError {
    /// The thing at fault — a path, a flag, or a configuration key.
    subject: String,
    /// What went wrong with it, in the operator's terms.
    reason: String,
}

impl ToolError {
    /// Blame `subject` for `reason`.
    pub fn new(subject: impl fmt::Display, reason: impl fmt::Display) -> Self {
        Self {
            subject: subject.to_string(),
            reason: reason.to_string(),
        }
    }

    /// Blame a filesystem path.
    ///
    /// The path is rendered exactly as it was given rather than canonicalised,
    /// so that a diagnostic about `--config nowhere/landav.toml` names the
    /// string the caller typed and not an absolute path they never wrote.
    pub fn at_path(path: &Path, reason: impl fmt::Display) -> Self {
        Self::new(path.display(), reason)
    }
}
