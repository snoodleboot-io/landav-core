//! `LAN-113`: **a call to a constant-cost method defined in the same class is
//! declared from that method's own derived bound.**
//!
//! `self.helper(x)` inside `class Walker` where `def helper` is three lines
//! down is a call to a known function. LAN-110 measured it as a fifth of every
//! method-call blocker, and it needs no type inference: the receiver's class is
//! the class the call sits in. What was missing is that the call site never
//! used what the analysis already knew about the callee.
//!
//! Phase 1 (LAN-113): a sibling whose bound is exact and var-free is entered
//! into a table and declared at the call site as that many steps, through the
//! existing `DeclaredEffect` mechanism. Phase 2 (LAN-116): a sibling whose
//! bound is exact but *not* constant composes by substitution - its own bound,
//! rewritten into the caller's variables by the call's positional arguments -
//! carried as a `DeclaredCost::Composed` index into the program's side table.
//! The ITS lowering refuses such a node (a bound is not an ITS cost), so the
//! native engine composes while the ITS reach is unchanged.
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

/// **Phase 2: a non-constant sibling composes by substitution.**
///
/// `linear_helper` is `Θ(2 + 2·len(items))`; `calls_linear` passes its own
/// collection parameter `items`, so the composed bound mentions
/// `len(items)` in the *caller's* variables, is complete, and carries the
/// premise. Pinned on a variable rather than a literal shape: the algebra's
/// normal form is allowed to move, the dependence on `len(items)` is not.
#[test]
fn a_non_constant_sibling_composes_in_the_callers_variables() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", CLASS)?;
    let fs = functions(&project.check(&source, &["--json"])?);
    let f = named(&fs, "Walker.calls_linear");
    assert!(
        is_complete(f),
        "the linear sibling must compose, not hole: {f}"
    );
    assert!(
        f["bound"].as_str().unwrap().contains("len(items)"),
        "the composed bound must be in the caller's variables: {f}"
    );
    assert!(has_sibling_premise(f, "Walker.linear_helper"), "{f}");
    Ok(())
}

/// A few more Phase 2 shapes, in one class.
const PHASE_2: &str = "\
class W:
    def helper(self, items: list) -> int:
        total = 0
        for x in items:
            total = total + 1
        return total

    def in_loop(self, items: list, n: int) -> int:
        for i in range(n):
            self.helper(items)
        return 1

    def int_helper(self, k: int) -> int:
        for i in range(k):
            pass
        return 1

    def passes_int_param(self, n: int) -> int:
        self.int_helper(n)
        return 1

    def passes_literal(self) -> int:
        self.int_helper(5)
        return 1

    def passes_loop_counter(self, n: int) -> int:
        for i in range(n):
            self.int_helper(i)
        return 1

    def passes_keyword(self, n: int) -> int:
        self.int_helper(k=n)
        return 1

    def passes_wrong_kind(self, rows: list) -> int:
        self.int_helper(rows)
        return 1

    def partial_helper(self, n: int) -> int:
        unknown(n)
        return 1

    def calls_partial(self, n: int) -> int:
        self.partial_helper(n)
        return 1


class Base:
    def shape(self, n: int) -> int:
        for i in range(n):
            pass
        return 1

    def uses_shape(self, n: int) -> int:
        self.shape(n)
        return 1


class Sub(Base):
    def shape(self, n: int) -> int:
        for i in range(n):
            for j in range(n):
                pass
        return 1
";

/// **A composed sibling inside a loop is multiplied by the trip count.**
#[test]
fn a_composed_sibling_inside_a_loop_is_multiplied() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", PHASE_2)?;
    let fs = functions(&project.check(&source, &["--json"])?);
    let f = named(&fs, "W.in_loop");
    assert!(is_complete(f), "{f}");
    let b = f["bound"].as_str().unwrap();
    assert!(
        b.contains("len(items)") && b.contains('n'),
        "the bound must be the sibling's length term times the loop's n: {f}"
    );
    Ok(())
}

/// **An integer parameter and an integer literal both substitute.**
#[test]
fn an_integer_parameter_and_a_literal_substitute() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", PHASE_2)?;
    let fs = functions(&project.check(&source, &["--json"])?);
    let p = named(&fs, "W.passes_int_param");
    assert!(
        is_complete(p) && p["bound"].as_str().unwrap().contains('n'),
        "{p}"
    );
    let l = named(&fs, "W.passes_literal");
    assert!(
        is_complete(l) && !l["bound"].as_str().unwrap().contains('k'),
        "a literal argument leaves no sibling variable behind: {l}"
    );
    Ok(())
}

/// **What refuses, and must: never a bound with a variable the caller cannot
/// name, never a guess.**
#[test]
fn unmappable_arguments_keep_the_hole() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", PHASE_2)?;
    let fs = functions(&project.check(&source, &["--json"])?);
    for (name, why) in [
        (
            "W.passes_loop_counter",
            "a loop counter is rebound in the body, not a variable the engine names",
        ),
        (
            "W.passes_keyword",
            "a keyword argument cannot be mapped by position",
        ),
        (
            "W.passes_wrong_kind",
            "a collection passed where the sibling reads an integer",
        ),
        (
            "W.calls_partial",
            "a sibling whose own bound has a hole is not exact",
        ),
    ] {
        let f = named(&fs, name);
        assert!(!is_complete(f), "{name}: {why}: {f}");
        assert!(
            f["holes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|h| h["construct"] == "call"),
            "{name}: the call must stay a hole: {f}"
        );
    }
    Ok(())
}

/// **A method a subclass in the module overrides is never composed.**
///
/// `Sub.shape` is quadratic where `Base.shape` is linear. Composing
/// `Base.shape` into `Base.uses_shape` would be a confidently wrong *shape*
/// for a `Sub`, so the override is detected and the call stays a hole.
#[test]
fn a_method_overridden_in_the_module_is_not_composed() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("w.py", PHASE_2)?;
    let fs = functions(&project.check(&source, &["--json"])?);
    let f = named(&fs, "Base.uses_shape");
    assert!(!is_complete(f), "{f}");
    assert!(
        !has_sibling_premise(f, "Base.shape"),
        "no premise either: {f}"
    );
    Ok(())
}
