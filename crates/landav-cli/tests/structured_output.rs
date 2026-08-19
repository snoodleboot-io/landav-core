//! `LAN-84` at the process boundary: **a program must be able to consume a run
//! without parsing prose.**
//!
//! # What is being defended
//!
//! Both of landav's stated consumers are machines — a CI gate and an agent
//! deciding what to change. Everything a run concludes already exists in
//! structured form internally; the failure this suite guards is that it gets
//! flattened to English on the way out and cannot be recovered.
//!
//! # This output is a contract
//!
//! Anything emitted is something a consumer will depend on, so the assertions
//! here are deliberately about *shape and meaning* rather than about exact
//! numbers. A test that pins `"bound": "(2 + (2 * n))"` would fail whenever the
//! bound algebra's normal form changes, which is allowed; a test that pins
//! "there is a bound, and it is marked exact" fails only when something real
//! breaks.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use common::{EXIT_CLEAN, Project};
use serde_json::Value;

/// One function of each shape the reporting distinguishes.
const SHAPES_PY: &str = r"
def only_counted(n: int) -> int:
    x = 0
    for i in range(n):
        x = i
    return x


def mixed(n: int) -> int:
    x = 0
    for i in range(n):
        x = i
    while n > 0:
        n = n - 1
    return x


def refused(items: list, n: int) -> int:
    total = 0
    for i in range(n):
        total = total + sorted(items)[i]
    return total
";

fn run_json(project: &Project, source: &str) -> io::Result<Value> {
    let target = project.write("shapes.py", source)?;
    let run = project.check(&target, &["--json"])?;
    Ok(serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not valid JSON ({why}): {}", run.describe())))
}

fn function<'a>(run: &'a Value, name: &str) -> &'a Value {
    run["functions"]
        .as_array()
        .expect("functions is an array")
        .iter()
        .find(|f| f["name"] == name)
        .unwrap_or_else(|| panic!("no function named {name} in {run}"))
}

/// The flag is opt-in. A person at a terminal must not get a wall of JSON for
/// asking a simple question.
#[test]
fn text_is_unchanged_when_json_is_not_asked_for() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("shapes.py", SHAPES_PY)?;

    let run = project.check(&target, &[])?;
    assert!(
        serde_json::from_str::<Value>(&run.stdout).is_err(),
        "a run without --json emitted JSON: {}",
        run.describe()
    );
    assert!(
        run.mentions("landav:"),
        "the prose summary must still be there: {}",
        run.describe()
    );
    Ok(())
}

/// Nothing but JSON on stdout, so `landav check . --json | jq` needs no
/// filtering. Diagnostics belong on stderr.
#[test]
fn stdout_carries_json_and_nothing_else() -> io::Result<()> {
    let project = Project::new()?;
    let _ = run_json(&project, SHAPES_PY)?;
    Ok(())
}

/// The schema is versioned from the first release. A consumer that breaks
/// silently across an upgrade is worse than one that never worked.
#[test]
fn the_schema_declares_its_version() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, SHAPES_PY)?;
    assert_eq!(
        run["schema_version"], 1,
        "the schema version changed; that is a decision, not an accident, and \
         consumers need to be told"
    );
    Ok(())
}

/// The verdict is in the JSON *and* in the exit code, so a gate can branch on
/// either without having to parse the other.
#[test]
fn the_verdict_is_carried_in_both_places() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("shapes.py", SHAPES_PY)?;
    let run = project.check(&target, &["--json"])?;
    let parsed: Value = serde_json::from_str(&run.stdout).expect("valid JSON");

    assert_eq!(run.code, EXIT_CLEAN);
    assert_eq!(
        parsed["outcome"],
        "clean",
        "the JSON must report the same verdict the exit code does: {}",
        run.describe()
    );
    Ok(())
}

/// The distinction the whole engine exists to make, machine-readable.
#[test]
fn an_exact_bound_is_marked_exact() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, SHAPES_PY)?;
    let f = function(&run, "only_counted");

    assert_eq!(f["lowered"], true);
    assert_eq!(f["bound_kind"], "exact");
    assert!(
        f["bound"].is_string(),
        "an exact result must carry its bound: {f}"
    );
    assert_eq!(
        f["holes"].as_array().map(Vec::len),
        Some(0),
        "an exact result has no unanalysed regions: {f}"
    );
    Ok(())
}

