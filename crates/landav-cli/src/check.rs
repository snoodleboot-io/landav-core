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
//!
//! # Where the analysis happens, and where it does not
//!
//! Nowhere in this crate. `CONTRIBUTING.md` non-negotiable 4 puts every
//! language fact behind a frontend, and [`landav_python`] is the frontend: it
//! owns the parser, the ten `LAV0xx` rules, their false-positive budget and
//! their fixture corpus. This module walks paths, hands each file's text to
//! [`landav_python::analyze_module`], and turns what comes back into one
//! [`Outcome`]. There is no Python keyword, no quoting rule and no version
//! heuristic below this line, and there must never be one again: the crate
//! shipped a second, line-oriented Python scanner for one milestone and the
//! two rule sets immediately disagreed — the private one advised replacing a
//! `frozenset` with a set, and called an integer counter a rebuilt list.
//!
//! # A file the frontend could not parse is inconclusive, not clean
//!
//! [`landav_python::PythonError::Parse`] means the bytes were read and were
//! not Python the frontend can read: a Python 2 module, a template, a
//! generated `.py` that is really JSON, a 3.12 file using PEP 701 f-strings
//! the pinned parser predates. None of that supports "analysis ran and every
//! bound held", so it maps to [`Outcome::Inconclusive`] — exit `1`, with the
//! position named — and never to [`Outcome::Clean`].
//!
//! It maps to `Inconclusive` rather than to a tool error on purpose. The tool
//! completed; what it has to report is a fact about the file, and the person
//! who can act on it is the person who owns the file.

use std::io::Write as _;
use std::path::Path;

use landav_python::{ModuleAnalysis, PythonError};

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
    let mut report = Report::new(std::io::stdout().lock());

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
        match landav_python::analyze_module(path, &text) {
            Ok(module) => {
                statements += module.statements();
                findings += module.findings().len();
                publish(&mut report, &module);
            }
            Err(PythonError::Parse {
                line,
                column,
                detail,
                ..
            }) => {
                inconclusive += 1;
                report.line(format_args!(
                    "{}:{line}:{column}: inconclusive: unreadable-source: the frontend \
                     could not read this file as Python ({detail}), so no bound was \
                     derived from it and it is not covered by this run's verdict",
                    path.display()
                ));
            }
            // `PythonError` is `#[non_exhaustive]`, so this arm is required.
            // Everything that is not a parse failure is the frontend saying it
            // could not complete, which is a tool error and never a verdict.
            // Failing towards blame is the only safe direction for a variant
            // this build has not seen.
            Err(problem) => walk.problems.push(ToolError::at_path(path, problem)),
        }
    }

    summarise(
        &mut report,
        target,
        &config,
        &walk.sources,
        findings,
        inconclusive,
    );
    // A report that never reached the operator is not a report, so a stream
    // that could not be written is a reason the run did not complete — and
    // therefore a `2` with blame, not a silent verdict about findings nobody
    // saw. `landav check src/ | head -1` takes this path.
    if let Some(problem) = report.finish() {
        walk.problems.push(problem);
    }

    let outcome = if walk.problems.is_empty() {
        classify(kind, walk.sources.len(), statements, findings, inconclusive)
    } else {
        Outcome::Failed
    };

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

/// Write one line per finding, in the frontend's own order.
///
/// The rule code and the wording both come from [`landav_python`]; this crate
/// contributes the file position layout and nothing else.
fn publish<W: std::io::Write>(report: &mut Report<W>, module: &ModuleAnalysis) {
    for finding in module.findings() {
        let at = finding.location();
        report.line(format_args!(
            "{}:{}:{}: finding: {}: {}",
            at.file().display(),
            at.line(),
            at.column(),
            finding.rule(),
            finding.explanation()
        ));
    }
}

