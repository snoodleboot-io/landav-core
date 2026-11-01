//! `LAN-127`: **a field the class only ever stores a builtin collection into
//! is a collection receiver.**
//!
//! # The gap this closes
//!
//! `LAN-134` let a method resolve against a pack row when the receiver is a
//! parameter annotated with a concrete builtin - `def f(xs: list)` and
//! `xs.append(v)`. Real code does not look like that. It looks like
//! `self.items = []` in `__init__` and `self.items.append(v)` in a method, and
//! counted on `6b71ee0` there are 504 such `self.attr.m()` call holes on the
//! standard library, 62 of them in 38 functions blocked by nothing else.
//!
//! # Why a field can be trusted where a parameter must be annotated
//!
//! The two paths reach the same conclusion from opposite ends, and the
//! difference is worth stating because it looks like a weakening and is not.
//!
//! For a parameter, the kind is a claim the *caller* makes. The analysis
//! cannot check it, so it insists on an annotation, and on a concrete one: a
//! protocol's `append` is user code of unknown cost.
//!
//! For a field, the kind is something this module can **read**. `self.rows =
//! []` is a list - said by the program rather than about it - and the only
//! open question is whether anything else ever stores anything different. That
//! question is answered over the class's same-module chain, which is the same
//! scope and the same scan `LAN-120` uses to decide that a store is a plain
//! `object.__setattr__`.
//!
//! # What the premise carries
//!
//! `same-class-field`, subject `Class.field`. What the chain cannot see is a
//! subclass in another module, a caller assigning the attribute, or a
//! `setattr` - so the premise says in as many words that nothing outside the
//! chain stores anything else. A reader who knows their code does that can
//! see the bound resting on it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use common::Project;
use serde_json::Value;

fn functions(run: &common::Run) -> Vec<Value> {
    let doc: Value = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("not JSON ({why}):\n{}", run.describe()));
    doc["functions"].as_array().cloned().unwrap_or_default()
}

fn named<'a>(functions: &'a [Value], name: &str) -> &'a Value {
    functions
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("no function {name} in {functions:?}"))
}

