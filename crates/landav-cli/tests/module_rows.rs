//! `LAN-128`: **a callee reached through an imported module name resolves
//! against a `module.attr` row.**
//!
//! `os.getcwd()` costs one syscall whatever module calls it, and the pack can
//! say so - but only under a key that means something: `os` has to be the
//! name an absolute import bound, once, and never rebound in the function. A
//! name with any other meaning gets no row, exactly as a bare builtin name
//! that the module shadows gets none (`LAN-94`).
//!
//! # `lazy import` is a fourth way the name can mean something else
//!
//! `LAN-132` raised the declared Python to 3.15, which brought PEP 810. A
//! lazily bound name is not a module - it is a promise, and the *first read of
//! it* runs the whole module import. So `os.getcwd()` under `lazy import os`
//! is not the one step this pack's row claims; it is one step plus an
//! unbounded import, which is precisely the cost a row must never hide.
//!
//! That makes it a soundness rule rather than a coverage one, and the only
//! one of the three releases' new syntax that changes an answer. It is why
//! `lowering::imports_of` skips a lazy import while `binding_sites` still
//! counts it: a lazy import does bind the name, so it must still disqualify a
//! row keyed by it; what `lazy` changes is the cost of the read, not the
//! meaning of the name.

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

fn check(source: &str) -> io::Result<Vec<Value>> {
    let project = Project::new()?;
    let file = project.write("w.py", source)?;
    Ok(functions(&project.check(&file, &["--json"])?))
}

/// **The three spellings of an import all reach the row.**
#[test]
fn a_callee_through_an_import_resolves_under_its_real_name() -> io::Result<()> {
    let fs = check(
        "\
import os
import threading as th
from time import time as now
import codecs


def plain() -> int:
    os.getcwd()
    return 1


def aliased() -> int:
    th.Lock()
    return 1


def from_import() -> int:
    now()
    return 1


def registry(name) -> int:
    codecs.CodecInfo(name=name)
    return 1
",
    )?;
    for name in ["plain", "aliased", "from_import", "registry"] {
        let f = named(&fs, name);
        assert_eq!(f["bound_kind"], "exact", "{name}: {f}");
    }
    Ok(())
}

/// **A name that means something else gets no row.**
#[test]
fn a_shadowed_or_rebound_module_name_keeps_the_hole() -> io::Result<()> {
    let fs = check(
        "\
import os
import time

os = make_fake_os()


def shadowed_at_module_level() -> int:
    os.getcwd()
    return 1


def rebound_in_the_body() -> int:
    time = other()
    time.time()
    return 1


def not_a_row() -> int:
    time.sleep(1)
    return 1
",
    )?;
    for name in [
        "shadowed_at_module_level",
        "rebound_in_the_body",
        "not_a_row",
    ] {
        let f = named(&fs, name);
        assert!(
            holes(f).contains(&"call".to_owned()),
            "{name}: the call must stay a hole: {f}"
        );
    }
    Ok(())
}

/// **A relative import names a module of this package; it is not the row's.**
#[test]
fn a_relative_import_is_not_the_stdlib_module() -> io::Result<()> {
    let fs = check(
        "\
from . import os


def local_os() -> int:
    os.getcwd()
    return 1
",
    )?;
    let f = named(&fs, "local_os");
    assert!(holes(f).contains(&"call".to_owned()), "{f}");
    Ok(())
}

/// **A lazily bound name resolves no row.** PEP 810, `LAN-132`.
///
/// The eager function is in the same file on purpose. Without it this test
/// could pass because the rows stopped working altogether, which is the
/// failure mode a negative assertion invites: `eager` proves the rows still
/// resolve, and the two lazy ones prove they do not resolve through a promise.
///
/// Both spellings are covered because they bind through different code paths -
/// `lazy import time` binds a module name, `lazy from codecs import CodecInfo`
/// binds an attribute - and a fix to one would plausibly leave the other.
#[test]
fn a_lazy_import_does_not_resolve_a_module_attribute_row() -> io::Result<()> {
    let fs = check(
        "\
import os
lazy import time
lazy from codecs import CodecInfo


def eager() -> int:
    os.getcwd()
    return 1


def lazily_imported() -> int:
    time.time()
    return 1


def lazily_from_imported(name) -> int:
    CodecInfo(name=name)
    return 1
",
    )?;
    let eager = named(&fs, "eager");
    assert_eq!(
        eager["bound_kind"], "exact",
        "the eager control must still resolve, or this test proves nothing \
         about `lazy`: {eager}"
    );
    for name in ["lazily_imported", "lazily_from_imported"] {
        let f = named(&fs, name);
        assert!(
            holes(f).contains(&"call".to_owned()),
            "{name}: a lazily bound name pays its module's import on first \
             read, so the call must keep its hole rather than resolve to the \
             row's one step: {f}"
        );
    }
    Ok(())
}

/// **What the rows deliberately leave out stays a hole.**
#[test]
fn an_input_proportional_callee_has_no_row() -> io::Result<()> {
    let fs = check(
        "\
import codecs
import re


def encode(data, table) -> int:
    codecs.charmap_encode(data, 'strict', table)
    return 1


def compile(pattern) -> int:
    re.compile(pattern)
    return 1
",
    )?;
    for name in ["encode", "compile"] {
        let f = named(&fs, name);
        assert!(holes(f).contains(&"call".to_owned()), "{name}: {f}");
    }
    Ok(())
}