/// The run's report stream, and the first failure to write to it.
///
/// Every line is written as it is decided rather than buffered to the end, so
/// that blame survives a later file killing the run. What is *not* silent is
/// a write that fails: the first error is kept and surfaces as a [`ToolError`]
/// from [`Report::finish`], because an exit code describing findings the
/// operator never saw is a code that describes nothing they can act on.
struct Report<W: std::io::Write> {
    /// Where the report goes.
    out: W,
    /// The first write failure, if any. Later writes are skipped: a stream
    /// that has failed once will not start working again mid-run, and
    /// retrying only multiplies the diagnostics.
    failure: Option<std::io::Error>,
}

impl<W: std::io::Write> Report<W> {
    /// A report over `out`, with nothing written and nothing failed.
    const fn new(out: W) -> Self {
        Self { out, failure: None }
    }

    /// Write one record, terminated by a newline.
    fn line(&mut self, args: std::fmt::Arguments<'_>) {
        if self.failure.is_some() {
            return;
        }
        if let Err(error) = writeln!(self.out, "{args}") {
            self.failure = Some(error);
        }
    }

    /// Flush, and hand back blame if any part of the report was lost.
    fn finish(mut self) -> Option<ToolError> {
        if self.failure.is_none()
            && let Err(error) = self.out.flush()
        {
            self.failure = Some(error);
        }
        self.failure.map(|error| {
            ToolError::new(
                "standard output",
                format!(
                    "could not be written ({error}), so the report is incomplete; the \
                     exit code would describe findings that never reached the operator"
                ),
            )
        })
    }
}

/// Turn the counts into exactly one outcome.
///
/// Order matters, and it is the order of how much a claim would be worth. A
/// finding or an unaccounted term is something the run *established*, so it
/// outranks the emptiness check: a file that failed to parse holds no
/// statements this crate can count, and letting that read as "nothing to
/// analyse" would turn the strongest thing the run learned into the weakest.
/// Below those, nothing analysed is not a verdict at all, and clean is what is
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
    if findings > 0 {
        return Outcome::Findings;
    }
    if inconclusive > 0 {
        return Outcome::Inconclusive;
    }
    if files == 0 || statements == 0 {
        return match target {
            Target::Directory => Outcome::NothingAnalysed,
            Target::File => Outcome::Clean,
        };
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
fn summarise<W: std::io::Write>(
    report: &mut Report<W>,
    target: &Path,
    config: &Config,
    sources: &[std::path::PathBuf],
    findings: usize,
    inconclusive: usize,
) {
    report.line(format_args!(
        "landav: {} analysed under {} — {} finding(s), {} inconclusive; configuration: {}",
        plural(sources.len(), "file"),
        target.display(),
        findings,
        inconclusive,
        config.source()
    ));
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
    use super::{Report, classify, plural};
    use crate::outcome::Outcome;
    use crate::sources::Target;

    /// A writer that fails the way a closed pipe does.
    struct BrokenPipe;

    impl std::io::Write for BrokenPipe {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// `landav check src/ | head -1`. The findings after the first were never
    /// delivered, so the run did not do what it was asked and says so, rather
    /// than handing back a code that describes output nobody received.
    #[test]
    fn a_report_that_could_not_be_written_carries_blame() {
        let mut report = Report::new(BrokenPipe);
        report.line(format_args!("a finding nobody will see"));
        let blame = report.finish().map(|error| error.to_string());
        assert!(
            blame
                .as_deref()
                .is_some_and(|text| text.contains("standard output")),
            "{blame:?}"
        );
    }

    /// A report that was delivered in full is not a failure.
    #[test]
    fn a_report_that_was_written_reports_no_problem() {
        let mut report = Report::new(Vec::new());
        report.line(format_args!("delivered"));
        assert!(report.finish().is_none());
    }

    /// A file the frontend could not parse contributes no statements this
    /// crate can count. The emptiness rule must not outrank it, or the
    /// strongest thing the run learned would read as the weakest.
    #[test]
    fn an_unparsable_file_is_inconclusive_rather_than_nothing_analysed() {
        assert_eq!(classify(Target::File, 1, 0, 0, 1), Outcome::Inconclusive);
        assert_eq!(
            classify(Target::Directory, 1, 0, 0, 1),
            Outcome::Inconclusive
        );
        assert_ne!(classify(Target::File, 1, 0, 0, 1), Outcome::Clean);
    }

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
