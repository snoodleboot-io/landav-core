//! `LAN-120`: **`self.attr = v` is one step when nothing in the class chain
//! intercepts it, and the bound says so.**
//!
//! After `LAN-118` a store through `self` was the sole blocker for 362 stdlib
//! functions, 658 of their 717 holes `self.attr = v`. A plain attribute store
//! is `object.__setattr__`, a dictionary write - unless the receiver's type
//! defines `__setattr__`, binds `attr` as a descriptor, declares `__slots__`,
//! or carries a metaclass or class decorator that may install any of those.
//! The frontend knows one type for `self`: the enclosing class and its
//! same-module chain. Within what the chain shows, the store is admitted;
//! what it cannot show is the `plain-attribute-store` premise.

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

fn is_exact(f: &Value) -> bool {
    f["bound_kind"] == "exact"
}

fn holes(f: &Value) -> Vec<String> {
    f["holes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["construct"].as_str().unwrap().to_owned())
        .collect()
}

fn has_store_premise(f: &Value, subject: &str) -> bool {
    f["premises"].as_array().unwrap().iter().any(|p| {
        p["trust"] == "plain-attribute-store"
            && p["subject"] == subject
            && p["because"]
                .as_str()
                .unwrap()
                .contains("object.__setattr__")
    })
}

fn check(source: &str) -> io::Result<(Project, Vec<Value>)> {
    let project = Project::new()?;
    let file = project.write("w.py", source)?;
    let run = project.check(&file, &["--json"])?;
    let fs = functions(&run);
    Ok((project, fs))
}

/// **A store on `self` in a clean chain is a step, and the premise is
/// published, in JSON and in text.**
#[test]
fn a_plain_store_derives_exactly_with_the_premise() -> io::Result<()> {
    let source = "\
class A:
    def __init__(self, x, y: int) -> int:
        self.x = x
        self.count: int = y
        return 1


class B(object):
    def set(self, v) -> int:
        self.v = v
        return 1
";
    let (project, fs) = check(source)?;
    for (name, subject) in [("A.__init__", "A.x"), ("B.set", "B.v")] {
        let f = named(&fs, name);
        assert!(is_exact(f), "{name}: a plain store is one step: {f}");
        assert!(
            has_store_premise(f, subject),
            "{name}: the premise names the attribute: {f}"
        );
    }
    let a = named(&fs, "A.__init__");
    assert!(
        has_store_premise(a, "A.count"),
        "an annotated store too: {a}"
    );

    let file = project.write("w.py", source)?;
    let text = project.check(&file, &["--bounds"])?;
    assert!(
        text.stdout.contains("believed on `A.count`, `A.x`")
            && text.stdout.contains("plain `object.__setattr__`"),
        "the text says what the bound rests on:\n{}",
        text.describe()
    );
    Ok(())
}

/// **An external base is the premise, not a refusal.**
#[test]
fn an_external_base_is_covered_by_the_premise() -> io::Result<()> {
    let (_, fs) = check(
        "\
from somewhere import Base


class A(Base):
    def __init__(self, x) -> int:
        self.x = x
        return 1
",
    )?;
    let f = named(&fs, "A.__init__");
    assert!(is_exact(f) && has_store_premise(f, "A.x"), "{f}");
    Ok(())
}

/// **The value's own cost survives; only the store's hole is gone.**
#[test]
fn the_stored_values_cost_is_still_charged() -> io::Result<()> {
    let (_, fs) = check(
        "\
class A:
    def __init__(self, x) -> int:
        self.x = f(x)
        return 1
",
    )?;
    let f = named(&fs, "A.__init__");
    assert_eq!(holes(f), vec!["call"], "{f}");
    assert!(
        has_store_premise(f, "A.x"),
        "the store is still admitted: {f}"
    );
    Ok(())
}

/// **What the chain can see refuses: `__setattr__`, a class-level binding of
/// the name, a metaclass, a class decorator - in the class, a same-module
/// base, or a same-module subclass. `__slots__` does not (`LAN-121`).**
#[test]
fn an_intercepted_chain_refuses() -> io::Result<()> {
    let source = "\
class Base:
    def __init__(self, x) -> int:
        self.x = x
        return 1


class Sub(Base):
    def __setattr__(self, name, value):
        pass


class Prop:
    def set(self, v) -> int:
        self.v = v
        return 1

    @property
    def v(self):
        return 1


class ViaBase(Prop):
    def put(self, w) -> int:
        self.w = w
        return 1


class Slotted:
    __slots__ = ('x',)

    def set(self, x) -> int:
        self.x = x
        return 1


class Meta(type):
    pass


class WithMeta(metaclass=Meta):
    def set(self, x) -> int:
        self.x = x
        return 1


@decorated
class Decorated:
    def set(self, x) -> int:
        self.x = x
        return 1


class ClassAttr:
    x = 0

    def set(self, x) -> int:
        self.x = x
        return 1
";
    let (_, fs) = check(source)?;
    for (name, why) in [
        (
            "Base.__init__",
            "a same-module subclass defines `__setattr__`",
        ),
        ("Prop.set", "`v` is a property on the class"),
        (
            "ViaBase.put",
            "a property on `v` in a base says nothing about `w`",
        ),
        (
            "Slotted.set",
            "a slot is a fixed-offset write, not an interception",
        ),
        ("WithMeta.set", "a metaclass"),
        (
            "Decorated.set",
            "a class decorator may install a `__setattr__`",
        ),
        ("ClassAttr.set", "`x` is bound at class level"),
    ] {
        let f = named(&fs, name);
        if name == "ViaBase.put" {
            // `w` is bound nowhere in the chain and nothing intercepts stores
            // generally: a property on `v` says nothing about `w`. Admitted.
            assert!(is_exact(f), "{name}: {why}: {f}");
            continue;
        }
        if name == "Slotted.set" {
            // `LAN-121`: a slot is a member descriptor, one step to write.
            assert!(
                is_exact(f) && has_store_premise(f, "Slotted.x"),
                "{name}: {why}: {f}"
            );
            continue;
        }
        assert!(
            holes(f).contains(&"complex-assignment-target".to_owned()),
            "{name}: {why}: the store must stay a hole: {f}"
        );
        assert!(
            f["premises"].as_array().unwrap().is_empty(),
            "{name}: no premise for a store that was not admitted: {f}"
        );
    }
    Ok(())
}

/// **A receiver that is not an instance, or not the instance, refuses.**
#[test]
fn a_receiver_that_is_not_the_instance_refuses() -> io::Result<()> {
    let (_, fs) = check(
        "\
class A:
    @classmethod
    def make(cls, x) -> int:
        cls.x = x
        return 1

    @staticmethod
    def put(self, x) -> int:
        self.x = x
        return 1

    def swap(self, other, x) -> int:
        self = other
        self.x = x
        return 1

    def elsewhere(self, obj, x) -> int:
        obj.x = x
        return 1

    def item(self, k, x) -> int:
        self.x[k] = x
        return 1

    def bump(self, n: int) -> int:
        self.n += n
        return 1


def free(self, x) -> int:
    self.x = x
    return 1
",
    )?;
    for name in ["A.make", "A.put", "A.swap", "A.elsewhere", "A.item", "free"] {
        let f = named(&fs, name);
        assert!(
            holes(f).contains(&"complex-assignment-target".to_owned()),
            "{name}: the store must stay a hole: {f}"
        );
    }
    let bump = named(&fs, "A.bump");
    assert!(
        !is_exact(bump),
        "`self.n += n` reads `self.n` and may run user code: {bump}"
    );
    Ok(())
}

/// **An admitted store touches nothing the frame can read; a refused one
/// still forgets everything.**
///
/// `LAN-122`. The premise the store is published under says it is
/// `object.__setattr__`: the instance's own dictionary or slot, no other
/// object. So a parameter's length (`LAN-101`'s volatile names) survives it,
/// and the loop below is counted exactly as the loop above. The control is the
/// same store on a chain that intercepts - there a `__setattr__` may run
/// anything, and the loop below loses its length as before.
#[test]
fn an_admitted_store_keeps_lengths_and_a_refused_one_forgets_them() -> io::Result<()> {
    let (_, fs) = check(
        "\
class A:
    def before(self, items: list) -> int:
        total = 0
        for a in items:
            total = total + 1
        self.items = items
        return total

    def after(self, items: list) -> int:
        self.items = items
        total = 0
        for a in items:
            total = total + 1
        return total


class Hooked:
    def __setattr__(self, name, value):
        pass

    def after(self, items: list) -> int:
        self.items = items
        total = 0
        for a in items:
            total = total + 1
        return total
",
    )?;
    for name in ["A.before", "A.after"] {
        let f = named(&fs, name);
        assert!(
            is_exact(f) && f["bound"].as_str().unwrap().contains("len(items)"),
            "{name}: the loop is counted on the parameter's length whichever side of the \
             store it is on: {f}"
        );
    }
    let hooked = named(&fs, "Hooked.after");
    assert!(
        !is_exact(hooked) && holes(hooked).contains(&"complex-assignment-target".to_owned()),
        "a store the chain intercepts is a hole and forgets the length below it: {hooked}"
    );
    Ok(())
}
