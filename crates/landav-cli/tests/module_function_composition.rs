//! `LAN-129`: **a call to a function the module defines once composes from
//! that definition's own bound, under a `same-module-function` premise.**
//!
//! The sibling table (`LAN-113`/116/126) keyed by a bare name. What makes the
//! bare name safe to read is the same thing that makes an import safe to read
//! (`LAN-128`): the module binds it exactly once, by a `def`, and the calling
//! function neither takes it as a parameter nor rebinds it. What the analysis
//! cannot see - a monkeypatch, a `global` assignment at run time - is the
//! premise, and it is said.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use common::Project;
use serde_json::Value;

fn functions(run: &common::Run) -> Vec<Value> {
    let doc: Value = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|e| panic!("not JSON ({e}):\n{}", run.describe()));
    doc["functions"].as_array().cloned().unwrap_or_default()
}

fn named<'a>(fs: &'a [Value], name: &str) -> &'a Value {
    fs.iter()
        .find(|f| f["name"] == name)
        .unwrap_or_else(|| panic!("no function {name} in {fs:?}"))
}

fn holes(f: &Value) -> Vec<String> {
    f["holes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["construct"].as_str().unwrap().to_owned())
        .collect()
}

fn has_premise(f: &Value, subject: &str) -> bool {
    f["premises"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["trust"] == "same-module-function" && p["subject"] == subject)
}

const MODULE: &str = "\
def constant(x) -> int:
    return 1


def linear(items: list) -> int:
    total = 0
    for a in items:
        total = total + 1
    return total


def leaky(obj) -> int:
    return obj.size


def calls_out(x) -> int:
    f(x)
    return 1


def uses_constant(x) -> int:
    constant(x)
    return 1


def uses_linear(items: list, n: int) -> int:
    for i in range(n):
        linear(items)
    return 1


def uses_leaky(obj) -> int:
    leaky(obj)
    return 1


def uses_calls_out(x) -> int:
    calls_out(x)
    return 1


def shadows_by_parameter(constant, x) -> int:
    constant(x)
    return 1


def rebinds_in_body(x) -> int:
    constant = other
    constant(x)
    return 1


def ping(n: int) -> int:
    pong(n)
    return 1


def pong(n: int) -> int:
    ping(n)
    return 1
";

#[test]
fn a_module_function_composes_like_a_sibling() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", MODULE)?;
    let fs = functions(&project.check(&source, &["--json"])?);

    let c = named(&fs, "uses_constant");
    assert_eq!(c["bound_kind"], "exact", "{c}");
    assert!(has_premise(c, "constant"), "{c}");

    let l = named(&fs, "uses_linear");
    assert_eq!(l["bound_kind"], "exact", "{l}");
    let b = l["bound"].as_str().unwrap();
    assert!(
        b.contains("len(items)") && b.contains('n'),
        "every parameter maps, no receiver skipped: {l}"
    );
    assert!(has_premise(l, "linear"), "{l}");

    let k = named(&fs, "uses_leaky");
    assert_eq!(
        holes(k),
        vec!["attribute"],
        "the callee's region, inherited: {k}"
    );
    assert!(
        k["holes"][0]["origin"]
            .as_str()
            .unwrap()
            .contains("w.py:13:"),
        "at the callee's line: {k}"
    );
    assert!(has_premise(k, "leaky"), "{k}");
    Ok(())
}

#[test]
fn what_must_not_compose_keeps_its_hole() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", MODULE)?;
    let fs = functions(&project.check(&source, &["--json"])?);
    for (name, why) in [
        (
            "uses_calls_out",
            "the callee still calls `f`: the cycle rule",
        ),
        ("shadows_by_parameter", "`constant` is a parameter here"),
        ("rebinds_in_body", "`constant` is rebound before the call"),
        ("ping", "a cycle never enters the table"),
        ("pong", "a cycle never enters the table"),
    ] {
        let f = named(&fs, name);
        assert!(holes(f).contains(&"call".to_owned()), "{name}: {why}: {f}");
        assert!(
            f["premises"].as_array().unwrap().is_empty(),
            "{name}: no premise: {f}"
        );
    }
    Ok(())
}

/// A module that binds the name twice means two things by it; neither is
/// read.
#[test]
fn a_name_the_module_binds_twice_is_not_read() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write(
        "w.py",
        "\
def helper(x) -> int:
    return 1

helper = make_helper()


def uses(x) -> int:
    helper(x)
    return 1
",
    )?;
    let fs = functions(&project.check(&source, &["--json"])?);
    let f = named(&fs, "uses");
    assert!(holes(f).contains(&"call".to_owned()), "{f}");
    Ok(())
}
