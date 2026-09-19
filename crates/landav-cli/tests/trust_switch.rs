//! `LAN-106`: **`--trust` makes the premise something a reader can act on.**
//!
//! Publishing a premise says which bounds rest on a user `__len__` agreeing
//! with a user `__iter__`. It does not let anyone *check* what that trust
//! bought, and a reader who cannot accept it is left taking the tool's word
//! for which numbers it touched.
//!
//! So the premise is also a switch. `--trust concrete` reads a length only from
//! a concrete builtin, where CPython guarantees that iterating yields exactly
//! `len` items. Run a tree both ways and the results that differ are exactly
//! the ones the weaker premise bought - no argument required.
//!
//! # Why the setting is recorded in the output
//!
//! A comparison of two runs is worthless if you cannot tell which is which.
//! `summary.trust` carries it in the JSON always, and the text says so when the
//! trust was narrowed - the default stays silent, so an ordinary run reads as
//! it always has.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use serde_json::Value;

use common::Project;

const SOURCE: &str = "from collections.abc import Sequence\n\n\
def on_a_protocol(rows: Sequence[str]) -> int:\n\
\x20   total = 0\n\
\x20   for row in rows:\n\
\x20       total += 1\n\
\x20   return total\n\n\
def on_a_builtin(items: list) -> int:\n\
\x20   total = 0\n\
\x20   for item in items:\n\
\x20       total += 1\n\
\x20   return total\n";

fn run_with(project: &Project, trust: &str) -> io::Result<Value> {
    let target = project.write("trust.py", SOURCE)?;
    let run = project.check(&target, &["--json", "--trust", trust])?;
    Ok(serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not valid JSON ({why}): {}", run.describe())))
}

fn bound_of<'a>(parsed: &'a Value, name: &str) -> &'a Value {
    parsed["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("no function {name}"))
}

/// **The two runs differ exactly where the weaker premise was used.**
///
/// The whole point, in one test. The protocol-annotated function loses its
/// bound under `concrete`; the builtin-annotated one beside it does not move,
/// which is what makes the difference attributable rather than coincidental.
#[test]
fn narrowing_the_trust_moves_exactly_the_results_that_rested_on_it() -> io::Result<()> {
    let project = Project::new()?;
    let trusting = run_with(&project, "protocol")?;
    let strict = run_with(&project, "concrete")?;

    assert_eq!(
        bound_of(&trusting, "on_a_protocol")["bound_kind"],
        "exact",
        "a `Sequence` is trusted by default: {trusting}"
    );
    assert_eq!(
        bound_of(&strict, "on_a_protocol")["bound_kind"],
        "partial",
        "under `concrete` the length is not read at all, so the loop is a region \
         and no complete claim is made: {strict}"
    );
    assert_eq!(
        bound_of(&trusting, "on_a_builtin")["bound"],
        bound_of(&strict, "on_a_builtin")["bound"],
        "a builtin annotation is unaffected by the setting - otherwise the diff \
         between two runs would not attribute anything"
    );
    Ok(())
}

/// **Narrowing removes the premise with the bound.** A premise that survived
/// the setting that disowns it would be a claim about a number nobody reports.
#[test]
fn narrowing_the_trust_removes_the_protocol_premise() -> io::Result<()> {
    let project = Project::new()?;
    let strict = run_with(&project, "concrete")?;
    let premises = bound_of(&strict, "on_a_protocol")["premises"]
        .as_array()
        .unwrap();
    assert!(
        premises.is_empty(),
        "nothing rests on the protocol here any more: {strict}"
    );
    Ok(())
}

/// **Each run says which trust it used.**
#[test]
fn the_run_records_the_trust_it_used() -> io::Result<()> {
    let project = Project::new()?;
    for trust in ["protocol", "concrete"] {
        assert_eq!(
            run_with(&project, trust)?["summary"]["trust"],
            trust,
            "a diff of two runs is worthless if neither says which it is"
        );
    }
    Ok(())
}

/// **The default is to trust, and it is silent about it.**
///
/// The decision was to admit the protocols, so a run with no flag behaves as
/// one with `--trust protocol`, and its summary line reads as it always has.
/// A run that narrowed the trust says so, because its numbers mean something
/// different.
#[test]
fn the_default_trusts_and_only_narrowing_is_announced() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("trust.py", SOURCE)?;

    let default = project.check(&target, &["--json"])?;
    let parsed: Value = serde_json::from_str(&default.stdout).unwrap();
    assert_eq!(
        parsed["summary"]["trust"],
        "protocol",
        "{}",
        default.describe()
    );

    let plain = project.check(&target, &[])?;
    assert!(
        !plain.mentions("trust:"),
        "the default must not add noise to every summary line: {}",
        plain.describe()
    );
    let narrowed = project.check(&target, &["--trust", "concrete"])?;
    assert!(
        narrowed.mentions("trust: concrete annotations only"),
        "a narrowed run has changed what its numbers mean and must say so: {}",
        narrowed.describe()
    );
    Ok(())
}

/// **An unknown value is refused, and the message says what the choice is for.**
#[test]
fn an_unknown_trust_is_a_usage_error() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("trust.py", SOURCE)?;
    let run = project.check(&target, &["--trust", "everything"])?;
    assert_eq!(
        run.code,
        common::EXIT_TOOL_ERROR,
        "an unrecognised setting is a usage error, not a verdict: {}",
        run.describe()
    );
    assert!(
        run.mentions("concrete") && run.mentions("protocol"),
        "the message must name the choices: {}",
        run.describe()
    );
    Ok(())
}
