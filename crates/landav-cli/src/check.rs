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
//!
//! # A partly-lowered file must not read as a fully-analysed one
//!
//! `LAN-68`. A Python construct the lowering will not take produces **no
//! transition**, and an integer transition system missing a transition admits
//! fewer executions than the program has — so a bound derived from it can be
//! exceeded. The analysis is unsound *by omission*, and on a terminal it looks
//! exactly like a clean result. [`landav_its::lower`] closes that hole for one
//! function by refusing outright rather than emitting a partial system;
//! [`landav_its::Coverage`] closes it for a run.
//!
//! Two decisions, made here:
//!
//! * **The ratio is on every run's summary line**, complete or partial, with or
//!   without `--coverage`. Without it the summary says "3 files analysed" and
//!   stops, which invites exactly the wrong reading. This is the argument
//!   `LAN-66` makes for the suppression counts, applied to a number that has
//!   more riding on it.
//! * **`--coverage` escalates a partial run to [`Outcome::Inconclusive`]**, and
//!   the default run's exit code is unchanged. At M0 no bound is derived from
//!   the lowering, so the default verdict — the `LAV0xx` rules — does not rest
//!   on it, and failing every real Python file on the reach of an M0 fragment
//!   produces a gate that gets switched off. Asking about the lowering and
//!   getting a partial answer is a different matter, and it lands where
//!   `--resource` already lands. See [`classify`] for where a refusal sits in
//!   the ordering and why it is never [`Outcome::Clean`].
//!
//! # A waived finding is not a finding, and the waiver is
//!
//! `LAN-66` lets an author waive a rule they have judged acceptable, inline or
//! per path, and a waived finding does **not** raise the exit code. That is
//! the point of the feature: a gate that fails anyway would be answered by
//! deleting the rule, or by deleting the gate.
//!
//! What the exit code stops carrying, the report starts carrying. Every
//! waiver is printed — the ones that fired, the ones that fired for nothing,
//! and the ones naming a code no build has ever issued — and the run summary
//! counts them. None of them changes the exit code either: a stale waiver is
//! not a defect in the code under analysis, the run that would fail on it
//! depends on which subset of the tree was named, and failing on one punishes
//! whoever fixed the underlying problem and left the comment behind.
//!
//! Escalating any of that to a build failure is a *policy* decision — whose
//! waiver, approved by whom, expiring when — and `docs/EDITIONS.md` puts
//! policy governance (`E-001`) on the paid side. This crate records; it does
//! not govern, and it contains no entitlement logic of any kind.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::Path;

use landav_bound::ResourceKind;
use landav_its::Coverage;
use landav_python::{ModuleAnalysis, PythonError, Suppression, SuppressionStatus};

use crate::config::{self, Config};
use crate::diagnostic::ToolError;
use crate::machine;
use crate::outcome::Outcome;
use crate::sources::Target;

/// Run `check` over `target`, reporting to stdout and stderr.
pub fn run(
    target: Option<&Path>,
    stdin_name: Option<&str>,
    explicit_config: Option<&Path>,
    resource: Option<ResourceKind>,
    coverage: bool,
    bounds: bool,
    json: bool,
) -> Outcome {
    match analyse(
        target,
        stdin_name,
        explicit_config,
        resource,
        coverage,
        bounds,
        json,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            report_failure(&error);
            Outcome::Failed
        }
    }
}

