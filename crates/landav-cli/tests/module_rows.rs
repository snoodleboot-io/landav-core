//! `LAN-128`: **a callee reached through an imported module name resolves
//! against a `module.attr` row.**
//!
//! `os.getcwd()` costs one syscall whatever module calls it, and the pack can
//! say so - but only under a key that means something: `os` has to be the
//! name an absolute import bound, once, and never rebound in the function. A
//! name with any other meaning gets no row, exactly as a bare builtin name
//! that the module shadows gets none (`LAN-94`).

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
