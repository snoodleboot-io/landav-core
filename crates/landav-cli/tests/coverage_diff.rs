//! `LAN-119`: **`landav report` and `landav diff` read a run back and say what
//! it shows, the same way every time.**
//!
//! The acceptance is at the process boundary like every other subcommand's:
//! the binary reads JSON that `landav check --json` wrote (or JSON shaped like
//! it), and what it prints is what the measurements since `LAN-89` computed by
//! hand - functions by bound kind, holes by construct, sole blockers, the
//! regression line, and an explanation for every function that gained holes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use common::{EXIT_CLEAN, EXIT_TOOL_ERROR, Project};
use serde_json::{Value, json};

/// A run document in the machine shape, down to the fields `diff` reads.
fn run(functions: Vec<Value>) -> String {
    json!({
        "schema_version": 3,
        "outcome": "inconclusive",
        "summary": {},
        "functions": functions,
        "findings": [],
        "problems": []
    })
    .to_string()
}

fn function(name: &str, line: u32, kind: Option<&str>, holes: &[&str]) -> Value {
    json!({
        "name": name,
        "file": "w.py",
        "line": line,
        "column": 1,
        "lowered": false,
        "premises": [],
        "bound": kind.map(|_| "1"),
        "bound_kind": kind,
        "exact_outside_holes": true,
        "holes": holes.iter().map(|construct| json!({
            "variable": "#hole0",
            "construct": construct,
            "assumption": "callee-cost-unknown",
            "assumption_subject": null,
            "origin": "w.py:1:1"
        })).collect::<Vec<_>>(),
        "refused": []
    })
}

/// Two real runs over the same file at two commits' worth of difference:
/// the function at the same line goes from a `call` hole to exact.
#[test]
fn a_real_pair_of_runs_reports_the_improvement_and_the_regression() -> io::Result<()> {
    let project = Project::new()?;
    let file = project.write(
        "w.py",
        "def g(x, n: int) -> int:\n    for i in range(n):\n        f(x)\n    return 1\n",
    )?;
    let before = project.check(&file, &["--json"])?;
    project.write(
        "w.py",
        "def g(x, n: int) -> int:\n    for i in range(n):\n        pass\n    return 1\n",
    )?;
    let after = project.check(&file, &["--json"])?;
    let before_path = project.write("before.json", &before.stdout)?;
    let after_path = project.write("after.json", &after.stdout)?;

    let forward = project.run(&[
        "diff",
        before_path.to_str().unwrap(),
        after_path.to_str().unwrap(),
        "--json",
    ])?;
    assert_eq!(forward.code, EXIT_CLEAN, "{}", forward.describe());
    let delta: Value = serde_json::from_str(&forward.stdout).expect("a JSON delta");
    assert_eq!(delta["matched"], 1, "{}", forward.describe());
    assert_eq!(delta["improved"], 1, "{}", forward.describe());
    assert_eq!(delta["regressions"].as_array().unwrap().len(), 0);
    assert_eq!(delta["before"]["sole_blockers"]["call"], 1);
    assert_eq!(delta["after"]["by_kind"]["exact"], 1);

    let backward = project.run(&[
        "diff",
        after_path.to_str().unwrap(),
        before_path.to_str().unwrap(),
    ])?;
    assert_eq!(
        backward.code, EXIT_CLEAN,
        "a regression is reported, not exited on"
    );
    assert!(
        backward.stdout.contains("exact -> not exact: 1") && backward.stdout.contains("REGRESSION"),
        "the regression line comes first and names it:\n{}",
        backward.describe()
    );
    Ok(())
}

