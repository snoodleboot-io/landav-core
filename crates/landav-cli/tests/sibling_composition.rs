//! `LAN-113`: **a call to a constant-cost method defined in the same class is
//! declared from that method's own derived bound.**
//!
//! `self.helper(x)` inside `class Walker` where `def helper` is three lines
//! down is a call to a known function. LAN-110 measured it as a fifth of every
//! method-call blocker, and it needs no type inference: the receiver's class is
//! the class the call sits in. What was missing is that the call site never
//! used what the analysis already knew about the callee.
//!
//! This is the narrow, constant-only first form (Phase 1): a sibling whose
//! bound is exact and var-free is entered into a table and declared at the
//! call site as that many steps, through the existing `DeclaredEffect`
//! mechanism. A sibling with a non-constant bound stays a hole (Phase 2).
//!
//! The soundness boundaries pinned here: recursion excludes itself (a method
//! in a cycle is never constant), an inherited method has no definition in the
//! body and stays a hole, a rebound receiver is not our class, and the
//! late-binding assumption is published as a `same-class-method` premise.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use common::Project;
use serde_json::Value;

const CLASS: &str = "\
class Walker:
    def helper(self, x) -> int:
        return 1

    def caller(self, n: int) -> int:
        total = 0
        for i in range(n):
            self.helper(i)
            total = total + 1
        return total

    def rec_a(self, n: int) -> int:
        self.rec_b(n)
        return 1

    def rec_b(self, n: int) -> int:
        self.rec_a(n)
        return 1

    def uses_inherited(self, n: int) -> int:
        self.not_defined_here(n)
        return 1

    def c(self) -> int:
        return 1

    def b(self) -> int:
        self.c()
        return 1

    def a(self, n: int) -> int:
        for i in range(n):
            self.b()
        return 1

    def rebinds_self(self, other, n: int) -> int:
        self = other
        self.helper(n)
        return 1

    def linear_helper(self, items: list) -> int:
        total = 0
        for x in items:
            total = total + 1
        return total

    def calls_linear(self, items: list) -> int:
        self.linear_helper(items)
        return 1
";

fn functions(run: &common::Run) -> Vec<Value> {
    let parsed: Value = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|e| panic!("not JSON: {e}\n{}", run.describe()));
    parsed["functions"].as_array().expect("functions").clone()
}

fn named<'a>(fs: &'a [Value], name: &str) -> &'a Value {
    fs.iter()
        .find(|f| f["name"] == name)
        .unwrap_or_else(|| panic!("no function {name}"))
}

fn is_complete(f: &Value) -> bool {
    f["holes"].as_array().is_some_and(Vec::is_empty) && f["bound"].is_string()
}

fn has_sibling_premise(f: &Value, subject: &str) -> bool {
    f["premises"].as_array().is_some_and(|ps| {
        ps.iter()
            .any(|p| p["trust"] == "same-class-method" && p["subject"] == subject)
    })
}

/// **The headline: the loop around a constant sibling is counted, and the
/// bound is complete, with the premise.**
#[test]
fn a_constant_sibling_is_composed_and_the_premise_is_published() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", CLASS)?;
    let run = project.check(&source, &["--json"])?;
    let fs = functions(&run);

    let caller = named(&fs, "Walker.caller");
    assert!(
        is_complete(caller),
        "the call to a constant sibling must not hole: {caller}"
    );
    assert!(
        caller["bound"].as_str().unwrap().contains('n'),
        "the loop around the call must keep its count: {caller}"
    );
    assert!(
        has_sibling_premise(caller, "Walker.helper"),
        "the late-binding assumption must be published: {caller}"
    );

    let text = project.check(&source, &["--bounds"])?;
    assert!(
        text.mentions("believed on `Walker.helper`"),
        "the premise must be in the text too:\n{}",
        text.describe()
    );
    Ok(())
}

/// **Recursion excludes itself.** Neither method in a cycle is ever constant.
#[test]
fn a_recursive_pair_keeps_both_holes() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", CLASS)?;
    let fs = functions(&project.check(&source, &["--json"])?);
    for name in ["Walker.rec_a", "Walker.rec_b"] {
        let f = named(&fs, name);
        assert!(
            !is_complete(f),
            "{name} is in a cycle and must stay a hole: {f}"
        );
        assert!(!has_sibling_premise(f, "Walker.rec_a") && !has_sibling_premise(f, "Walker.rec_b"));
    }
    Ok(())
}

/// **An inherited method has no definition here, so the call stays a hole.**
#[test]
fn an_inherited_method_stays_a_hole() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", CLASS)?;
    let fs = functions(&project.check(&source, &["--json"])?);
    let f = named(&fs, "Walker.uses_inherited");
    assert!(!is_complete(f), "{f}");
    Ok(())
}

/// **A chain resolves one link per round.** `c` constant → `b` constant → `a`.
#[test]
fn a_chain_of_siblings_resolves_to_a_fixed_point() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", CLASS)?;
    let fs = functions(&project.check(&source, &["--json"])?);
    assert!(is_complete(named(&fs, "Walker.b")));
    let a = named(&fs, "Walker.a");
    assert!(is_complete(a), "{a}");
    assert!(has_sibling_premise(a, "Walker.b"), "{a}");
    assert!(has_sibling_premise(named(&fs, "Walker.b"), "Walker.c"));
    Ok(())
}

/// **A rebound receiver is not our class.** `self = other; self.helper()`.
#[test]
fn a_rebound_receiver_is_not_resolved() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", CLASS)?;
    let fs = functions(&project.check(&source, &["--json"])?);
    let f = named(&fs, "Walker.rebinds_self");
    assert!(
        f["holes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["construct"] == "call"),
        "after `self = other` the call is on an unknown object: {f}"
    );
    Ok(())
}

/// **Phase 1 is constant-only.** A linear sibling stays a hole, never a wrong
/// constant.
#[test]
fn a_non_constant_sibling_stays_a_hole() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", CLASS)?;
    let fs = functions(&project.check(&source, &["--json"])?);
    let f = named(&fs, "Walker.calls_linear");
    assert!(
        !is_complete(f),
        "a Θ(len) sibling must not be composed as a constant: {f}"
    );
    Ok(())
}
