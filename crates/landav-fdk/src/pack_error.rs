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
    /// The pack declares a format this build does not read.
    ///
    /// The one refusal that is not a complaint about the pack. Every other
    /// variant says the author got something wrong; this one says the author
    /// is ahead and the *binary* is what needs changing, so it names both
    /// numbers rather than saying "invalid".
    ///
    /// Without it the same pack fails as [`Self::Malformed`] with serde's
    /// `unknown field` text, which sends a reader to edit a file that is
    /// correct.
    #[error(
        "signature pack declares format {found}, but this build of landav reads \
         up to format {supported}: use a newer landav to read this pack"
    )]
    UnsupportedFormat {
        /// The format the pack declares.
        found: u32,
        /// The newest format this build reads.
        supported: u32,
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
