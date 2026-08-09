//! Shared plumbing for the `LAN-65` fixture corpus.
//!
//! # The corpus layout
//!
//! ```text
//! tests/fixtures/
//!   LAV003_string_concat_in_loop/
//!     positive/*.py   <- the genuine defect; every one must fire
//!     negative/*.py   <- idiomatic Python that resembles it; none may fire
//! ```
//!
//! The directory name is `{code}_{rule name with '-' as '_'}`, which is what
//! ties the corpus to [`landav_python::registry`]. A rule with no fixture
//! directory, or a fixture directory with no rule, is a test failure — see
//! `tests/rule_registry.rs`.
//!
//! # How an expectation is written
//!
//! A positive fixture marks each expected finding with a trailing comment on
//! the line the finding must be reported at:
//!
//! ```python
//! out += format_row(row)  # LANDAV: LAV003 anchor=out += format_row(row)
//! ```
//!
//! * the **line** is the line the marker itself is on;
//! * the **column** is derived from `anchor`: the 1-based UTF-8 byte offset of
//!   the anchor text within the part of the line *before* the marker. The
//!   anchor must occur exactly once there, and the harness fails the fixture
//!   if it does not — an ambiguous anchor is a fixture bug, not a rule bug.
//!
//! Deriving the column rather than writing a number keeps the expectation
//! readable and survives re-indentation, while still asserting an exact
//! column: a rule that reports the enclosing `for` instead of the offending
//! expression fails, which is the point.
//!
//! A negative fixture must contain **no** markers at all. That is checked, so
//! a positive fixture filed into `negative/` cannot pass by accident.

#![allow(dead_code)]
// CONTRIBUTING non-negotiable 2 ("never panic") governs *library* code: a
// panicking analyser destroys the blame path that makes a partial bound
// useful. A test that cannot panic cannot fail, so the workspace lints are
// relaxed here and only here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    fs,
    path::{Path, PathBuf},
};

use landav_python::{Finding, analyze_source};

/// The token that introduces an expectation comment in a fixture.
pub const MARKER: &str = "# LANDAV:";

/// The separator between the rule code and the anchor text.
pub const ANCHOR_KEY: &str = "anchor=";

/// One finding a positive fixture asserts must be produced.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Expectation {
    /// 1-based line, taken from the line the marker sits on.
    pub line: u32,
    /// 1-based UTF-8 byte column, derived from the anchor.
    pub column: u32,
    /// The rule code that must fire here.
    pub code: String,
    /// The source text the column points at, kept for failure messages.
    pub anchor: String,
}

/// One fixture file plus whatever it asserts.
#[derive(Debug)]
pub struct Fixture {
    /// Absolute path, as handed to the analyser.
    pub path: PathBuf,
    /// `LAV003_string_concat_in_loop/positive/report_accumulator.py`, for
    /// failure messages.
    pub label: String,
    /// The file's contents.
    pub source: String,
    /// Empty for every negative fixture, non-empty for every positive one.
    pub expectations: Vec<Expectation>,
}

impl Fixture {
    /// Runs the analyser over this fixture.
    ///
    /// A fixture that does not parse is a broken fixture, so a parse failure
    /// is surfaced as a test failure rather than as an empty result.
    pub fn analyse(&self) -> Vec<Finding> {
        match analyze_source(&self.path, &self.source) {
            Ok(findings) => findings,
            Err(error) => panic!("{}: fixture failed to analyse: {error}", self.label),
        }
    }

    /// `(code, line, column)` for every finding, sorted.
    pub fn observed(&self) -> Vec<(String, u32, u32)> {
        let mut observed: Vec<(String, u32, u32)> = self
            .analyse()
            .iter()
            .map(|finding| {
                (
                    finding.rule().as_str().to_owned(),
                    finding.location().line(),
                    finding.location().column(),
                )
            })
            .collect();
        observed.sort();
        observed
    }

    /// `(code, line, column)` for every expectation, sorted.
    pub fn expected(&self) -> Vec<(String, u32, u32)> {
        let mut expected: Vec<(String, u32, u32)> = self
            .expectations
            .iter()
            .map(|expectation| {
                (
                    expectation.code.clone(),
                    expectation.line,
                    expectation.column,
                )
            })
            .collect();
        expected.sort();
        expected
    }

    /// The number of physical lines, for range assertions.
    pub fn line_count(&self) -> u32 {
        u32::try_from(self.source.lines().count()).expect("fixture is not millions of lines")
    }
}

