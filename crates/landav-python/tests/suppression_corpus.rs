//! `LAN-66` acceptance criteria 1 and 3, over `tests/suppression/`.
//!
//! * **AC 1** — a `# noqa`-style inline comment naming a rule code silences
//!   that rule on that line, and nothing else.
//! * **AC 3** — every waiver is reported, including the ones that suppressed
//!   nothing, so that a waiver cannot rot in silence.
//!
//! # Why this corpus is a sibling of `tests/fixtures/`, not a member of it
//!
//! The `LAN-65` corpus is shaped `{code}_{rule name}/{positive,negative}/`,
//! its negative half asserts *zero findings from any rule*, and both halves
//! are certified. A suppression fixture is neither: it contains a real defect
//! **and** reports nothing, which is exactly what a negative fixture is
//! forbidden to look like. Filing these under `tests/fixtures/` would either
//! break that harness or quietly weaken it, and the 55 negatives are the
//! shared false-positive suite. So the tree sits beside it and nothing in
//! `common/` walks it.
//!
//! # How an expectation is written
//!
//! A marker comment sits on its own line, directly above the source line it is
//! about. The line number is therefore never written down and never drifts
//! when somebody edits the file above it.
//!
//! ```python
//! # LANDAV-FINDING: LAV003
//! # LANDAV-WAIVER: LAV010 status=retired count=0 reason=deliberate
//! out += str(row)  # noqa: LAV010 - deliberate
//! ```
//!
//! * `# LANDAV-FINDING: CODE` — a finding of `CODE` must still be reported on
//!   the next source line. A waived finding must **not** be marked, so the
//!   set equality below is what proves a waiver removed it.
//! * `# LANDAV-WAIVER: CODE status=... count=N [reason=...]` — a suppression
//!   record with exactly that status and count must be reported for the next
//!   source line. `reason=` runs to the end of the line and must come last; an
//!   absent `reason=` asserts that no reason was recorded, so a parser that
//!   invented one would fail here.
//!
//! Both directions are asserted as **set equality**, not containment: a
//! fixture that produces an extra finding, or an extra waiver, fails.

// See `common/mod.rs` for why the panic lints are relaxed in test code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::{
    fs,
    path::{Path, PathBuf},
};

use landav_python::{
    ModuleAnalysis, PathWaiver, SuppressionStatus, analyze_module, analyze_module_with,
};

/// Marks a finding that must survive on the next source line.
const FINDING_MARKER: &str = "# LANDAV-FINDING:";

/// Marks a suppression record that must be reported for the next source line.
const WAIVER_MARKER: &str = "# LANDAV-WAIVER:";

/// The smallest corpus that covers every status a waiver can end in.
const MINIMUM_FIXTURES: usize = 9;

/// A finding a fixture asserts must still be reported.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ExpectedFinding {
    line: u32,
    code: String,
}

/// A suppression record a fixture asserts must be reported.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ExpectedWaiver {
    line: u32,
    code: String,
    status: String,
    count: usize,
    reason: Option<String>,
}

/// One fixture and everything it asserts.
struct Case {
    path: PathBuf,
    label: String,
    source: String,
    findings: Vec<ExpectedFinding>,
    waivers: Vec<ExpectedWaiver>,
}

impl Case {
    /// The analysis, with no configured waivers: inline suppression only.
    fn analyse(&self) -> ModuleAnalysis {
        match analyze_module(&self.path, &self.source) {
            Ok(module) => module,
            Err(error) => panic!("{}: fixture failed to analyse: {error}", self.label),
        }
    }

    fn observed_findings(&self) -> Vec<ExpectedFinding> {
        let mut observed: Vec<ExpectedFinding> = self
            .analyse()
            .findings()
            .iter()
            .map(|finding| ExpectedFinding {
                line: finding.location().line(),
                code: finding.rule().as_str().to_owned(),
            })
            .collect();
        observed.sort();
        observed
    }

    fn observed_waivers(&self) -> Vec<ExpectedWaiver> {
        let mut observed: Vec<ExpectedWaiver> = self
            .analyse()
            .suppressions()
            .iter()
            .map(|record| ExpectedWaiver {
                line: record.origin().line().unwrap_or(0),
                code: record.code().to_owned(),
                status: record.status().as_str().to_owned(),
                count: record.suppressed(),
                reason: record.reason().map(str::to_owned),
            })
            .collect();
        observed.sort();
        observed
    }
}

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("suppression")
}

