//! `LAN-65` acceptance criteria 2 and 3, over the fixture corpus.
//!
//! * **AC 2** — every rule has positive *and* negative fixtures, every
//!   positive one fires, and **no negative one fires at all**.
//! * **AC 3** — every finding carries a file, a line, a column and a one-line
//!   explanation, and the line and column are asserted exactly.
//!
//! # Why the negative direction is the stricter half
//!
//! A negative fixture asserts zero findings from *any* rule, not merely from
//! the rule whose directory it sits in. The negative tree is therefore a
//! shared false-positive suite: a new rule that fires on `"".join(...)`, on
//! `stack.pop()`, or on `try:`/`finally:` fails here even though none of those
//! files was written with that rule in mind. A rule that fires on every
//! occurrence of a pattern gets switched off within a week, so this is the
//! assertion that decides whether the rule set survives contact with a real
//! repository.

// See `common/mod.rs` for why the panic lints are relaxed in test code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Fixture, RuleFixtures, load_corpus};

/// The corpus itself has to be well formed before anything it asserts means
/// something.
#[test]
fn corpus_is_well_formed() {
    let corpus = load_corpus();
    let mut problems = Vec::new();

    for rule in &corpus {
        if !common::is_rule_code(&rule.code) {
            problems.push(format!(
                "{}: `{}` is not a LAVnnn code",
                rule.directory_name, rule.code
            ));
        }
        if rule.slug.is_empty() {
            problems.push(format!(
                "{}: no rule name after the code",
                rule.directory_name
            ));
        }
        if rule.positive.is_empty() {
            problems.push(format!("{}: no positive fixtures", rule.directory_name));
        }
        if rule.negative.len() < 2 {
            problems.push(format!(
                "{}: {} negative fixtures; a rule needs at least two that would trip a naive \
                 implementation",
                rule.directory_name,
                rule.negative.len()
            ));
        }

        for fixture in &rule.positive {
            if fixture.expectations.is_empty() {
                problems.push(format!(
                    "{}: positive fixture asserts nothing",
                    fixture.label
                ));
            }
            if !fixture
                .expectations
                .iter()
                .any(|marker| marker.code == rule.code)
            {
                problems.push(format!(
                    "{}: no marker for `{}`, the rule this directory is for",
                    fixture.label, rule.code
                ));
            }
            for marker in &fixture.expectations {
                if marker.line > fixture.line_count() {
                    problems.push(format!("{}: marker past end of file", fixture.label));
                }
            }
        }

        for fixture in &rule.negative {
            if !fixture.expectations.is_empty() {
                problems.push(format!(
                    "{}: a negative fixture must contain no `# LANDAV:` marker, found {}",
                    fixture.label,
                    fixture.expectations.len()
                ));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "malformed fixture corpus:\n  {}",
        problems.join("\n  ")
    );
}

/// Every positive fixture fires its rule, at exactly the marked line and
/// column, and nowhere else.
#[test]
fn positive_fixtures_fire_at_the_marked_position() {
    let corpus = load_corpus();
    let mut failures = Vec::new();

    for rule in &corpus {
        for fixture in &rule.positive {
            let expected = fixture.expected();
            let interesting = codes_of_interest(rule, fixture);
            let observed: Vec<(String, u32, u32)> = fixture
                .observed()
                .into_iter()
                .filter(|(code, _, _)| interesting.iter().any(|wanted| wanted == code))
                .collect();

            if observed != expected {
                failures.push(format!(
                    "{}\n      expected {}\n      observed {}",
                    fixture.label,
                    render(&expected),
                    render(&observed)
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} positive fixture(s) did not report their rule at the marked position:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// No negative fixture produces a finding from any rule.
#[test]
fn negative_fixtures_produce_no_findings() {
    let corpus = load_corpus();
    let mut failures = Vec::new();

    for rule in &corpus {
        for fixture in &rule.negative {
            let observed = fixture.observed();
            if !observed.is_empty() {
                failures.push(format!(
                    "{}\n      false positive(s): {}",
                    fixture.label,
                    render(&observed)
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} negative fixture(s) produced a finding; each one is a false positive on idiomatic \
         Python:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// Findings carry a file, a plausible line and a plausible column.
#[test]
fn findings_carry_file_line_and_column() {
    let mut failures = Vec::new();

    for fixture in every_fixture() {
        let lines = fixture.line_count();
        for finding in fixture.analyse() {
            let location = finding.location();
            if location.file() != fixture.path {
                failures.push(format!(
                    "{}: finding names `{}`, not the file it was found in",
                    fixture.label,
                    location.file().display()
                ));
            }
            if location.line() == 0 || location.line() > lines {
                failures.push(format!(
                    "{}: line {} outside 1..={lines}",
                    fixture.label,
                    location.line()
                ));
            }
            if location.column() == 0 {
                failures.push(format!("{}: column 0; columns are 1-based", fixture.label));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "positions are wrong:\n  {}",
        failures.join("\n  ")
    );
}

/// The explanation is one non-empty line, short enough to sit beside the
/// position, and it is not just the rule code again.
#[test]
fn explanations_are_one_useful_line() {
    let mut failures = Vec::new();

    for fixture in every_fixture() {
        for finding in fixture.analyse() {
            let explanation = finding.explanation();
            let code = finding.rule().as_str();
            if explanation.trim().is_empty() {
                failures.push(format!(
                    "{}: {code} has an empty explanation",
                    fixture.label
                ));
            }
            if explanation.contains('\n') {
                failures.push(format!(
                    "{}: {code} explanation is not one line",
                    fixture.label
                ));
            }
            if explanation.chars().count() > 160 {
                failures.push(format!(
                    "{}: {code} explanation over 160 chars",
                    fixture.label
                ));
            }
            if explanation.trim() == code {
                failures.push(format!(
                    "{}: {code} explanation restates the code and explains nothing",
                    fixture.label
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "explanations are unusable:\n  {}",
        failures.join("\n  ")
    );
}

/// Two runs over identical bytes must produce identical output, and the order
/// is part of the contract rather than an accident of rule declaration order.
#[test]
fn findings_are_ordered_and_reproducible() {
    let mut failures = Vec::new();

    for fixture in every_fixture() {
        let first = fixture.analyse();
        let second = fixture.analyse();
        if first != second {
            failures.push(format!("{}: two runs disagreed", fixture.label));
        }

        let keys: Vec<(u32, u32, &str)> = first
            .iter()
            .map(|finding| {
                (
                    finding.location().line(),
                    finding.location().column(),
                    finding.rule().as_str(),
                )
            })
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        if keys != sorted {
            failures.push(format!(
                "{}: findings are not sorted by (line, column, code): {keys:?}",
                fixture.label
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "output is not deterministic:\n  {}",
        failures.join("\n  ")
    );
}

/// Every code a positive fixture has an opinion about: the rule the directory
/// is for, plus any rule an explicit marker names.
///
/// Findings from *other* rules are tolerated on positive fixtures — a genuine
/// defect often trips more than one pattern — but they are not tolerated on
/// negative fixtures, which is where the false-positive budget is spent.
fn codes_of_interest(rule: &RuleFixtures, fixture: &Fixture) -> Vec<String> {
    let mut codes = vec![rule.code.clone()];
    for marker in &fixture.expectations {
        if !codes.contains(&marker.code) {
            codes.push(marker.code.clone());
        }
    }
    codes
}

fn every_fixture() -> Vec<Fixture> {
    load_corpus()
        .into_iter()
        .flat_map(|rule| rule.positive.into_iter().chain(rule.negative))
        .collect()
}

fn render(entries: &[(String, u32, u32)]) -> String {
    if entries.is_empty() {
        return "nothing".to_owned();
    }
    entries
        .iter()
        .map(|(code, line, column)| format!("{code}@{line}:{column}"))
        .collect::<Vec<_>>()
        .join(", ")
}
