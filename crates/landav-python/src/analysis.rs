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

/// One module, analysed: what the rules found and how much code they ran over.
///
/// The second half is not decoration. A caller that turns findings into a
/// verdict has to be able to tell "analysed, nothing to report" from "there
/// was nothing to analyse" — a `.py` holding only a licence header supports
/// neither a clean bill of health nor a finding — and counting Python
/// statements is a question only the frontend can answer. Publishing it here
/// is what lets a driver stay ignorant of Python.
#[derive(Debug, Clone)]
pub struct ModuleAnalysis {
    findings: Vec<Finding>,
    statements: usize,
}

impl ModuleAnalysis {
    /// Every rule that fired, in `(line, column, rule code)` order.
    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// The findings, by value.
    #[must_use]
    pub fn into_findings(self) -> Vec<Finding> {
        self.findings
    }

    /// How many Python statements the module contains, at any nesting depth.
    ///
    /// Zero means the file parsed and held no code at all: empty, or nothing
    /// but comments and blank lines. It is a count of *statements*, not of
    /// lines, so a docstring-only module counts one and a hundred-line
    /// comment block counts none.
    #[must_use]
    pub const fn statements(&self) -> usize {
        self.statements
    }
}

/// Runs every rule in [`crate::registry()`] over one Python source file.
///
/// Equivalent to [`analyze_module`] followed by
/// [`ModuleAnalysis::into_findings`]; this is the spelling for callers that
/// only want the findings.
///
/// # Errors
///
/// As [`analyze_module`].
pub fn analyze_source(path: &Path, source: &str) -> Result<Vec<Finding>, PythonError> {
    analyze_module(path, source).map(ModuleAnalysis::into_findings)
}

/// Runs every rule in [`crate::registry()`] over one Python source file, and
/// reports how much code it ran over.
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
pub fn analyze_module(path: &Path, source: &str) -> Result<ModuleAnalysis, PythonError> {
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

    let statements = analysis.program.statements.len();
    Ok(ModuleAnalysis {
        findings: analysis.run(),
        statements,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{analyze_module, analyze_source};

    /// A driver decides "nothing to analyse" from this number, so a file that
    /// parsed and held no code has to be distinguishable from one that held
    /// some. Comments and blank lines are not statements.
    #[test]
    fn statements_separate_an_empty_module_from_a_quiet_one() {
        let count = |source: &str| {
            analyze_module(Path::new("counted.py"), source)
                .ok()
                .map(|module| module.statements())
        };
        assert_eq!(count(""), Some(0));
        assert_eq!(count("\n\n# just a comment\n\n"), Some(0));
        assert_eq!(count("x = 1\n"), Some(1));
        // Nested statements count: the body is code the rules ran over.
        assert!(count("def f():\n    return 1\n").is_some_and(|n| n >= 2));
    }

    /// A UTF-8 byte-order mark is legal at the start of a Python file, and
    /// every Windows-authored source carries one. Rejecting it would make a
    /// whole platform's correct code unanalysable.
    #[test]
    fn a_byte_order_mark_does_not_stop_the_file_being_read() {
        let source = "\u{feff}def total(items):\n    return sum(items)\n";
        let module = analyze_module(Path::new("bom.py"), source);
        assert!(
            module
                .as_ref()
                .is_ok_and(|module| module.statements() > 0 && module.findings().is_empty()),
            "{:?}",
            module.err().map(|error| error.to_string())
        );
    }

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
