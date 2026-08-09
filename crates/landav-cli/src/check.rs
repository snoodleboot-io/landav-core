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
    let sources = crate::sources::collect(target)?;

    let mut findings = 0usize;
    let mut inconclusive = 0usize;
    let mut statements = 0usize;
    let mut out = std::io::stdout().lock();

    for path in &sources {
        let text = read_source(path)?;
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

    let outcome = classify(sources.len(), statements, findings, inconclusive);
    summarise(&mut out, target, &config, &sources, findings, inconclusive);
    let _ = out.flush();

    if outcome == Outcome::NothingAnalysed {
        report_failure(&nothing_to_analyse(target, sources.len()));
    }
    Ok(outcome)
}

/// Turn the counts into exactly one outcome.
///
/// Order matters, and it is the order of how much a claim would be worth:
/// nothing analysed is not a verdict at all, a finding outranks an unaccounted
/// term only in what gets *reported* (they share a code), and clean is what is
/// left when the run actually proved something.
const fn classify(
    files: usize,
    statements: usize,
    findings: usize,
    inconclusive: usize,
) -> Outcome {
    if files == 0 || statements == 0 {
        return Outcome::NothingAnalysed;
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
    } else {
        "contains only empty Python files, so the run checked no code at all; \
         analysing nothing is not the same as finding nothing"
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

    #[test]
    fn a_target_with_no_files_is_never_clean() {
        assert_eq!(classify(0, 0, 0, 0), Outcome::NothingAnalysed);
    }

    #[test]
    fn files_with_no_statements_are_never_clean() {
        assert_eq!(classify(3, 0, 0, 0), Outcome::NothingAnalysed);
    }

    #[test]
    fn an_inconclusive_unit_is_not_absorbed_by_clean_neighbours() {
        assert_eq!(classify(2, 40, 0, 1), Outcome::Inconclusive);
        assert_ne!(classify(2, 40, 0, 1), Outcome::Clean);
    }

    #[test]
    fn a_proven_run_is_clean() {
        assert_eq!(classify(1, 6, 0, 0), Outcome::Clean);
    }

    #[test]
    fn findings_outrank_inconclusive_in_what_is_reported() {
        assert_eq!(classify(2, 40, 1, 1), Outcome::Findings);
    }

    #[test]
    fn the_summary_counts_read_as_english() {
        assert_eq!(plural(1, "file"), "1 file");
        assert_eq!(plural(0, "file"), "0 files");
        assert_eq!(plural(2, "file"), "2 files");
    }
}
