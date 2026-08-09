//! `landav check PATH` — the zero-config entry point.
//!
//! # The shape of a run
//!
//! Configuration, then resolution, then analysis, then exactly one
//! [`Outcome`]. Each stage can only fail by naming what it could not do; there
//! is no stage that swallows a failure and lets the next one report a verdict
//! over a subset of the work.
//!
//! # Aggregation
//!
//! A run over a tree publishes the *worst* thing it saw, because the verdict
//! is a claim about the whole target:
//!
//! * anything the tool could not look at wins outright — a clean neighbour
//!   does not license a claim about a file whose bytes were never read;
//! * otherwise a finding or an unaccounted term wins over a clean unit;
//! * a run with nothing to analyse is [`Outcome::NothingAnalysed`], never
//!   clean.

use std::io::Write as _;
use std::path::Path;

use crate::analysis::{self, Kind};
use crate::config::{self, Config};
use crate::diagnostic::ToolError;
use crate::outcome::Outcome;
use crate::sources::Target;

/// Run `check` over `target`, reporting to stdout and stderr.
pub fn run(target: &Path, explicit_config: Option<&Path>) -> Outcome {
    match analyse(target, explicit_config) {
        Ok(outcome) => outcome,
        Err(error) => {
            report_failure(&error);
            Outcome::Failed
        }
    }
}

/// The run proper. Every failure is a [`ToolError`] carrying blame.
fn analyse(target: &Path, explicit_config: Option<&Path>) -> Result<Outcome, ToolError> {
    let config = config::load(target, explicit_config)?;
    let (kind, mut walk) = crate::sources::collect(target)?;

    let mut findings = 0usize;
    let mut inconclusive = 0usize;
    let mut statements = 0usize;
    let mut out = std::io::stdout().lock();

    for path in &walk.sources {
        // A file that could not be read is recorded and the walk continues, so
        // that one run names every path it could not look at rather than one
        // per invocation.
        let text = match read_source(path) {
            Ok(text) => text,
            Err(problem) => {
                walk.problems.push(problem);
                continue;
            }
        };
        let scan = analysis::scan(&text);
        statements += scan.statements;

        for observation in &scan.observations {
            match observation.kind {
                Kind::Finding => findings += 1,
                Kind::Inconclusive => inconclusive += 1,
            }
            // Written per observation rather than buffered to the end so that
            // the blame survives even if a later file kills the run.
            let _ = writeln!(
                out,
                "{}:{}: {}: {}: {}",
                path.display(),
                observation.line,
                observation.kind,
                observation.rule,
                observation.message
            );
        }
    }

    let outcome = if walk.problems.is_empty() {
        classify(kind, walk.sources.len(), statements, findings, inconclusive)
    } else {
        Outcome::Failed
    };
    summarise(
        &mut out,
        target,
        &config,
        &walk.sources,
        findings,
        inconclusive,
    );
    let _ = out.flush();

    // Sorted, so that identical input produces identical stderr whatever order
    // the filesystem handed the entries back in.
    walk.problems.sort_by_key(ToolError::to_string);
    for problem in &walk.problems {
        report_failure(problem);
    }
    if outcome == Outcome::NothingAnalysed {
        report_failure(&nothing_to_analyse(target, walk.sources.len()));
    }
    Ok(outcome)
}

/// Turn the counts into exactly one outcome.
///
/// Order matters, and it is the order of how much a claim would be worth:
/// nothing analysed is not a verdict at all, a finding outranks an unaccounted
/// term only in what gets *reported* (they share a code), and clean is what is
/// left when the run actually proved something.
///
/// # Why "nothing analysed" depends on how the target was named
///
/// The rule exists so that a CI path which stops matching after a directory
/// move cannot go green forever: a *directory* that turns out to hold no code
/// may not be the directory the author meant, and there is no way to tell from
/// inside the run. That argument does not reach a file the caller named by
/// hand. `landav check pkg/__init__.py` on an `__init__.py` holding a licence
/// header names a path that exists and was read; there is no glob that could
/// have gone stale, and answering "the tool could not look" is false. A
/// pre-commit hook feeding changed files one at a time hits that case on every
/// commit, and a gate that cries wolf on `__init__.py` is a gate that gets
/// removed.
///
/// So the rule is scoped to directory targets, which is exactly where its
/// justification applies, and leaves the acceptance suite's empty-directory
/// assertion intact.
const fn classify(
    target: Target,
    files: usize,
    statements: usize,
    findings: usize,
    inconclusive: usize,
) -> Outcome {
    if files == 0 || statements == 0 {
        return match target {
            Target::Directory => Outcome::NothingAnalysed,
            Target::File => Outcome::Clean,
        };
    }
    if findings > 0 {
        return Outcome::Findings;
    }
    if inconclusive > 0 {
        return Outcome::Inconclusive;
    }
    Outcome::Clean
}

