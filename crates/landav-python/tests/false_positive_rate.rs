//! `LAN-65` acceptance criterion 4: a false-positive rate under 5%.
//!
//! # What this harness can and cannot claim, and the difference matters
//!
//! AC 4 says "under 5% on the eval corpus". **The eval corpus is an R1
//! deliverable and does not exist at R0.** Nothing in this repository can
//! measure the number AC 4 is about, and this file does not pretend otherwise.
//!
//! What it does:
//!
//! * If `LANDAV_EVAL_CORPUS` names a directory, every `.py` file beneath it is
//!   treated as known-clean and the measured rate is asserted against
//!   [`MAX_FALSE_POSITIVE_RATE`]. That is the real measurement, and it becomes
//!   available the day the corpus lands — no code change, one environment
//!   variable.
//! * Otherwise it falls back to the negative fixture tree. Those files are
//!   known-clean by construction, so the rate is meaningful, but they were
//!   written by the same person who specified the rules and they are chosen to
//!   be hard. A corpus you wrote to trip yourself up is a **lower bound on
//!   difficulty and an upper bound on confidence**: passing here says the
//!   obvious false positives are gone, not that the rate on real code is under
//!   5%.
//!
//! **Consequence for release notes:** at R0 the 5% figure is *unverified*. It
//! should be stated as an open target with the harness named, not claimed as a
//! measured property. Claiming a measured number from this fallback corpus
//! would be the same category of error as reporting a bound the code can
//! exceed — a number that reads as evidence and is not.

// See `common/mod.rs` for why the panic lints are relaxed in test code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::{fs, path::PathBuf};

use landav_python::analyze_source;

use common::{collect_python_files, fixtures_root, load_corpus};

/// The ceiling from AC 4, as a fraction of files.
///
/// Per *file* rather than per *line*: a rule that fires eleven times in one
/// unusual file is one bad file, whereas per-line accounting would let a
/// single pathological file swamp the measurement in either direction.
pub const MAX_FALSE_POSITIVE_RATE: f64 = 0.05;

/// The environment variable that points the harness at the real corpus.
pub const EVAL_CORPUS_ENV: &str = "LANDAV_EVAL_CORPUS";

#[test]
fn false_positive_rate_is_under_the_ceiling() {
    let (label, files) = corpus();
    assert!(
        !files.is_empty(),
        "{label}: no Python files to measure against"
    );

    let mut dirty = Vec::new();
    let mut unparsed = Vec::new();

    for path in &files {
        let Ok(source) = fs::read_to_string(path) else {
            panic!("{}: unreadable", path.display());
        };
        match analyze_source(path, &source) {
            Ok(findings) if findings.is_empty() => {}
            Ok(findings) => {
                let codes: Vec<&str> = findings
                    .iter()
                    .map(|finding| finding.rule().as_str())
                    .collect();
                dirty.push(format!("{} [{}]", path.display(), codes.join(", ")));
            }
            // A file the frontend cannot parse is not a false positive; it is a
            // coverage gap, and conflating the two would let a parser that
            // rejects half the corpus report a flattering rate.
            Err(error) => unparsed.push(format!("{error}")),
        }
    }

    let measured = dirty.len() as f64 / files.len() as f64;

    assert!(
        measured < MAX_FALSE_POSITIVE_RATE,
        "{label}: false-positive rate {:.2}% over {} file(s) exceeds the {:.0}% ceiling\n  {}\n\
         (unparsed: {})",
        measured * 100.0,
        files.len(),
        MAX_FALSE_POSITIVE_RATE * 100.0,
        dirty.join("\n  "),
        unparsed.len()
    );
}

/// The fallback corpus must actually contain something to measure, or the
/// assertion above is vacuous and would stay green through any regression.
#[test]
fn the_fallback_corpus_is_not_empty() {
    let negatives: usize = load_corpus().iter().map(|rule| rule.negative.len()).sum();
    assert!(
        negatives >= 20,
        "the negative fixture tree is the R0 stand-in for the eval corpus; {negatives} files is \
         too few for the measured rate to mean anything"
    );
}

/// Every file the harness measures, and a label naming which corpus it is.
fn corpus() -> (String, Vec<PathBuf>) {
    match std::env::var(EVAL_CORPUS_ENV) {
        Ok(configured) if !configured.trim().is_empty() => {
            let root = PathBuf::from(configured.trim());
            assert!(
                root.is_dir(),
                "{EVAL_CORPUS_ENV} is set to `{}`, which is not a directory",
                root.display()
            );
            (
                format!("eval corpus at {}", root.display()),
                collect_python_files(&root),
            )
        }
        _ => {
            let mut files = Vec::new();
            for entry in collect_python_files(&fixtures_root()) {
                if entry
                    .components()
                    .any(|component| component.as_os_str() == "negative")
                {
                    files.push(entry);
                }
            }
            (
                "R0 fallback corpus (negative fixtures; NOT the eval corpus)".to_owned(),
                files,
            )
        }
    }
}