/// The run proper. Every failure is a [`ToolError`] carrying blame.
fn analyse(
    target: Option<&Path>,
    stdin_name: Option<&str>,
    explicit_config: Option<&Path>,
    resource: Option<ResourceKind>,
    detail: bool,
    bounds: bool,
    json: bool,
) -> Result<Outcome, ToolError> {
    // Two different notions of "where", deliberately separated. Configuration
    // is discovered from the working directory when there is no path, because
    // a caller piping a snippet is still working inside a project and should
    // get that project's rules. What the run *reports* itself as having
    // analysed is the snippet's name, because telling a user the run covered
    // `.` when it read one buffer would be a claim about a whole tree.
    let anchor = target.unwrap_or_else(|| Path::new("."));
    let config = config::load(anchor, explicit_config)?;

    // Source read once, before the walk, so a failure to read it is a tool
    // error rather than a file that mysteriously vanished mid-run.
    let piped = match stdin_name {
        Some(name) => Some((name.to_owned(), read_stdin(name)?)),
        None => None,
    };

    let reported_as = stdin_name.map_or(anchor, Path::new);

    let (kind, mut walk) = match piped.as_ref() {
        // One synthetic unit. Everything downstream - parsing, waivers,
        // coverage, the verdict - treats it exactly as it treats a file, which
        // is the point: a snippet must not take a second code path that can
        // drift from the first.
        Some((name, _)) => (
            crate::sources::Target::File,
            crate::sources::Walk {
                sources: vec![std::path::PathBuf::from(name)],
                problems: Vec::new(),
            },
        ),
        None => crate::sources::collect(anchor)?,
    };

    let mut findings = 0usize;
    let mut inconclusive = 0usize;
    let mut statements = 0usize;
    let mut waived = Tally::default();
    let mut coverage = Coverage::new();
    // Structured output is built alongside the text rather than instead of it,
    // so the two cannot disagree about what the run found. In JSON mode the
    // text is withheld at the point of writing, not skipped at the point of
    // computing - a second code path would be a second thing to keep correct.
    let mut collected = json.then(machine::Collector::default);
    let mut report = Report::new_gated(std::io::stdout().lock(), !json);

    for path in &walk.sources {
        // A file that could not be read is recorded and the walk continues, so
        // that one run names every path it could not look at rather than one
        // per invocation.
        let text = match piped.as_ref() {
            Some((_, text)) => text.clone(),
            None => match read_source(path) {
                Ok(text) => text,
                Err(problem) => {
                    walk.problems.push(problem);
                    continue;
                }
            },
        };
        match landav_python::analyze_module_with(path, &text, config.waivers()) {
            Ok(module) => {
                statements += module.statements();
                findings += module.findings().len();
                if let Some(sink) = collected.as_mut() {
                    sink.absorb_findings(&module);
                }
                publish(&mut report, &module);
                waived.absorb(&mut report, &module);
                // Only for a file that parsed. A file the frontend could not
                // read offered no function to lower, and counting it as a
                // refused construct would file a parser limitation under a
                // language construct nobody wrote.
                if let Err(problem) = accumulate(
                    path,
                    &text,
                    &mut coverage,
                    bounds.then_some(&mut report),
                    collected.as_mut(),
                ) {
                    walk.problems.push(problem);
                }
            }
            Err(PythonError::Parse {
                line,
                column,
                detail,
                ..
            }) => {
                inconclusive += 1;
                if let Some(sink) = collected.as_mut() {
                    sink.problem(
                        Some(path.display().to_string()),
                        format!("unreadable as Python at {line}:{column}: {detail}"),
                    );
                }
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

    // Configured waivers are decided once and applied to many files, so they
    // are folded and reported after the walk rather than once per file. This
    // is also what names a waiver whose glob matched nothing at all, which no
    // per-file record could.
    waived.publish_configured(&mut report, config.waivers());

    // A resource was asked about and this build derives no bound for any of
    // them, so the run has nothing to say about the question it was asked.
    // Reported before the summary, on its own line, so that it reads like the
    // other inconclusive results — which is what it is.
    let unaccounted = resource_unaccounted(resource, statements);
    if let Some(kind) = resource
        && unaccounted
    {
        report.line(format_args!("{}", crate::resource::unaccounted(kind)));
    }

    // The full report, only when it was asked for. Printed before the summary,
    // like the findings, so that the summary stays the last line of the run.
    if detail {
        for line in coverage.report().lines() {
            report.line(format_args!("{line}"));
        }
    }

    summarise(
        &mut report,
        reported_as,
        &config,
        &walk.sources,
        findings,
        &waived,
        inconclusive,
        resource,
        &coverage,
    );
    // Printed *after* the summary rather than before it, so that the summary
    // remains the first `landav:` line of the run — a contract the suppression
    // suite reads the run's counts off.
    if !coverage.is_complete() {
        report.line(format_args!("{}", partial_analysis(&coverage, detail)));
    }
    // A report that never reached the operator is not a report, so a stream
    // that could not be written is a reason the run did not complete — and
    // therefore a `2` with blame, not a silent verdict about findings nobody
    // saw. `landav check src/ | head -1` takes this path.
    if let Some(problem) = report.finish() {
        walk.problems.push(problem);
    }

    let mut outcome = if walk.problems.is_empty() {
        classify(
            kind,
            walk.sources.len(),
            statements,
            findings,
            inconclusive,
            unaccounted,
            detail && !coverage.is_complete(),
        )
    } else {
        Outcome::Failed
    };

    if let Some(sink) = collected {
        // Emitted after the outcome is decided, so the JSON can carry the same
        // verdict the exit code does. A consumer must never have to infer one
        // from the other.
        let run = sink.finish(
            outcome,
            machine::Summary {
                files_analysed: walk.sources.len(),
                statements,
                functions: coverage.units(),
                lowered: coverage.lowered(),
                coverage_percent: coverage.percent(),
                refusals: coverage.refusals(),
                findings,
                suppressed: waived.suppressed(),
                stale_waivers: waived.stale(),
            },
            &walk.problems,
        );
        match serde_json::to_string_pretty(&run) {
            Ok(text) => println!("{text}"),
            Err(why) => {
                // A consumer that asked for JSON and received nothing must not
                // read the silence as a clean run. The verdict is overridden
                // here rather than only recorded, because it has already been
                // decided by this point and a problem pushed now would not
                // reach it.
                walk.problems.push(ToolError::at_path(
                    anchor,
                    format!("could not render the run as JSON: {why}"),
                ));
                outcome = Outcome::Failed;
            }
        }
    }

    // Sorted, so that identical input produces identical stderr whatever order
    // the filesystem handed the entries back in.
    walk.problems.sort_by_key(ToolError::to_string);
    for problem in &walk.problems {
        report_failure(problem);
    }
    if outcome == Outcome::NothingAnalysed {
        report_failure(&nothing_to_analyse(reported_as, walk.sources.len()));
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

/// What the run learned about its waivers.
///
/// Inline waivers are printed as they are met, beside the file they are in.
/// Configured ones are folded here and printed once at the end: a glob over a
/// thousand-file tree is one waiver the author wrote, not a thousand.
#[derive(Debug, Default)]
struct Tally {
    /// Findings removed by a waiver, of either kind.
    suppressed: usize,
    /// Waivers that removed nothing: unused, retired, or naming no rule.
    stale: usize,
    /// Findings credited to each `(pattern, rule code)`.
    credited: BTreeMap<(String, String), usize>,
}

impl Tally {
    /// Findings removed by a waiver.
    const fn suppressed(&self) -> usize {
        self.suppressed
    }

    /// Waivers that removed nothing.
    const fn stale(&self) -> usize {
        self.stale
    }
}

impl Tally {
    /// Records one module's waivers, printing the inline ones.
    fn absorb<W: std::io::Write>(&mut self, report: &mut Report<W>, module: &ModuleAnalysis) {
        for record in module.suppressions() {
            self.suppressed += record.suppressed();
            match record.origin().pattern() {
                // A configured waiver's verdict is not final until every file
                // has been walked, so it is counted and not yet judged.
                Some(pattern) => {
                    *self
                        .credited
                        .entry((pattern.to_owned(), record.code().to_owned()))
                        .or_default() += record.suppressed();
                }
                None => {
                    if record.is_stale() {
                        self.stale += 1;
                    }
                    report.line(format_args!(
                        "{}:{}: suppressed: {}: {}",
                        record
                            .origin()
                            .file()
                            .unwrap_or_else(|| Path::new("<unknown>"))
                            .display(),
                        record.origin().line().unwrap_or_default(),
                        record.code(),
                        describe(record)
                    ));
                }
            }
        }
    }

    /// Prints one line per configured waiver, in the order they were written.
    ///
    /// Driven from the configuration rather than from what the files produced,
    /// so a glob that matched no file at all — the stale path that a directory
    /// move leaves behind — is named instead of vanishing.
    fn publish_configured<W: std::io::Write>(
        &mut self,
        report: &mut Report<W>,
        waivers: &[landav_python::PathWaiver],
    ) {
        let mut seen = BTreeSet::new();
        for waiver in waivers {
            for code in waiver.rules() {
                let key = (waiver.pattern().to_owned(), code.clone());
                if !seen.insert(key.clone()) {
                    // Two entries waiving the same code over the same glob are
                    // one waiver as far as the findings are concerned;
                    // reporting the credit twice would double-count it.
                    continue;
                }
                // Declared with nothing to its name, then credited with what
                // the walk found. Folding through the record rather than
                // summing beside it is what keeps the status derived rather
                // than asserted: a waiver that fired in the last file of the
                // tree comes out applied, not stale.
                let record = Suppression::per_path(
                    code.clone(),
                    waiver.pattern().to_owned(),
                    Some(waiver.reason().to_owned()),
                    0,
                )
                .crediting(self.credited.get(&key).copied().unwrap_or_default());
                if record.is_stale() {
                    self.stale += 1;
                }
                report.line(format_args!(
                    "{}: suppressed: {}: {}",
                    waiver.pattern(),
                    record.code(),
                    describe(&record)
                ));
            }
        }
    }
}

/// The half of a suppression line that says what the waiver did.
///
/// Written as an exhaustive match with no wildcard arm: a fifth
/// [`SuppressionStatus`] is a compile error here until somebody decides what
/// the operator is told about it, which is the same reasoning
/// [`crate::outcome`] uses for the exit codes.
fn describe(record: &Suppression) -> String {
    let scope = match record.origin().pattern() {
        Some(_) => "waived by configuration",
        None => "waived inline",
    };
    match record.status() {
        SuppressionStatus::Applied => format!(
            "{} {scope}: {}",
            plural(record.suppressed(), "finding"),
            record.reason().unwrap_or("no reason given")
        ),
        SuppressionStatus::Unused => format!(
            "{scope}, but the rule did not fire in the code this covers; the waiver \
             is doing nothing and can be removed"
        ),
        SuppressionStatus::Retired => format!(
            "{scope}, but this code was issued and then withdrawn; it is never \
             reported and never reused, so the waiver has no effect"
        ),
        SuppressionStatus::Unknown => format!(
            "{scope}, but no landav rule has ever carried this code; nothing is \
             waived by it, so check the spelling"
        ),
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
    /// Whether anything is actually written.
    ///
    /// A JSON run still walks every line the text run would, and discards
    /// them here. Withholding at the point of *writing* rather than skipping
    /// at the point of *computing* means there is one code path deciding what
    /// a run found, so the two formats cannot come to different conclusions.
    writing: bool,
    /// Where the report goes.
    out: W,
    /// The first write failure, if any. Later writes are skipped: a stream
    /// that has failed once will not start working again mid-run, and
    /// retrying only multiplies the diagnostics.
    failure: Option<std::io::Error>,
}

impl<W: std::io::Write> Report<W> {
    /// A report over `out`, with nothing written and nothing failed.
    /// A report that writes only when `writing`.
    const fn new_gated(out: W, writing: bool) -> Self {
        Self {
            writing,
            out,
            failure: None,
        }
    }

    /// Write one record, terminated by a newline.
    fn line(&mut self, args: std::fmt::Arguments<'_>) {
        if !self.writing {
            return;
        }
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

/// Whether the run owes the operator an unaccounted-for-resource result.
///
/// True when a resource was named and there was code to account for it in.
///
/// The `statements` qualification is the whole content of this function, and it
/// is why it is a function rather than an expression inline in [`analyse`]: a
/// run that analysed no code at all has a stronger and more actionable thing to
/// say about itself, and [`classify`] says it. Announcing that a resource was
/// left unaccounted for over a tree that held no code buries "this path matches
/// nothing" — a stale glob the caller can fix today — under a milestone
/// limitation that will be equally true of every run until bound inference
/// lands.
const fn resource_unaccounted(resource: Option<ResourceKind>, statements: usize) -> bool {
    resource.is_some() && statements > 0
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
///
/// # Where a selected resource sits in that order, and why it is last
///
/// `unaccounted` is LAN-60: a resource was named and no bound for it was
/// derived. It ranks **below** everything the run established about the code
/// and below the emptiness check, and above clean.
///
/// Below the emptiness check because the two answers compete and one of them is
/// worth more. "This target holds no code" is a fact about the caller's
/// invocation that they can act on today — a path that has stopped matching
/// after a directory move — while "no bound was derived" is a fact about this
/// milestone that will be equally true of every run until the analysis tier
/// lands. Letting the milestone limitation win would mask the stale path with a
/// message nobody can do anything about, and turn a `2` into a `1`.
///
/// Above clean because exit `0` claims analysis ran and every bound held. There
/// is no bound and it did not hold; saying so is the difference between a
/// verdict and a fabrication.
///
/// # Where a refused construct sits, and why it is `Inconclusive`
///
/// `refused` is LAN-68: the caller asked how much of the target became an
/// integer transition system, and the answer was "not all of it".
///
/// It is **never** [`Outcome::Clean`], and that is the whole story. A construct
/// the lowering will not take produces no transition; a system missing a
/// transition admits fewer executions than the program has, so a bound derived
/// from it can be exceeded. Exit `0` claims analysis ran and every bound held,
/// and here part of the program was never turned into anything a bound could be
/// derived from. A clean code for that is unsound by omission and looks exactly
/// like a real clean result, which is the failure this story exists to prevent.
///
/// It is [`Outcome::Inconclusive`] rather than [`Outcome::Failed`] for the
/// reason `crate::outcome` gives: the tool ran to completion and produced a
/// result *about the code*, naming the construct and the position, so the
/// person who can act on it is the author of the code and not whoever runs CI.
/// Filing a `sorted()` call the analyser declined to model alongside an
/// unreadable input would page the wrong team, and would teach them that code
/// `2` is noise.
///
/// It ranks beside `unaccounted` and below everything the run established about
/// the code, on the same argument: a finding and an unreadable file are results,
/// and the reach of this milestone's fragment is a limitation. It does not
/// compete with the emptiness check either way — a refusal needs a function,
/// and a function is at least one statement, so `refused` implies
/// `statements > 0`.
const fn classify(
    target: Target,
    files: usize,
    statements: usize,
    findings: usize,
    inconclusive: usize,
    unaccounted: bool,
    refused: bool,
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
    if refused || unaccounted {
        return Outcome::Inconclusive;
    }
    Outcome::Clean
}

/// Translate every function in one file and record whether each one lowered.
///
/// Two steps, two owners, and they are kept apart deliberately.
/// [`landav_python::lower_module`] answers "what does this Python function look
/// like in the numeric fragment" and fails only if the *file* cannot be read;
/// [`landav_its::lower`] answers "can that be turned into a transition system"
/// and fails whenever the function uses something outside the fragment. Fusing
/// them would make "this file has a syntax error" and "this function calls
/// `sorted`" the same kind of failure, and they need different responses — the
/// first is a tool error, the second is a result about the code.
///
/// A function is offered for **every** top-level `def`, including the ones that
/// obviously will not lower. Skipping them would make the denominator flatter
/// as the fragment got narrower, which is exactly backwards.
///
/// # The file is parsed twice
///
/// Once by [`landav_python::analyze_module_with`] for the rules and once here
/// for the lowering. Acceptable at M0 and worth removing when the two entry
/// points can share a parse; it is recorded here rather than left to be
/// rediscovered by whoever profiles a large tree.
///
/// # Errors
///
/// A [`ToolError`] naming the path if the frontend could not translate it at
/// all. The caller has already parsed the file successfully, so this is a
/// disagreement between two entry points rather than a property of the source —
/// it is blamed rather than swallowed, because a coverage denominator that
/// silently lost a file is the omission this story is about.
fn accumulate<W: std::io::Write>(
    path: &Path,
    text: &str,
    coverage: &mut Coverage,
    mut bounds: Option<&mut Report<W>>,
    mut collected: Option<&mut machine::Collector>,
) -> Result<(), ToolError> {
    let functions = landav_python::lower_module(path, text).map_err(|error| {
        ToolError::at_path(
            path,
            format!(
                "parsed for the rules but not for the lowering ({error}), so this file \
                 is missing from the coverage report and the report's denominator \
                 would understate what was skipped"
            ),
        )
    })?;
    for function in &functions {
        let lowered = landav_its::lower(function.program());
        // A function that did not lower has no bound to report either, and
        // saying so is the point: the coverage line counts it, and a `--bounds`
        // run that simply omitted it would read as a bound of zero.
        if let Some(report) = bounds.as_deref_mut() {
            report.line(format_args!(
                "{}",
                describe_bound(function, lowered.is_ok())
            ));
        }
        if let Some(sink) = collected.as_deref_mut() {
            sink.absorb_function(function, lowered.as_ref().map(|_| ()));
        }
        coverage.record(lowered.as_ref());
    }
    Ok(())
}

/// One function's bound, as a line.
///
/// # Why the quantifier is spelled out
///
/// `Theta` and `O` are different claims and the difference is the product.
/// `Theta` says the analysis derived the cost *exactly*; `O` says the true cost
/// may be lower than the number shown. A reader who cannot tell them apart
/// cannot tell a tight answer from a cautious one, which is most of what this
/// tool is for.
///
/// The engine reports its own exactness, so no solver is consulted and none
/// needs to be installed. Where it has no answer the line says that too - the
/// external solver may still find an upper bound, and silence here would be
/// read as zero.
fn describe_bound(function: &landav_python::LoweredFunction, lowered: bool) -> String {
    let at = function.location();
    let where_ = format!("{}:{}:{}", at.file().display(), at.line(), at.column());
    if !lowered {
        return format!(
            "{where_}: {}: no bound: this function did not lower, so nothing was \
             derived for it",
            function.name()
        );
    }
    let derived = landav_engine::cost(function.program());
    match &derived {
        landav_engine::TripCount::Exact(bound) => format!(
            "{where_}: {}: Theta({bound}) - derived exactly",
            function.name()
        ),
        landav_engine::TripCount::AtMost(bound) => format!(
            "{where_}: {}: O({bound}) - an upper bound; the true cost may be lower",
            function.name()
        ),
        landav_engine::TripCount::Partial { bound, holes, .. } => format!(
            "{where_}: {}: {}({bound}) apart from {}",
            function.name(),
            // The qualifier still describes what *was* derived. "Exact except
            // for the `while` at line 42" is actionable; "at most infinity" is
            // not, and collapsing to it would throw away the useful half.
            if derived.exact_outside_holes() {
                "Theta"
            } else {
                "O"
            },
            describe_holes(holes, &at.file().display().to_string())
        ),
        landav_engine::TripCount::Unknown => format!(
            "{where_}: {}: no bound: the native engine could not read this function \
             at all, so it is covered only by whatever an external solver reports",
            function.name()
        ),
    }
}

/// The unanalysed regions, named and placed.
///
/// This is the blame. "No bound" tells a user nothing they can act on; naming
/// the construct and the line tells them exactly what to change, or what to
/// point a solver at.
fn describe_holes(holes: &[landav_engine::Hole], file: &str) -> String {
    holes
        .iter()
        .map(|hole| {
            // The line already opens with this file's path, so repeating it per
            // hole turns a readable sentence into three lines of noise. Trimmed
            // to the position, which is the part that differs.
            let origin = hole.origin().as_str();
            let at = origin
                .strip_prefix(file)
                .map_or(origin, |rest| rest.trim_start_matches(':'));
            format!("{} ({} at {at})", hole.var().symbol(), hole.construct())
        })
        .collect::<Vec<String>>()
        .join(", ")
}

/// The line a run prints when part of what it looked at did not lower.
///
/// Printed on **every** run that refused something, with or without
/// `--coverage`, because this is the sentence that stops a partly-analysed file
/// reading as a whole one. The detail — every construct, every position, and
/// the constructs that were never met — is behind the flag; the fact is not.
fn partial_analysis(coverage: &Coverage, detail: bool) -> String {
    let mut line = format!(
        "landav: coverage: {} of {} function(s) became an integer transition system",
        coverage.lowered(),
        coverage.units()
    );
    if let Some((construct, count)) = coverage.dominant() {
        line.push_str(&format!(
            "; {} construct(s) out of scope, most often {} ({}) ×{count}",
            coverage.refusals(),
            construct.tag(),
            construct.describe()
        ));
    }
    if coverage.malformed() > 0 {
        line.push_str(&format!(
            "; {} function(s) the frontend built wrongly",
            coverage.malformed()
        ));
    }
    line.push_str(
        "; a function that did not lower produces no transition system, so nothing is \
         derived from it and no bound reported here covers it",
    );
    if !detail {
        line.push_str(" — run again with --coverage for the construct list and positions");
    }
    line
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
///
/// The suppression counts are printed on **every** run, including the runs
/// with none, because that is what makes them greppable: a CI job that watches
/// the number can only notice it going up if the number is always there.
/// `LAN-66` criterion 3 is this line — a waiver that never appears in a
/// summary is a waiver nobody will ever revisit.
///
/// The coverage ratio is here on the same argument and for a sharper reason.
/// Without it the summary says "3 files analysed" and stops, which invites the
/// reading `LAN-68` exists to prevent: that the three files were analysed
/// *whole*. The clause is on every run — complete, partial and with no function
/// to lower at all — so that a number a CI job watches is always there to be
/// watched, and so that the day it drops there is a baseline to notice against.
/// The selected resource is named here on **every** run that named one,
/// including the runs that found nothing to analyse and so print no
/// unaccounted-for line. A summary that does not say which resource was asked
/// about cannot be filed against the invocation that produced it, and
/// `--resource ops` and `--resource alloc` share an algebra, so the algebra
/// alone would not tell them apart — see [`crate::resource`].
#[expect(
    clippy::too_many_arguments,
    reason = "the summary is a projection of the run's counts; bundling them \
              into a struct would put a constructor between each count and the \
              line it appears on"
)]
fn summarise<W: std::io::Write>(
    report: &mut Report<W>,
    target: &Path,
    config: &Config,
    sources: &[std::path::PathBuf],
    findings: usize,
    waived: &Tally,
    inconclusive: usize,
    resource: Option<ResourceKind>,
    coverage: &Coverage,
) {
    report.line(format_args!(
        "landav: {} analysed under {} — {} finding(s), {} suppressed, {}, {} inconclusive; \
         {}; resource: {}; configuration: {}",
        plural(sources.len(), "file"),
        target.display(),
        findings,
        waived.suppressed,
        plural(waived.stale, "stale waiver"),
        inconclusive,
        coverage.summary(),
        describe_resource(resource),
        config.source()
    ));
}

/// The summary's resource clause.
///
/// Printed even when no resource was selected, for the same reason the
/// suppression counts are printed on runs with none: a number a CI job watches
/// can only be seen to change if it is always there.
fn describe_resource(resource: Option<ResourceKind>) -> String {
    match resource {
        Some(kind) => crate::resource::selected(kind),
        None => "none selected".to_owned(),
    }
}

/// `1 file` / `2 files`, so the summary reads as English.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Read Python source from standard input.
///
/// # Why empty input is a failure rather than a clean run
///
/// A caller that piped nothing - a broken shell pipeline, a variable that
/// expanded to nothing, an editor sending an unsaved buffer - would otherwise
/// get a green run that analysed no code at all. That is the same failure
/// [`nothing_to_analyse`] exists to prevent for a directory that stopped
/// matching, and it is worse here because there is no path to go and inspect.
///
/// Whitespace-only counts as empty. It parses as a valid module with no
/// statements, so it would otherwise be indistinguishable from a successful
/// analysis of nothing.
fn read_stdin(name: &str) -> Result<String, ToolError> {
    use std::io::Read as _;

    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|err| ToolError::at_path(Path::new(name), format!("cannot be read: {err}")))?;

    if text.trim().is_empty() {
        return Err(ToolError::at_path(
            Path::new(name),
            "was empty, so the run analysed no code at all; a caller that meant to \
             send source is looking at a broken pipeline rather than a clean result"
                .to_owned(),
        ));
    }
    Ok(text)
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
    use super::{
        Report, classify, describe_resource, partial_analysis, plural, resource_unaccounted,
    };
    use crate::outcome::Outcome;
    use crate::sources::Target;
    use landav_bound::ResourceKind;
    use landav_its::Coverage;

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
        let mut report = Report::new_gated(BrokenPipe, true);
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
        let mut report = Report::new_gated(Vec::new(), true);
        report.line(format_args!("delivered"));
        assert!(report.finish().is_none());
    }

    /// A file the frontend could not parse contributes no statements this
    /// crate can count. The emptiness rule must not outrank it, or the
    /// strongest thing the run learned would read as the weakest.
    #[test]
    fn an_unparsable_file_is_inconclusive_rather_than_nothing_analysed() {
        assert_eq!(
            classify(Target::File, 1, 0, 0, 1, false, false),
            Outcome::Inconclusive
        );
        assert_eq!(
            classify(Target::Directory, 1, 0, 0, 1, false, false),
            Outcome::Inconclusive
        );
        assert_ne!(
            classify(Target::File, 1, 0, 0, 1, false, false),
            Outcome::Clean
        );
    }

    #[test]
    fn a_directory_with_no_files_is_never_clean() {
        assert_eq!(
            classify(Target::Directory, 0, 0, 0, 0, false, false),
            Outcome::NothingAnalysed
        );
    }

    #[test]
    fn a_directory_of_files_with_no_statements_is_never_clean() {
        assert_eq!(
            classify(Target::Directory, 3, 0, 0, 0, false, false),
            Outcome::NothingAnalysed
        );
    }

    /// R3. A file the caller named by hand cannot be a path that stopped
    /// matching, so an empty `__init__.py` is not "the tool could not look".
    #[test]
    fn a_named_file_with_no_statements_is_not_a_tool_error() {
        assert_eq!(
            classify(Target::File, 1, 0, 0, 0, false, false),
            Outcome::Clean
        );
    }

    #[test]
    fn an_inconclusive_unit_is_not_absorbed_by_clean_neighbours() {
        assert_eq!(
            classify(Target::Directory, 2, 40, 0, 1, false, false),
            Outcome::Inconclusive
        );
        assert_ne!(
            classify(Target::Directory, 2, 40, 0, 1, false, false),
            Outcome::Clean
        );
    }

    #[test]
    fn a_proven_run_is_clean() {
        assert_eq!(
            classify(Target::File, 1, 6, 0, 0, false, false),
            Outcome::Clean
        );
    }

    #[test]
    fn findings_outrank_inconclusive_in_what_is_reported() {
        assert_eq!(
            classify(Target::Directory, 2, 40, 1, 1, false, false),
            Outcome::Findings
        );
    }

    /// LAN-60. A resource was named, no bound for it was derived, and the run
    /// analysed real code: exit `0` would claim a bound held that was never
    /// computed.
    #[test]
    fn a_resource_nothing_could_bound_is_never_clean() {
        assert_eq!(
            classify(Target::File, 1, 6, 0, 0, true, false),
            Outcome::Inconclusive
        );
        assert_eq!(
            classify(Target::Directory, 2, 40, 0, 0, true, false),
            Outcome::Inconclusive
        );
        // The control: the same run without a resource selected is clean, so
        // the outcome is a consequence of the question asked.
        assert_eq!(
            classify(Target::File, 1, 6, 0, 0, false, false),
            Outcome::Clean
        );
    }

    /// "This target holds no code" outranks "no bound was derived for the
    /// resource you named". The first is a fact about the caller's invocation
    /// that they can act on today; the second is true of every run in this
    /// milestone, and letting it win would mask a path that has stopped
    /// matching behind a limitation nobody can do anything about.
    #[test]
    fn nothing_analysed_outranks_an_unbounded_resource() {
        assert_eq!(
            classify(Target::Directory, 0, 0, 0, 0, true, false),
            Outcome::NothingAnalysed
        );
        assert_eq!(
            classify(Target::Directory, 3, 0, 0, 0, true, false),
            Outcome::NothingAnalysed
        );
    }

    /// Everything the run established about the code outranks it too: a
    /// finding and an unreadable file are results, and a milestone limitation
    /// is not.
    #[test]
    fn established_results_outrank_an_unbounded_resource() {
        assert_eq!(
            classify(Target::Directory, 2, 40, 1, 0, true, false),
            Outcome::Findings
        );
        assert_eq!(
            classify(Target::Directory, 2, 40, 0, 1, true, false),
            Outcome::Inconclusive
        );
    }

    /// The unaccounted-for-resource result is owed only when there was code to
    /// account for. A run over a tree that held nothing has "this target holds
    /// no code" to report, and that must not be buried under a caveat that is
    /// true of every run in this milestone.
    #[test]
    fn a_run_that_analysed_nothing_is_owed_no_resource_result() {
        assert!(!resource_unaccounted(Some(ResourceKind::Ops), 0));
        assert!(!resource_unaccounted(None, 40));
        assert!(!resource_unaccounted(None, 0));
        assert!(resource_unaccounted(Some(ResourceKind::Ops), 1));
        assert!(resource_unaccounted(Some(ResourceKind::PeakMem), 40));
    }

    /// The summary names the selected resource, and says so plainly when there
    /// is none — the clause is on every run so that a CI job watching it can
    /// see it change.
    #[test]
    fn the_summary_names_the_resource_it_was_asked_about() {
        assert_eq!(describe_resource(None), "none selected");
        assert_eq!(
            describe_resource(Some(ResourceKind::Ops)),
            crate::resource::selected(ResourceKind::Ops)
        );
        // Two resources over one algebra must not summarise identically; see
        // `crate::resource` and `landav_bound::CacheKeyMaterial`.
        assert_ne!(
            describe_resource(Some(ResourceKind::Ops)),
            describe_resource(Some(ResourceKind::Alloc))
        );
    }

    #[test]
    fn the_summary_counts_read_as_english() {
        assert_eq!(plural(1, "file"), "1 file");
        assert_eq!(plural(0, "file"), "0 files");
        assert_eq!(plural(2, "file"), "2 files");
    }

    // -----------------------------------------------------------------------
    // LAN-68
    // -----------------------------------------------------------------------

    /// A refused construct is never clean, whichever way the target was named.
    ///
    /// The central assertion of `LAN-68` at the level the ordering is decided.
    /// Exit `0` claims analysis ran and every bound held; part of the program
    /// was never turned into anything a bound could be derived from.
    #[test]
    fn a_refused_construct_is_never_clean() {
        assert_eq!(
            classify(Target::File, 1, 6, 0, 0, false, true),
            Outcome::Inconclusive
        );
        assert_eq!(
            classify(Target::Directory, 2, 40, 0, 0, false, true),
            Outcome::Inconclusive
        );
        // The control: the same run with nothing refused is clean, so the
        // outcome is a consequence of the refusal and not of the shape.
        assert_eq!(
            classify(Target::File, 1, 6, 0, 0, false, false),
            Outcome::Clean
        );
    }

    /// Everything the run established about the code still outranks it.
    ///
    /// A finding and an unreadable file are results; the reach of this
    /// milestone's fragment is a limitation, and a limitation must not mask a
    /// result somebody can act on today.
    #[test]
    fn established_results_outrank_a_refused_construct() {
        assert_eq!(
            classify(Target::Directory, 2, 40, 1, 0, false, true),
            Outcome::Findings
        );
        assert_eq!(
            classify(Target::Directory, 2, 40, 0, 1, false, true),
            Outcome::Inconclusive
        );
    }

    /// A refusal and an unbounded resource are the same tier, and neither can
    /// be silently absorbed by the other.
    #[test]
    fn a_refusal_and_an_unbounded_resource_share_a_tier() {
        assert_eq!(
            classify(Target::File, 1, 6, 0, 0, true, true),
            Outcome::Inconclusive
        );
        assert_eq!(
            classify(Target::File, 1, 6, 0, 0, true, false),
            Outcome::Inconclusive
        );
        assert_eq!(
            classify(Target::File, 1, 6, 0, 0, false, true),
            Outcome::Inconclusive
        );
    }

    /// The line a reader sees names the ratio, the construct and the
    /// consequence, and points at the detail only when the detail was not
    /// already printed.
    #[test]
    fn the_partial_analysis_line_says_what_it_means() {
        let coverage = Coverage::new();
        let line = partial_analysis(&coverage, false);
        assert!(
            line.contains("0 of 0"),
            "the line must carry the ratio: {line}"
        );
        assert!(
            line.contains("no transition system"),
            "the line must say what a refusal costs: {line}"
        );
        assert!(
            line.contains("--coverage"),
            "a default run must say where the detail is: {line}"
        );
        assert!(
            !partial_analysis(&coverage, true).contains("--coverage"),
            "a run that already printed the detail must not advertise it again"
        );
    }

    /// A frontend defect is named on the line, and only when there is one.
    ///
    /// Closes a mutant `cargo mutants` left alive: nothing exercised the
    /// malformed clause, so a build that always printed it — or never did —
    /// passed. Both directions matter. Always printing it puts "1 function(s)
    /// the frontend built wrongly" on runs where the frontend is fine, which
    /// trains a reader to skip the clause; never printing it hides the one
    /// failure in this report that is landav's own bug rather than a property
    /// of the analysed code.
    #[test]
    fn the_partial_analysis_line_names_a_frontend_defect_only_when_there_is_one() {
        use landav_its::{Construct, SourceProgramBuilder};

        let origin = |line: u32| landav_bound::Origin::new(format!("unit.py:{line}:1"));

        // A refusal: a language construct outside the fragment.
        let mut builder = SourceProgramBuilder::new("refuser", origin(1), vec![]);
        let offending = builder.unsupported_stmt(Construct::Call, origin(4));
        let refused = builder.build(vec![offending]);
        let refused = landav_its::lower(&refused);
        assert!(refused.is_err(), "a call is outside the fragment");
        let Err(refusal) = refused else { return };

        // A frontend defect: a body naming a statement from another program.
        let mut donor = SourceProgramBuilder::new("donor", origin(1), vec![]);
        let stranger = donor.return_stmt(origin(9));
        let _ = donor.build(vec![stranger]);
        let broken =
            SourceProgramBuilder::new("built_wrong", origin(1), vec![]).build(vec![stranger]);
        let broken = landav_its::lower(&broken);
        assert!(broken.is_err(), "the handle names no node in this program");
        let Err(defect) = broken else { return };

        let mut refusals_only = Coverage::new();
        refusals_only.record(Err(&refusal));
        assert!(
            !partial_analysis(&refusals_only, false).contains("frontend"),
            "a refused construct was reported as a frontend defect: {}",
            partial_analysis(&refusals_only, false)
        );

        let mut with_defect = Coverage::new();
        with_defect.record(Err(&defect));
        assert!(
            partial_analysis(&with_defect, false).contains("frontend"),
            "a malformed program was not reported at all: {}",
            partial_analysis(&with_defect, false)
        );
    }
}
