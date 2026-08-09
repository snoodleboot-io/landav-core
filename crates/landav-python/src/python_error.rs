//! [`PythonError`] — the frontend's failure type.

use std::path::PathBuf;

use thiserror::Error;

/// Everything the Python frontend can fail at while producing findings.
///
/// Every variant names the file, and the parse variant names the position, so
/// a failure carries blame rather than being a bare "could not analyse" — see
/// non-negotiable 3 in `CONTRIBUTING.md`. There is no `Unknown` variant, by
/// design.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PythonError {
    /// The source is not syntactically valid Python 3 at the named position.
    #[error("{path}:{line}:{column}: not valid Python 3: {detail}")]
    Parse {
        /// The file that failed to parse.
        path: PathBuf,
        /// 1-based line of the first syntax error.
        line: u32,
        /// 1-based UTF-8 byte column of the first syntax error.
        column: u32,
        /// What the parser objected to.
        detail: String,
    },

    /// The source could not be read from disk.
    #[error("{path}: could not read source")]
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
}
