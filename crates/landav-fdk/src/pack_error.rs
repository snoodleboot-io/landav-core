//! [`PackError`] - why a signature pack could not be read.

use thiserror::Error;

/// A signature pack that could not be read, and what was wrong with it.
///
/// Typed rather than a string, and every variant names its subject: a pack is
/// data a user may have written, so "invalid pack" is not an answer anybody can
/// act on.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackError {
    /// The text is not TOML, or the TOML is not a pack.
    #[error("signature pack is not readable as TOML: {reason}")]
    Malformed {
        /// What the TOML reader said.
        reason: String,
    },
    /// Two rows claim the same callee.
    ///
    /// Refused rather than last-wins. Two rows for one name means somebody
    /// appended instead of editing, and the two `why` fields are exactly the
    /// disagreement a silent last-wins would hide.
    #[error("signature pack declares `{callee}` twice")]
    Duplicate {
        /// The callee named twice.
        callee: String,
    },
    /// A row's `why` is empty.
    ///
    /// The one field validated beyond its type. `why = ""` satisfies the
    /// deserialiser and defeats the point of the field, so it is refused here.
    #[error("signature pack row for `{callee}` has an empty `why`")]
    UnexplainedRow {
        /// The callee whose row explains nothing.
        callee: String,
    },
}
