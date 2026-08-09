//! `LAN-65` acceptance criterion 1: at least eight rules, each with a code and
//! documentation.
//!
//! The registry and the fixture corpus are asserted to be the same set. That
//! is what stops the two halves of the criterion drifting apart: a rule can no
//! longer be documented but untested, or tested but undocumented, and the
//! failure names which of the two happened.

// See `common/mod.rs` for why the panic lints are relaxed in test code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeSet;

use landav_python::{MINIMUM_RULE_COUNT, registry, rule_for_code};

use common::load_corpus;

#[test]
fn at_least_eight_rules_are_registered() {
    let count = registry().len();
    assert!(
        count >= MINIMUM_RULE_COUNT,
        "F-005 ships at least {MINIMUM_RULE_COUNT} rules; the registry declares {count}"
    );
}

#[test]
fn codes_are_well_formed_and_distinct() {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();

    for rule in registry() {
        let code = rule.code().as_str();
        if !common::is_rule_code(code) {
            problems.push(format!("`{code}` is not `LAV` plus three digits"));
        }
        if !seen.insert(code) {
            problems.push(format!(
                "`{code}` is declared twice; codes are permanent identifiers"
            ));
        }
        if rule_for_code(code) != Some(rule) {
            problems.push(format!("`{code}` is not reachable through rule_for_code"));
        }
    }

    assert!(
        problems.is_empty(),
        "rule codes are unusable:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn names_are_kebab_case_and_distinct() {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();

    for rule in registry() {
        let name = rule.name();
        let shaped = !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
            && !name.starts_with('-')
            && !name.ends_with('-');
        if !shaped {
            problems.push(format!("`{name}` is not a kebab-case rule name"));
        }
        if !seen.insert(name) {
            problems.push(format!("`{name}` is declared twice"));
        }
    }

    assert!(
        problems.is_empty(),
        "rule names are unusable:\n  {}",
        problems.join("\n  ")
    );
}

/// Documentation is output, not decoration: `landav explain LAV003` prints it.
/// An empty or one-word entry satisfies "has documentation" on paper and fails
/// the person reading the CI log, so the shape is asserted.
#[test]
fn every_rule_has_documentation() {
    let mut problems = Vec::new();

    for rule in registry() {
        let code = rule.code().as_str();
        let summary = rule.summary();
        let documentation = rule.documentation();

        if summary.trim().is_empty() {
            problems.push(format!("{code}: empty summary"));
        }
        if summary.contains('\n') {
            problems.push(format!("{code}: summary must be a single line"));
        }
        if summary.chars().count() > 100 {
            problems.push(format!("{code}: summary over 100 chars"));
        }
        if documentation.trim().is_empty() {
            problems.push(format!("{code}: empty documentation"));
        }
        if documentation.trim().len() < 120 {
            problems.push(format!(
                "{code}: documentation is {} bytes; it has to say why the pattern is superlinear \
                 and what to write instead",
                documentation.trim().len()
            ));
        }
        if documentation.trim() == summary.trim() {
            problems.push(format!("{code}: documentation just repeats the summary"));
        }
    }

    assert!(
        problems.is_empty(),
        "rule documentation is unusable:\n  {}",
        problems.join("\n  ")
    );
}

/// Every registered rule has a fixture directory, in both directions.
#[test]
fn every_registered_rule_has_fixtures() {
    let corpus = load_corpus();
    let mut problems = Vec::new();

    for rule in registry() {
        let expected_directory =
            format!("{}_{}", rule.code().as_str(), rule.name().replace('-', "_"));
        let found = corpus
            .iter()
            .find(|entry| entry.directory_name == expected_directory);
        match found {
            None => problems.push(format!(
                "{}: no fixture directory `tests/fixtures/{expected_directory}/`",
                rule.code()
            )),
            Some(entry) => {
                if entry.positive.is_empty() {
                    problems.push(format!("{}: no positive fixtures", rule.code()));
                }
                if entry.negative.len() < 2 {
                    problems.push(format!("{}: fewer than two negative fixtures", rule.code()));
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "registry and corpus disagree:\n  {}",
        problems.join("\n  ")
    );
}

/// Every fixture directory has a registered rule.
///
/// This is the direction that catches the interesting failure: a corpus entry
/// written for a rule that was specified and then quietly dropped.
#[test]
fn every_fixture_directory_has_a_registered_rule() {
    let corpus = load_corpus();
    let mut problems = Vec::new();

    for entry in &corpus {
        match rule_for_code(&entry.code) {
            None => problems.push(format!(
                "tests/fixtures/{}/ has no rule `{}` in the registry",
                entry.directory_name, entry.code
            )),
            Some(rule) => {
                let expected =
                    format!("{}_{}", rule.code().as_str(), rule.name().replace('-', "_"));
                if expected != entry.directory_name {
                    problems.push(format!(
                        "tests/fixtures/{}/ does not match rule name `{}` (expected `{expected}`)",
                        entry.directory_name,
                        rule.name()
                    ));
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "corpus and registry disagree:\n  {}",
        problems.join("\n  ")
    );
}
