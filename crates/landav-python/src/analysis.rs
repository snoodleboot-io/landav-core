//! The entry point every pattern rule is reached through.

use std::path::Path;

use rustpython_parser::{Parse, ast};

use crate::{
    context::analyse_program,
    finding::Finding,
    patterns::Analysis,
    python_error::PythonError,
    syntax::{LineIndex, MAX_EXPRESSION_DEPTH, MAX_NESTING_DEPTH, nesting_overflow},
};

/// Runs every rule in [`crate::registry()`] over one Python source file.
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
///
/// Source nested more deeply than the frontend will parse is reported the same
/// way. The parser is recursive-descent, so nesting depth is stack depth, and
/// this crate reads untrusted Python: a stack overflow aborts the process and
/// takes the blame path with it, whereas a `Parse` error naming the offset
/// leaves the caller something to act on.
pub fn analyze_source(path: &Path, source: &str) -> Result<Vec<Finding>, PythonError> {
    let index = LineIndex::new(source);

    if let Some(offset) = nesting_overflow(source) {
        let (line, column) = index.position(offset);
        return Err(PythonError::Parse {
            path: path.to_path_buf(),
            line,
            column,
            detail: format!(
                "nesting beyond {MAX_NESTING_DEPTH} blocks or {MAX_EXPRESSION_DEPTH} operators \
                 in one expression is not analysed"
            ),
        });
    }

    let module = ast::Suite::parse(source, &path.to_string_lossy()).map_err(|error| {
        let (line, column) = index.position(error.offset.to_usize());
        PythonError::Parse {
            path: path.to_path_buf(),
            line,
            column,
            detail: error.error.to_string(),
        }
    })?;

    let analysis = Analysis {
        path,
        source,
        index,
        program: analyse_program(&module, source),
    };

    Ok(analysis.run())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::analyze_source;

    #[test]
    fn a_syntax_error_names_the_position() {
        let error = analyze_source(Path::new("broken.py"), "def (:\n")
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.starts_with("broken.py:1:"), "{error}");
    }

    #[test]
    fn deep_nesting_is_an_error_rather_than_a_stack_overflow() {
        let source = format!("x = {}1{}\n", "(".repeat(5_000), ")".repeat(5_000));
        assert!(analyze_source(Path::new("deep.py"), &source).is_err());
    }

    /// Bracket depth is not the only way to build a deep tree. These three
    /// contain no brackets at all and each overflowed the stack before the
    /// operator-chain guard existed.
    #[test]
    fn deep_operator_chains_are_errors_rather_than_stack_overflows() {
        for source in [
            format!("x = {}1\n", "-".repeat(500_000)),
            format!("x = 1{}\n", " + 1".repeat(500_000)),
            format!("x = a{}\n", ".b".repeat(500_000)),
            format!("x = {}1\n", "not ".repeat(500_000)),
        ] {
            assert!(analyze_source(Path::new("chain.py"), &source).is_err());
        }
    }

    /// The guard measures one path, not the file: a flat literal of half a
    /// million elements is two nodes deep however long it is, and rejecting it
    /// would be a coverage gap dressed up as safety.
    #[test]
    fn a_long_flat_literal_is_still_analysed() {
        let source = format!("x = [{}]\n", "-1, ".repeat(50_000));
        assert!(analyze_source(Path::new("data.py"), &source).is_ok());
    }

    #[test]
    fn brackets_inside_a_string_do_not_trip_the_depth_guard() {
        let source = format!("x = \"{}\"\n", "[".repeat(1_000));
        assert!(analyze_source(Path::new("literal.py"), &source).is_ok());
    }

    #[test]
    fn an_empty_file_yields_no_findings() {
        let findings = analyze_source(Path::new("empty.py"), "").unwrap_or_default();
        assert!(findings.is_empty());
    }
}