/// A partial bound is distinguishable from an exact one, and says which
/// regions it could not derive and where. This is what an agent acts on.
#[test]
fn a_partial_bound_names_the_regions_it_could_not_derive() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, SHAPES_PY)?;
    let f = function(&run, "mixed");

    assert_eq!(f["bound_kind"], "partial");
    assert!(
        f["exact_outside_holes"] == true,
        "the counted half was exact, and a consumer needs that to distinguish \
         'exact except for that while' from 'approximate as well': {f}"
    );

    let holes = f["holes"].as_array().expect("holes is an array");
    assert_eq!(holes.len(), 1, "one `while` is one region: {f}");
    assert_eq!(holes[0]["construct"], "while");
    assert!(
        holes[0]["origin"]
            .as_str()
            .unwrap_or_default()
            .contains(':'),
        "a hole must be placed, not merely named: {f}"
    );

    // The hole's variable appears in the bound, so the two can be connected
    // without guessing.
    let bound = f["bound"].as_str().unwrap_or_default();
    let variable = holes[0]["variable"].as_str().unwrap_or_default();
    assert!(
        bound.contains(variable),
        "the bound {bound:?} must mention the hole {variable:?}"
    );
    Ok(())
}

/// The case that matters most at today's coverage: a function that did not
/// lower must say *why*, with positions, so an agent can act rather than
/// merely learn that it failed.
#[test]
fn a_refused_function_reports_every_construct_and_where() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, SHAPES_PY)?;
    let f = function(&run, "refused");

    assert_eq!(f["lowered"], false);
    assert!(
        f["bound"].is_null(),
        "a function that did not lower has no bound, and reporting one would \
         be inventing a conclusion: {f}"
    );

    let refused = f["refused"].as_array().expect("refused is an array");
    assert!(!refused.is_empty(), "a refusal must say why: {f}");
    for entry in refused {
        assert!(
            entry["construct"].is_string() && !entry["construct"].as_str().unwrap().is_empty(),
            "every refusal names a construct: {entry}"
        );
        assert!(
            entry["describes"].is_string(),
            "the construct carries its meaning, so a consumer need not hold a \
             table of codes: {entry}"
        );
        assert!(
            entry["origin"].as_str().unwrap_or_default().contains(':'),
            "every refusal is placed: {entry}"
        );
    }
    Ok(())
}

/// Constructs are named by behaviour, never by an opaque code. The agent's
/// transcript is read by a human, and a name survives that reading.
#[test]
fn constructs_are_named_rather_than_coded() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, SHAPES_PY)?;
    let f = function(&run, "refused");
    let names: Vec<String> = f["refused"]
        .as_array()
        .expect("array")
        .iter()
        .map(|r| r["construct"].as_str().unwrap_or_default().to_owned())
        .collect();

    assert!(
        names
            .iter()
            .any(|n| n.contains('-') || n.chars().all(|c| c.is_ascii_lowercase())),
        "constructs should read as words, got {names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|n| n.starts_with("LAV") || n.chars().next().is_some_and(char::is_numeric)),
        "a construct must not be an opaque code: {names:?}"
    );
    Ok(())
}

/// Every function appears, including the ones that failed. A consumer that
/// only ever sees successes will read silence as cleanliness — the same
/// argument `LAN-68` makes for the coverage report.
#[test]
fn every_function_appears_including_the_failures() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, SHAPES_PY)?;
    for name in ["only_counted", "mixed", "refused"] {
        let _ = function(&run, name);
    }
    assert_eq!(
        run["summary"]["functions"], 3,
        "the summary count must match what is listed: {}",
        run["summary"]
    );
    Ok(())
}

/// The counts a gate is likely to threshold on, present and consistent.
#[test]
fn the_summary_supports_gating() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, SHAPES_PY)?;
    let summary = &run["summary"];

    for field in [
        "files_analysed",
        "statements",
        "functions",
        "lowered",
        "refusals",
        "findings",
        "suppressed",
        "stale_waivers",
    ] {
        assert!(
            summary[field].is_u64(),
            "`{field}` must be a number a gate can compare: {summary}"
        );
    }
    assert_eq!(summary["lowered"], 2, "two of the three lower: {summary}");
    assert_eq!(summary["coverage_percent"], 66, "2 of 3: {summary}");
    Ok(())
}

/// A run with nothing to analyse reports zero coverage as *absent*, not as
/// zero percent. A gate thresholding on a percentage must not read "no
/// functions" as "no coverage" and fail a directory that legitimately has
/// none.
#[test]
fn coverage_is_null_rather_than_zero_when_there_is_nothing_to_cover() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, "x = 1\n")?;
    assert!(
        run["summary"]["coverage_percent"].is_null(),
        "no functions is not zero percent: {}",
        run["summary"]
    );
    Ok(())
}

/// `problems` is present even when empty, so a consumer cannot mistake a
/// missing key for an absence of trouble.
#[test]
fn problems_is_always_present() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, SHAPES_PY)?;
    assert!(
        run["problems"].is_array(),
        "problems must always be an array: {run}"
    );
    Ok(())
}