fn load_corpus() -> Vec<Case> {
    let root = corpus_root();
    let files = common::collect_python_files(&root);
    assert!(
        !files.is_empty(),
        "no suppression fixtures under {}",
        root.display()
    );

    files
        .into_iter()
        .map(|path| {
            let label = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<non-UTF-8>")
                .to_owned();
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{label}: unreadable: {error}"));
            let (findings, waivers) = parse_markers(&label, &source);
            Case {
                path,
                label,
                source,
                findings,
                waivers,
            }
        })
        .collect()
}

/// Reads every marker, resolving each to the source line beneath it.
fn parse_markers(label: &str, source: &str) -> (Vec<ExpectedFinding>, Vec<ExpectedWaiver>) {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();
    let mut waivers = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        if let Some(rest) = line.split_once(FINDING_MARKER) {
            let target = target_line(label, &lines, index);
            findings.push(ExpectedFinding {
                line: target,
                code: rest.1.trim().to_owned(),
            });
        } else if let Some(rest) = line.split_once(WAIVER_MARKER) {
            let target = target_line(label, &lines, index);
            waivers.push(parse_waiver(label, target, rest.1.trim()));
        }
    }

    findings.sort();
    waivers.sort();
    (findings, waivers)
}

/// The 1-based number of the first source line beneath `index` that is neither
/// blank nor another marker.
fn target_line(label: &str, lines: &[&str], index: usize) -> u32 {
    for (offset, line) in lines.iter().enumerate().skip(index + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with(FINDING_MARKER)
            || trimmed.starts_with(WAIVER_MARKER)
        {
            continue;
        }
        return u32::try_from(offset + 1).expect("fixtures are not billions of lines");
    }
    panic!(
        "{label}:{}: marker with no source line beneath it",
        index + 1
    )
}

/// `CODE status=... count=N [reason=...]`, with `reason=` running to the end.
fn parse_waiver(label: &str, line: u32, directive: &str) -> ExpectedWaiver {
    let (code, rest) = directive
        .split_once(char::is_whitespace)
        .unwrap_or_else(|| panic!("{label}:{line}: waiver marker has no attributes"));

    let (attributes, reason) = match rest.split_once("reason=") {
        Some((head, tail)) => (head, Some(tail.trim().to_owned())),
        None => (rest, None),
    };

    let mut status = None;
    let mut count = None;
    for attribute in attributes.split_whitespace() {
        match attribute.split_once('=') {
            Some(("status", value)) => status = Some(value.to_owned()),
            Some(("count", value)) => count = value.parse::<usize>().ok(),
            _ => panic!("{label}:{line}: unreadable waiver attribute `{attribute}`"),
        }
    }

    ExpectedWaiver {
        line,
        code: code.to_owned(),
        status: status.unwrap_or_else(|| panic!("{label}:{line}: waiver marker has no status")),
        count: count.unwrap_or_else(|| panic!("{label}:{line}: waiver marker has no count")),
        reason,
    }
}

/// The corpus has to be well formed before anything it asserts means anything.
#[test]
fn corpus_is_well_formed() {
    let corpus = load_corpus();
    assert!(
        corpus.len() >= MINIMUM_FIXTURES,
        "{} suppression fixtures; the four waiver statuses, a foreign directive, a directive \
         inside a string literal and an off-by-one line all need one",
        corpus.len()
    );

    let mut problems = Vec::new();
    for case in &corpus {
        if case.findings.is_empty() && case.waivers.is_empty() {
            problems.push(format!("{}: asserts nothing", case.label));
        }
        for waiver in &case.waivers {
            if !["applied", "unused", "retired", "unknown"].contains(&waiver.status.as_str()) {
                problems.push(format!(
                    "{}: `{}` is not a waiver status",
                    case.label, waiver.status
                ));
            }
            if waiver.status == "applied" && waiver.count == 0 {
                problems.push(format!(
                    "{}: `{}` claims to have applied and suppressed nothing",
                    case.label, waiver.code
                ));
            }
            if waiver.status != "applied" && waiver.count != 0 {
                problems.push(format!(
                    "{}: `{}` is not applied yet claims a count",
                    case.label, waiver.code
                ));
            }
        }
    }

    // Every status has to appear somewhere, or the corpus is silently only
    // testing the happy path.
    for status in ["applied", "unused", "retired", "unknown"] {
        let covered = corpus
            .iter()
            .flat_map(|case| &case.waivers)
            .any(|waiver| waiver.status == status);
        if !covered {
            problems.push(format!("no fixture produces a `{status}` waiver"));
        }
    }

    assert!(
        problems.is_empty(),
        "malformed suppression corpus:\n  {}",
        problems.join("\n  ")
    );
}

