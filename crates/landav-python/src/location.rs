//! [`Location`] — where a finding is, precisely enough to act on.

use std::path::{Path, PathBuf};

/// A source position: file, line and column.
///
/// # Conventions, fixed here so that a rule cannot pick its own
///
/// * **`line` is 1-based**, counting `\n`-terminated physical lines. The first
///   line of a file is line 1.
/// * **`column` is a 1-based UTF-8 byte offset within its line.** Byte rather
///   than character because that is what a byte-oriented lexer already has,
///   and because a character offset is ambiguous between scalar values,
///   grapheme clusters and UTF-16 code units. A future LSP surface converts;
///   the analyser does not guess.
/// * **The position is the start of the offending expression, not the start of
///   the enclosing loop.** The loop is context; the edit happens at the
///   expression, and an editor that jumps to the loop header makes the reader
///   hunt for the actual call. `tests/fixture_corpus.rs` asserts this exactly,
///   so a rule that reports the loop line fails.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location {
    file: PathBuf,
    line: u32,
    column: u32,
}

impl Location {
    /// Builds a location. `line` and `column` are both 1-based.
    #[must_use]
    pub fn new(file: PathBuf, line: u32, column: u32) -> Self {
        Self { file, line, column }
    }

    /// The file the finding is in, exactly as it was handed to the analyser.
    #[must_use]
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The 1-based line.
    #[must_use]
    pub const fn line(&self) -> u32 {
        self.line
    }

    /// The 1-based UTF-8 byte column within [`Location::line`].
    #[must_use]
    pub const fn column(&self) -> u32 {
        self.column
    }
}
