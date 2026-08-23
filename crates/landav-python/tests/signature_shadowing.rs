//! `LAN-94`: a signature pack is keyed by **name**, and a name can mean
//! something else.
//!
//! `signature_pack.rs` in `landav-cli` pins what the pack buys. This file pins
//! the one thing it must not buy: a bounded cost for a call to something the
//! analysis has never seen. `isinstance` is a builtin only until a module writes
//! `def isinstance(...)`, and every test below is a spelling of that.
//!
//! The choice recorded here is to **key on "not bound in this module or this
//! function"** rather than to accept the risk in prose. The module's own
//! bindings are visible to the same pass that already decides which names are
//! integers, so the check costs one traversal and removes every shadowing a
//! reader of the file could point at. What it does not remove is a wildcard
//! import - `from mymod import *` can rebind `isinstance` with nothing in the
//! module to show for it - and that residue is written down in
//! `landav_fdk::SignaturePack` rather than papered over.
//!
//! These live in `landav-python` rather than beside the acceptance tests because
//! the question is entirely a frontend one: which *names* the source binds. The
//! engine and the lowering never learn a callee's name at all.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_its::Construct;

/// Whether the lowering refuses a `call` to `callee` in the function `g`.
fn refuses_call_to(source: &str, callee: &str) -> bool {
    let functions = landav_python::lower_module(Path::new("shadow.py"), source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    let function = functions
        .iter()
        .find(|it| it.name() == "g")
        .unwrap_or_else(|| panic!("expected a function `g` in:\n{source}"));
    match landav_its::lower(function.program()) {
        Ok(_) => false,
        Err(error) => error.refusals().is_some_and(|ledger| {
            ledger.as_slice().iter().any(|record| {
                record.construct() == Construct::Call
                    && record.detail().is_some_and(|it| it.as_str() == callee)
            })
        }),
    }
}

/// The baseline every case below is measured against.
const PLAIN: &str = "\
def g(x) -> int:
    isinstance(x, int)
    return 0
";

#[test]
fn an_unshadowed_builtin_is_resolved() {
    assert!(
        !refuses_call_to(PLAIN, "isinstance"),
        "with nothing rebinding the name this is the builtin, and the pack \
         resolves it - every case below is this one plus a binding, so if this \
         fails the rest prove nothing"
    );
}

#[test]
fn a_name_the_module_rebinds_gets_no_signature() {
    for (what, source) in [
        (
            "a module-level def",
            "\
def isinstance(value, kind) -> int:
    return 0

def g(x) -> int:
    isinstance(x, int)
    return 0
",
        ),
        (
            "a module-level assignment",
            "\
isinstance = check

def g(x) -> int:
    isinstance(x, int)
    return 0
",
        ),
        (
            "an aliased import",
            "\
from mymod import check as isinstance

def g(x) -> int:
    isinstance(x, int)
    return 0
",
        ),
        (
            "a plain import",
            "\
from mymod import isinstance

def g(x) -> int:
    isinstance(x, int)
    return 0
",
        ),
        (
            "a class of that name",
            "\
class isinstance:
    pass

def g(x) -> int:
    isinstance(x, int)
    return 0
",
        ),
        (
            "a binding inside a branch",
            "\
if FAST:
    isinstance = fast_check

def g(x) -> int:
    isinstance(x, int)
    return 0
",
        ),
        (
            "a parameter of the calling function",
            "\
def g(x, isinstance) -> int:
    isinstance(x, int)
    return 0
",
        ),
        (
            "a local of the calling function",
            "\
def g(x) -> int:
    isinstance = x.check
    isinstance(x, int)
    return 0
",
        ),
        (
            "a nested def",
            "\
def g(x) -> int:
    def isinstance(value, kind):
        return 0
    isinstance(x, int)
    return 0
",
        ),
        (
            "a loop target",
            "\
def g(x, items) -> int:
    for isinstance in items:
        pass
    isinstance(x, int)
    return 0
",
        ),
    ] {
        assert!(
            refuses_call_to(source, "isinstance"),
            "{what} means `isinstance` is not the builtin, and a signature keyed \
             by name would publish a bounded cost for code this analysis has \
             never seen. It must stay a hole. For:\n{source}"
        );
    }
}

#[test]
fn a_name_another_function_binds_does_not_disqualify_this_one() {
    let source = "\
def other(isinstance) -> int:
    return 0

def g(x) -> int:
    isinstance(x, int)
    return 0
";
    assert!(
        !refuses_call_to(source, "isinstance"),
        "a parameter of a *different* function is a local of that function and \
         says nothing about what the name means here. Refusing on it would \
         throw away coverage for no soundness gain. For:\n{source}"
    );
}

#[test]
fn a_scope_whose_bindings_cannot_be_enumerated_disqualifies_every_signature() {
    let source = "\
def g(x, value) -> int:
    match value:
        case other:
            pass
    isinstance(x, int)
    return 0
";
    assert!(
        refuses_call_to(source, "isinstance"),
        "a `case` pattern binds capture names this pass does not walk, so the \
         scope answers `names I could not enumerate` and every signature in it \
         is disqualified. Failing closed costs coverage; guessing would cost \
         soundness. For:\n{source}"
    );
}

#[test]
fn an_argument_that_hides_a_cost_keeps_the_call_a_hole() {
    for (what, source) in [
        (
            "an attribute, which runs a `property`",
            "def g(x, node) -> int:\n    isinstance(x, node.kind)\n    return 0\n",
        ),
        (
            "a subscript, which runs a `__getitem__`",
            "def g(x, table) -> int:\n    isinstance(x, table[0])\n    return 0\n",
        ),
        (
            "a comprehension, which runs a loop",
            "def g(x, items) -> int:\n    isinstance(x, [k for k in items])\n    return 0\n",
        ),
    ] {
        assert!(
            refuses_call_to(source, "isinstance"),
            "{what}: an unresolved call denotes `omega` and covers for whatever \
             is written inside it, and resolving the call takes that cover away. \
             The argument is not translated here, so resolving would publish a \
             complete bound with its cost missing. For:\n{source}"
        );
    }
}

#[test]
fn a_call_in_an_argument_is_still_named_where_it_stands() {
    let source = "def g(x, n: int) -> int:\n    isinstance(x, lookup(n))\n    return 0\n";
    assert!(
        refuses_call_to(source, "lookup"),
        "`lookup` is translated and named where it stands, so resolving \
         `isinstance` removes only `isinstance`'s hole. For:\n{source}"
    );
    assert!(
        !refuses_call_to(source, "isinstance"),
        "and `isinstance` itself is resolved: its argument reaches a call, which \
         is translated, so nothing is hiding under it. For:\n{source}"
    );
}

#[test]
fn getattr_is_the_two_argument_form_only() {
    let two = "def g(x) -> int:\n    getattr(x, \"field\")\n    return 0\n";
    let three = "def g(x) -> int:\n    getattr(x, \"field\", None)\n    return 0\n";
    assert!(
        !refuses_call_to(two, "getattr"),
        "the two-argument form is one attribute lookup, which is exactly what \
         `Construct::Attribute` already claims for `x.y`"
    );
    assert!(
        refuses_call_to(three, "getattr"),
        "the three-argument form is that plus a caught `AttributeError` and a \
         third expression, and the row was written for the two-argument form. \
         Arity is a language question, so the frontend asks it rather than the \
         pack"
    );
}

#[test]
fn a_method_call_is_never_matched_against_a_row() {
    let source = "def g(x) -> int:\n    x.isinstance(1)\n    return 0\n";
    assert!(
        refuses_call_to(source, "isinstance"),
        "`x.isinstance` is a method on an object whose class this analysis has \
         never seen. Matching any object's attribute against a row keyed by a \
         builtin's name is a far weaker claim than matching a module-level name, \
         and the pack's method rows are refusals that stay documentary. \
         For:\n{source}"
    );
}