/// AC 1. A waived rule stops being reported; everything else still is.
#[test]
fn only_the_unwaived_findings_survive() {
    let mut failures = Vec::new();
    for case in load_corpus() {
        let observed = case.observed_findings();
        if observed != case.findings {
            failures.push(format!(
                "{}\n      expected {:?}\n      observed {:?}",
                case.label, case.findings, observed
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} fixture(s) reported the wrong surviving findings:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// AC 3. Every waiver is reported, with its status, its count and its reason —
/// and no waiver is reported that the fixture did not declare.
#[test]
fn every_waiver_is_reported_exactly_as_declared() {
    let mut failures = Vec::new();
    for case in load_corpus() {
        let observed = case.observed_waivers();
        if observed != case.waivers {
            failures.push(format!(
                "{}\n      expected {:?}\n      observed {:?}",
                case.label, case.waivers, observed
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} fixture(s) reported the wrong waivers:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// Nothing is lost. Every finding the rules produced is either reported or
/// accounted for by a waiver — a suppression that quietly dropped a second
/// finding it never claimed would fail here.
#[test]
fn findings_plus_waived_findings_is_conserved() {
    let mut failures = Vec::new();
    for case in load_corpus() {
        let module = case.analyse();
        let waived: usize = module
            .suppressions()
            .iter()
            .map(landav_python::Suppression::suppressed)
            .sum();
        let total = module.findings().len() + waived;
        let declared = case.findings.len()
            + case
                .waivers
                .iter()
                .map(|waiver| waiver.count)
                .sum::<usize>();
        if total != declared {
            failures.push(format!(
                "{}: {total} finding(s) accounted for, {declared} declared",
                case.label
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "findings went missing:\n  {}",
        failures.join("\n  ")
    );
}

/// Two runs over identical bytes produce identical records, in identical
/// order. A summary that reorders itself between runs is a summary whose diff
/// is worthless.
#[test]
fn suppression_records_are_ordered_and_reproducible() {
    let mut failures = Vec::new();
    for case in load_corpus() {
        let first = case.analyse();
        let second = case.analyse();
        let key = |module: &ModuleAnalysis| {
            module
                .suppressions()
                .iter()
                .map(|record| {
                    (
                        record.origin().line(),
                        record.code().to_owned(),
                        record.suppressed(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let one = key(&first);
        if one != key(&second) {
            failures.push(format!("{}: two runs disagreed", case.label));
        }
        let mut sorted = one.clone();
        sorted.sort();
        if one != sorted {
            failures.push(format!("{}: records are unordered: {one:?}", case.label));
        }
    }
    assert!(
        failures.is_empty(),
        "suppression output is not deterministic:\n  {}",
        failures.join("\n  ")
    );
}

/// Every waiver names where it was written, so `git blame` can name the person
/// who wrote it. That attribution is what the `E-001` governance layer turns
/// into a named approver, and a record without it cannot be governed.
#[test]
fn every_inline_waiver_names_the_file_and_line_it_was_written_on() {
    let mut failures = Vec::new();
    for case in load_corpus() {
        for record in case.analyse().suppressions() {
            let origin = record.origin();
            match (origin.file(), origin.line()) {
                (Some(file), Some(line)) => {
                    if file != case.path {
                        failures.push(format!("{}: waiver names another file", case.label));
                    }
                    if line == 0 || line > u32::try_from(case.source.lines().count()).unwrap_or(0) {
                        failures.push(format!("{}: waiver line {line} is outside", case.label));
                    }
                }
                _ => failures.push(format!(
                    "{}: inline waiver for `{}` has no position",
                    case.label,
                    record.code()
                )),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "waivers cannot be attributed:\n  {}",
        failures.join("\n  ")
    );
}

/// AC 2, at the frontend boundary. A configured waiver covering the file
/// silences the named rule without a comment anywhere in the source.
#[test]
fn a_configured_waiver_suppresses_without_touching_the_source() {
    let case = load_corpus()
        .into_iter()
        .find(|case| case.label == "directive_in_a_string.py")
        .expect("fixture present");

    let before = case.analyse();
    assert_eq!(before.findings().len(), 1, "{}", case.label);

    let waiver = PathWaiver::new(
        "**/directive_in_a_string.py".to_owned(),
        vec!["LAV003".to_owned()],
        "the fixture's defect is the point of the fixture".to_owned(),
    );
    let after = analyze_module_with(&case.path, &case.source, std::slice::from_ref(&waiver))
        .expect("fixture analyses");

    assert!(after.findings().is_empty(), "{}", case.label);
    let record = after.suppressions().first().expect("one record");
    assert_eq!(record.code(), "LAV003");
    assert_eq!(record.status(), SuppressionStatus::Applied);
    assert_eq!(record.suppressed(), 1);
    assert_eq!(
        record.origin().pattern(),
        Some("**/directive_in_a_string.py")
    );
    assert_eq!(record.origin().line(), None);
    assert_eq!(
        record.reason(),
        Some("the fixture's defect is the point of the fixture")
    );
}

/// A configured waiver that covers a file where the rule never fires is
/// reported as unused, exactly like an inline one. Criterion 3 does not stop
/// at the source file.
#[test]
fn a_configured_waiver_that_matched_nothing_is_reported_as_unused() {
    let case = load_corpus()
        .into_iter()
        .find(|case| case.label == "waiver_left_behind.py")
        .expect("fixture present");

    let waiver = PathWaiver::new(
        "**/*.py".to_owned(),
        vec!["LAV009".to_owned()],
        "pandas is not used in this tree".to_owned(),
    );
    let module = analyze_module_with(&case.path, &case.source, std::slice::from_ref(&waiver))
        .expect("fixture analyses");

    let configured: Vec<&landav_python::Suppression> = module
        .suppressions()
        .iter()
        .filter(|record| record.origin().pattern().is_some())
        .collect();
    assert_eq!(configured.len(), 1);
    assert_eq!(configured[0].status(), SuppressionStatus::Unused);
    assert!(configured[0].is_stale());
}

/// A waiver that does not cover the file produces no record for it at all —
/// otherwise every run over a large tree would report every waiver once per
/// file and the report would be unreadable.
#[test]
fn a_configured_waiver_that_does_not_cover_the_file_is_not_recorded_against_it() {
    let case = load_corpus()
        .into_iter()
        .find(|case| case.label == "directive_in_a_string.py")
        .expect("fixture present");

    let waiver = PathWaiver::new(
        "somewhere/else/**".to_owned(),
        vec!["LAV003".to_owned()],
        "a waiver about a different tree".to_owned(),
    );
    let module = analyze_module_with(&case.path, &case.source, std::slice::from_ref(&waiver))
        .expect("fixture analyses");

    assert_eq!(module.findings().len(), 1);
    assert!(
        module
            .suppressions()
            .iter()
            .all(|record| record.origin().pattern().is_none())
    );
}

/// When both forms cover the same finding the inline one is credited: it is
/// the narrower waiver and the one a reviewer can attribute to a person.
/// Crediting the glob would report the broad waiver as load-bearing and the
/// specific one as dead, which is exactly backwards.
#[test]
fn an_inline_waiver_outranks_a_configured_one_for_the_same_finding() {
    let case = load_corpus()
        .into_iter()
        .find(|case| case.label == "waived_in_place.py")
        .expect("fixture present");

    let waiver = PathWaiver::new(
        "**/*.py".to_owned(),
        vec!["LAV003".to_owned()],
        "a blanket waiver over the whole tree".to_owned(),
    );
    let module = analyze_module_with(&case.path, &case.source, std::slice::from_ref(&waiver))
        .expect("fixture analyses");

    let inline = module
        .suppressions()
        .iter()
        .find(|record| record.origin().line().is_some())
        .expect("an inline record");
    let configured = module
        .suppressions()
        .iter()
        .find(|record| record.origin().pattern().is_some())
        .expect("a configured record");

    assert_eq!(
        inline.suppressed(),
        1,
        "the inline waiver takes the finding"
    );
    assert_eq!(inline.status(), SuppressionStatus::Applied);
    assert_eq!(configured.suppressed(), 0);
    assert_eq!(configured.status(), SuppressionStatus::Unused);
}

/// Every fixture is valid Python and every waived line still parses. A fixture
/// that does not analyse tests nothing, and `analyse` would have panicked, so
/// this asserts the weaker property the harness itself depends on.
#[test]
fn every_fixture_parses() {
    for case in load_corpus() {
        assert!(
            analyze_module(&case.path, &case.source).is_ok(),
            "{}",
            case.label
        );
    }
}