/// **Every function that gained holes is explained, in the order the
/// measurements need, and the unexplained one is listed in full.**
#[test]
fn a_gainer_is_classified() -> io::Result<()> {
    let project = Project::new()?;
    let before = project.write(
        "before.json",
        &run(vec![
            function("lost", 1, Some("partial"), &["conditional-expression"]),
            function("named", 2, Some("partial"), &["call"]),
            function("odd", 3, Some("partial"), &["subscript"]),
        ]),
    )?;
    let after = project.write(
        "after.json",
        &run(vec![
            function("lost", 1, Some("partial"), &["call", "attribute"]),
            function("named", 2, Some("partial"), &["call", "call"]),
            function(
                "odd",
                3,
                Some("partial"),
                &["subscript", "non-integer-value"],
            ),
        ]),
    )?;
    let text = project.run(&["diff", before.to_str().unwrap(), after.to_str().unwrap()])?;
    assert_eq!(text.code, EXIT_CLEAN, "{}", text.describe());
    assert!(
        text.stdout.contains(
            "functions with more holes: 3 (lost a region 1, named an interior 1, unexplained 1)"
        ),
        "{}",
        text.describe()
    );
    assert!(
        text.stdout.contains("UNEXPLAINED w.py:3 odd"),
        "{}",
        text.describe()
    );

    let json = project.run(&[
        "diff",
        before.to_str().unwrap(),
        after.to_str().unwrap(),
        "--json",
    ])?;
    let delta: Value = serde_json::from_str(&json.stdout).expect("a JSON delta");
    let kinds: Vec<&str> = delta["gainers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["explanation"]["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["lost-region", "named-interior", "unexplained"]);
    assert_eq!(
        delta["gainers"][0]["explanation"]["construct"],
        "conditional-expression"
    );
    Ok(())
}

/// **`report` is the single-run half.**
#[test]
fn a_report_is_the_picture_of_one_run() -> io::Result<()> {
    let project = Project::new()?;
    let path = project.write(
        "run.json",
        &run(vec![
            function("a", 1, Some("exact"), &[]),
            function("b", 2, Some("partial"), &["call", "call"]),
            function("c", 3, Some("partial"), &["call", "attribute"]),
            function("d", 4, None, &[]),
        ]),
    )?;
    let json = project.run(&["report", path.to_str().unwrap(), "--json"])?;
    assert_eq!(json.code, EXIT_CLEAN, "{}", json.describe());
    let picture: Value = serde_json::from_str(&json.stdout).expect("a JSON picture");
    assert_eq!(picture["functions"], 4);
    assert_eq!(picture["by_kind"]["exact"], 1);
    assert_eq!(picture["by_kind"]["partial"], 2);
    assert_eq!(picture["by_kind"]["none"], 1);
    assert_eq!(picture["holes"], 4);
    assert_eq!(picture["holes_by_construct"]["call"], 3);
    assert_eq!(
        picture["sole_blockers"]["call"], 1,
        "`c` has two constructs and is not a sole blocker"
    );

    let text = project.run(&["report", path.to_str().unwrap()])?;
    assert!(
        text.stdout.contains("blocked by one construct"),
        "{}",
        text.describe()
    );
    Ok(())
}

/// **A run from another schema version is refused by name; so is a file
/// that is not a run.**
#[test]
fn a_foreign_schema_or_a_non_run_is_a_tool_error() -> io::Result<()> {
    let project = Project::new()?;
    let current = project.write("current.json", &run(vec![]))?;
    let foreign = project.write(
        "foreign.json",
        &json!({"schema_version": 2, "functions": []}).to_string(),
    )?;
    let mismatch = project.run(&["diff", current.to_str().unwrap(), foreign.to_str().unwrap()])?;
    assert_eq!(mismatch.code, EXIT_TOOL_ERROR, "{}", mismatch.describe());
    assert!(
        mismatch.stderr.contains("foreign.json")
            && mismatch.stderr.contains("schema version 2")
            && mismatch.stderr.contains("reads 3"),
        "both versions and the file are named:\n{}",
        mismatch.describe()
    );

    let junk = project.write("junk.json", "not json at all")?;
    let unreadable = project.run(&["report", junk.to_str().unwrap()])?;
    assert_eq!(
        unreadable.code,
        EXIT_TOOL_ERROR,
        "{}",
        unreadable.describe()
    );
    assert!(
        unreadable.stderr.contains("junk.json"),
        "{}",
        unreadable.describe()
    );

    let missing = project.run(&["report", "nowhere.json"])?;
    assert_eq!(missing.code, EXIT_TOOL_ERROR, "{}", missing.describe());
    Ok(())
}