fn holes(function: &Value) -> Vec<String> {
    function["holes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hole| hole["construct"].as_str().unwrap().to_owned())
        .collect()
}

fn premises(function: &Value) -> Vec<(String, String)> {
    function["premises"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|premise| {
                    (
                        premise["trust"].as_str().unwrap_or_default().to_owned(),
                        premise["subject"].as_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn check_json(source: &str) -> io::Result<Vec<Value>> {
    let project = Project::new()?;
    let file = project.write("w.py", source)?;
    Ok(functions(&project.check(&file, &["--json"])?))
}

fn check_text(source: &str) -> io::Result<String> {
    let project = Project::new()?;
    let file = project.write("w.py", source)?;
    Ok(project.check(&file, &["--bounds"])?.stdout)
}

/// A class that stores a list display once and appends to it in a counted loop.
const BOX: &str = "\
class Box:
    def __init__(self):
        self.items = []

    def fill(self, n: int) -> int:
        total = 0
        for i in range(n):
            self.items.append(i)
            total = total + 1
        return total
";

/// **The loop is counted, and the field premise is published.**
#[test]
fn a_field_only_ever_stored_as_a_list_is_a_collection_receiver() -> io::Result<()> {
    let functions = check_json(BOX)?;
    let fill = named(&functions, "Box.fill");

    assert_eq!(
        fill["bound_kind"], "exact",
        "the `append` resolves through the field, so nothing is left in the \
         way of counting this loop: {fill}"
    );
    assert!(holes(fill).is_empty(), "no hole should remain: {fill}");
    assert!(
        premises(fill)
            .iter()
            .any(|(trust, subject)| trust == "same-class-field" && subject == "Box.items"),
        "the bound rests on nothing outside the class storing anything else \
         into `items`, and has to say so: got {:?}",
        premises(fill)
    );
    assert!(
        premises(fill)
            .iter()
            .any(|(trust, _)| trust == "amortised-cost"),
        "`LAN-134`'s premise still applies - the field changes which receiver \
         resolves, not what `append` costs: got {:?}",
        premises(fill)
    );
    Ok(())
}

/// **The text line says it too.**
#[test]
fn the_text_line_names_the_field() -> io::Result<()> {
    let text = check_text(BOX)?;
    assert!(
        text.contains("taking `Box.items` as a builtin collection"),
        "a reader of the default bound output must see which field was taken \
         as a collection, and on what assumption:\n{text}"
    );
    Ok(())
}

/// **Every way the field's kind is not readable keeps the hole.**
///
/// One table rather than seven tests, because each entry is the same assertion
/// about a different reason, and a reader comparing them learns more from
/// seeing them together. Each class is named for the reason.
#[test]
fn a_field_whose_kind_the_class_cannot_vouch_for_keeps_the_hole() -> io::Result<()> {
    for (name, body) in [
        // Stored from a parameter: the type is whatever the caller passed.
        (
            "FromParam",
            "    def __init__(self, items):
        self.items = items
",
        ),
        // Stored from a call result: this analysis does not know what `build`
        // returns, and a row keyed on the receiver's kind may not guess.
        (
            "FromCall",
            "    def __init__(self):
        self.items = build()
",
        ),
        // Two kinds in the same class. Neither is the field's kind.
        (
            "TwoKinds",
            "    def __init__(self):
        self.items = []

    def swap(self):
        self.items = {}
",
        ),
        // An augmented store reads the field first and the operator is
        // whatever is in it, so the scan declines to classify it.
        (
            "Augmented",
            "    def __init__(self):
        self.items = []

    def extend(self, more):
        self.items += more
",
        ),
        // A class-level binding of the name may be a `property` whose getter
        // returns anything at all - the same reason `LAN-120` refuses the
        // store.
        (
            "ClassLevel",
            "    items = []

    def __init__(self):
        self.items = []
",
        ),
        // A `__setattr__` in the chain may store something else entirely.
        (
            "Intercepted",
            "    def __setattr__(self, key, value):
        pass

    def __init__(self):
        self.items = []
",
        ),
    ] {
        let source = format!(
            "\
class {name}:
{body}
    def fill(self, n: int) -> int:
        total = 0
        for i in range(n):
            self.items.append(i)
            total = total + 1
        return total
"
        );
        let functions = check_json(&source)?;
        let fill = named(&functions, &format!("{name}.fill"));
        assert!(
            holes(fill).contains(&"call".to_owned()),
            "{name}: the class cannot say what `items` holds, so the call must \
             keep its hole: {fill}\nfor:\n{source}"
        );
        assert!(
            !premises(fill)
                .iter()
                .any(|(trust, _)| trust == "same-class-field"),
            "{name}: nothing was taken as a collection field, so nothing \
             should claim it was: {fill}"
        );
    }
    Ok(())
}

/// **A subclass storing a different kind disqualifies the field.**
///
/// The chain is the scope, not the class. `fill` is defined on the base and
/// the call may run on a `Sub`, whose `reset` leaves a dict in the same field,
/// so the base's own store is not the whole story. This is the case a
/// class-at-a-time scan would get wrong.
#[test]
fn a_subclass_storing_another_kind_disqualifies_the_field() -> io::Result<()> {
    let source = "\
class Base:
    def __init__(self):
        self.rows = []

    def fill(self, n: int) -> int:
        total = 0
        for i in range(n):
            self.rows.append(i)
            total = total + 1
        return total


class Sub(Base):
    def reset(self):
        self.rows = {}
";
    let functions = check_json(source)?;
    let fill = named(&functions, "Base.fill");
    assert!(
        holes(fill).contains(&"call".to_owned()),
        "a subclass in the same module stores a dict into `rows`, so the \
         field has two kinds and no receiver: {fill}"
    );
    Ok(())
}

/// **A rebound `self` is not this class's instance.**
///
/// `self = other()` makes every line below it about an object whose class
/// nothing here has read, so the chain's stores say nothing about its fields.
#[test]
fn a_rebound_receiver_keeps_the_hole() -> io::Result<()> {
    let source = "\
class Box:
    def __init__(self):
        self.items = []

    def fill(self, n: int) -> int:
        total = 0
        self = other()
        for i in range(n):
            self.items.append(i)
            total = total + 1
        return total
";
    let functions = check_json(source)?;
    let fill = named(&functions, "Box.fill");
    assert!(
        holes(fill).contains(&"call".to_owned()),
        "`self` was rebound, so the class's stores describe a different \
         object: {fill}"
    );
    Ok(())
}

/// **A builtin constructor counts, and only while it is the builtin.**
///
/// `self.items = list()` is a list for the same reason `[]` is, and stops
/// being one the moment the module rebinds `list` - the same fail-closed rule
/// the signature pack applies to a bare callee name. A module whose bindings
/// could not be enumerated gets nothing either, which is why this is checked
/// rather than assumed.
#[test]
fn a_builtin_constructor_counts_unless_the_module_rebinds_it() -> io::Result<()> {
    let builtin = "\
class Box:
    def __init__(self):
        self.items = list()

    def fill(self, n: int) -> int:
        total = 0
        for i in range(n):
            self.items.append(i)
            total = total + 1
        return total
";
    let functions = check_json(builtin)?;
    let fill = named(&functions, "Box.fill");
    assert_eq!(
        fill["bound_kind"], "exact",
        "`list()` is the builtin here, so the field is a list: {fill}"
    );

    let shadowed = "\
def list(source):
    return source


class Box:
    def __init__(self):
        self.items = list()

    def fill(self, n: int) -> int:
        total = 0
        for i in range(n):
            self.items.append(i)
            total = total + 1
        return total
";
    let functions = check_json(shadowed)?;
    let fill = named(&functions, "Box.fill");
    assert!(
        holes(fill).contains(&"call".to_owned()),
        "this module's `list` is its own function, so `list()` says nothing \
         about the field's kind: {fill}"
    );
    Ok(())
}

/// **A method with no row stays a hole, however well known the receiver is.**
///
/// The receiver's *kind* is the new fact; the row is still the claim.
/// `list.remove` scans for its element and runs that element's `__eq__`, so it
/// has no row and knowing the field is a list changes nothing about it.
#[test]
fn a_method_with_no_row_stays_a_hole() -> io::Result<()> {
    let source = "\
class Box:
    def __init__(self):
        self.items = []

    def drop(self, n: int) -> int:
        total = 0
        for i in range(n):
            self.items.remove(i)
            total = total + 1
        return total
";
    let functions = check_json(source)?;
    let drop = named(&functions, "Box.drop");
    assert!(
        holes(drop).contains(&"call".to_owned()),
        "`remove` has no row, so the call stays a hole even on a field this \
         class fully vouches for: {drop}"
    );
    Ok(())
}
