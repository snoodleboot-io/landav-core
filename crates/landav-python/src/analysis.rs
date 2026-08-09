//! The entry point every pattern rule is reached through.

use std::path::Path;

use crate::{finding::Finding, python_error::PythonError};

/// Runs every rule in [`crate::registry`] over one Python source file.
///
/// `path` is not read; it is the label stamped into every
/// [`crate::Location`]. Callers that already have the source in memory — the
/// differential engine, the test corpus — must not be forced back through the
/// filesystem to get a finding with a filename on it.
///
/// # Ordering
///
/// The returned findings are sorted by `(line, column, rule code)`. This is a
/// contract, not an accident: two runs over identical bytes must produce
/// byte-identical output, or the CI baseline diffs against itself.
///
/// # Errors
///
/// [`PythonError::Parse`] if `source` is not valid Python 3. A file that does
/// not parse yields an error naming the position, never a silent empty result
/// — "no findings" and "could not look" are different answers and the caller
/// has to be able to tell them apart.
pub fn analyze_source(path: &Path, source: &str) -> Result<Vec<Finding>, PythonError> {
    // TODO(LAN-65): parse `source` and run the registry over it. Returning an
    // empty result keeps the workspace compiling while the fixture corpus and
    // the assertions in `tests/` stand as the specification; every positive
    // fixture fails against this stub, which is the intended red state.
    let _ = (path, source);
    Ok(Vec::new())
}