/// The diagnostic for a target that yielded nothing.
fn nothing_to_analyse(target: &Path, files: usize) -> ToolError {
    let reason = if files == 0 {
        "contains no Python source to analyse, so the run checked no code at all; \
         a path that has stopped matching reports this rather than reporting clean"
            .to_owned()
    } else {
        format!(
            "contains {files} Python file(s), none of which hold a single statement, \
             so the run checked no code at all; analysing nothing is not the same as \
             finding nothing"
        )
    };
    ToolError::at_path(target, reason)
}

/// The one-line summary every run ends with.
fn summarise(
    out: &mut impl std::io::Write,
    target: &Path,
    config: &Config,
    sources: &[std::path::PathBuf],
    findings: usize,
    inconclusive: usize,
) {
    let _ = writeln!(
        out,
        "landav: {} analysed under {} — {} finding(s), {} inconclusive; configuration: {}",
        plural(sources.len(), "file"),
        target.display(),
        findings,
        inconclusive,
        config.source()
    );
}

/// `1 file` / `2 files`, so the summary reads as English.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Read a source file, blaming it for anything that stops the bytes arriving.
///
/// A file whose bytes were never seen, or that could not be decoded, supports
/// no statement at all — including "clean".
fn read_source(path: &Path) -> Result<String, ToolError> {
    let bytes = std::fs::read(path)
        .map_err(|err| ToolError::at_path(path, format!("cannot be read: {err}")))?;
    String::from_utf8(bytes).map_err(|err| {
        ToolError::at_path(
            path,
            format!(
                "is not valid UTF-8 (at byte {}), so the parser never saw the text \
                 and nothing can be concluded about it",
                err.utf8_error().valid_up_to()
            ),
        )
    })
}

/// Write a tool error to stderr.
///
/// Always stderr, always prefixed, always naming a subject. CI shows this line
/// and nothing else.
fn report_failure(error: &ToolError) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "landav: error: {error}");
    let _ = err.flush();
}

#[cfg(test)]
mod tests {
    use super::{classify, plural};
    use crate::outcome::Outcome;
    use crate::sources::Target;

    #[test]
    fn a_directory_with_no_files_is_never_clean() {
        assert_eq!(
            classify(Target::Directory, 0, 0, 0, 0),
            Outcome::NothingAnalysed
        );
    }

    #[test]
    fn a_directory_of_files_with_no_statements_is_never_clean() {
        assert_eq!(
            classify(Target::Directory, 3, 0, 0, 0),
            Outcome::NothingAnalysed
        );
    }

    /// R3. A file the caller named by hand cannot be a path that stopped
    /// matching, so an empty `__init__.py` is not "the tool could not look".
    #[test]
    fn a_named_file_with_no_statements_is_not_a_tool_error() {
        assert_eq!(classify(Target::File, 1, 0, 0, 0), Outcome::Clean);
    }

    #[test]
    fn an_inconclusive_unit_is_not_absorbed_by_clean_neighbours() {
        assert_eq!(
            classify(Target::Directory, 2, 40, 0, 1),
            Outcome::Inconclusive
        );
        assert_ne!(classify(Target::Directory, 2, 40, 0, 1), Outcome::Clean);
    }

    #[test]
    fn a_proven_run_is_clean() {
        assert_eq!(classify(Target::File, 1, 6, 0, 0), Outcome::Clean);
    }

    #[test]
    fn findings_outrank_inconclusive_in_what_is_reported() {
        assert_eq!(classify(Target::Directory, 2, 40, 1, 1), Outcome::Findings);
    }

    #[test]
    fn the_summary_counts_read_as_english() {
        assert_eq!(plural(1, "file"), "1 file");
        assert_eq!(plural(0, "file"), "0 files");
        assert_eq!(plural(2, "file"), "2 files");
    }
}