/// Every fixture belonging to one rule.
#[derive(Debug)]
pub struct RuleFixtures {
    /// `LAV003`.
    pub code: String,
    /// `string_concat_in_loop`.
    pub slug: String,
    /// `LAV003_string_concat_in_loop`.
    pub directory_name: String,
    /// The genuine defects.
    pub positive: Vec<Fixture>,
    /// Idiomatic code that resembles the defect.
    pub negative: Vec<Fixture>,
}

/// The root of the fixture corpus.
pub fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Loads the whole corpus, in ascending rule-code order.
pub fn load_corpus() -> Vec<RuleFixtures> {
    let root = fixtures_root();
    let mut directories: Vec<PathBuf> = read_dir_sorted(&root)
        .into_iter()
        .filter(|entry| entry.is_dir())
        .collect();
    directories.sort();

    assert!(
        !directories.is_empty(),
        "no fixture directories under {}",
        root.display()
    );

    directories
        .iter()
        .map(|directory| {
            let directory_name = file_name(directory);
            let (code, slug) = directory_name.split_once('_').unwrap_or_else(|| {
                panic!("fixture directory `{directory_name}` is not `{{code}}_{{name}}`")
            });
            RuleFixtures {
                code: code.to_owned(),
                slug: slug.to_owned(),
                directory_name: directory_name.clone(),
                positive: load_side(directory, "positive"),
                negative: load_side(directory, "negative"),
            }
        })
        .collect()
}

/// Collects every `.py` file under `root`, recursively, in sorted order.
///
/// Used by the false-positive harness so that an external eval corpus can have
/// any directory shape it likes.
pub fn collect_python_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in read_dir_sorted(&current) {
            if entry.is_dir() {
                stack.push(entry);
            } else if entry.extension().is_some_and(|extension| extension == "py") {
                found.push(entry);
            }
        }
    }
    found.sort();
    found
}

fn load_side(directory: &Path, side: &str) -> Vec<Fixture> {
    let side_directory = directory.join(side);
    assert!(
        side_directory.is_dir(),
        "{} has no `{side}/` directory: every rule needs both directions",
        file_name(directory)
    );

    read_dir_sorted(&side_directory)
        .into_iter()
        .filter(|entry| entry.extension().is_some_and(|extension| extension == "py"))
        .map(|path| {
            let label = format!("{}/{side}/{}", file_name(directory), file_name(&path));
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{label}: unreadable: {error}"));
            let expectations = parse_expectations(&label, &source);
            Fixture {
                path,
                label,
                source,
                expectations,
            }
        })
        .collect()
}

/// Extracts every `# LANDAV: <CODE> anchor=<text>` expectation from a fixture.
pub fn parse_expectations(label: &str, source: &str) -> Vec<Expectation> {
    let mut expectations = Vec::new();

    for (index, line) in source.lines().enumerate() {
        let Some(marker_at) = line.find(MARKER) else {
            continue;
        };
        let number = u32::try_from(index + 1).expect("fixture is not billions of lines");
        let code_part = &line[..marker_at];
        let directive = line[marker_at + MARKER.len()..].trim();

        let (code, anchor) = directive.split_once(ANCHOR_KEY).unwrap_or_else(|| {
            panic!("{label}:{number}: marker has no `{ANCHOR_KEY}`: `{directive}`")
        });
        let code = code.trim();
        let anchor = anchor.trim_end();

        assert!(!anchor.is_empty(), "{label}:{number}: empty anchor");
        assert!(
            is_rule_code(code),
            "{label}:{number}: `{code}` is not a `LAVnnn` rule code"
        );

        let occurrences = code_part.match_indices(anchor).count();
        assert_eq!(
            occurrences, 1,
            "{label}:{number}: anchor `{anchor}` occurs {occurrences} times before the marker; \
             an anchor must identify exactly one column"
        );
        let offset = code_part.find(anchor).expect("just counted one occurrence");
        let column = u32::try_from(offset + 1).expect("fixture lines are short");

        expectations.push(Expectation {
            line: number,
            column,
            code: code.to_owned(),
            anchor: anchor.to_owned(),
        });
    }

    expectations
}

/// `true` for `LAV` followed by exactly three ASCII digits.
pub fn is_rule_code(code: &str) -> bool {
    let Some(digits) = code.strip_prefix("LAV") else {
        return false;
    };
    digits.len() == 3 && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn read_dir_sorted(directory: &Path) -> Vec<PathBuf> {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()));
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|error| {
                    panic!("cannot read an entry of {}: {error}", directory.display())
                })
                .path()
        })
        .collect();
    paths.sort();
    paths
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| panic!("non-UTF-8 path {}", path.display()))
        .to_owned()
}
