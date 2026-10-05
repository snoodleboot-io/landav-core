//! `LAN-109`: the builtin exception hierarchy is complete, and a user exception
//! of the same shape still gets no row.
//!
//! The pack admitted `ValueError`, `TypeError` and a dozen others as constant
//! construction but stopped short of the rest of the hierarchy — an accident of
//! curation, since every builtin exception (and warning) constructs the same
//! O(1) way. This pins that completing it resolves the newcomers while the
//! soundness boundary holds: a user-defined exception has a user-defined
//! `__init__`, so its name is bound in the module and the row is disqualified.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_its::Construct;

/// Whether raising `exc` with a bare-name argument leaves a `call` hole.
fn raising_holes(exc: &str, defined_locally: bool) -> bool {
    let definition = if defined_locally {
        format!("class {exc}(Exception):\n    pass\n\n\n")
    } else {
        String::new()
    };
    let source = format!("{definition}def g(path) -> int:\n    raise {exc}(path)\n    return 0\n");
    let functions = landav_python::lower_module(Path::new("exc.py"), &source)
        .unwrap_or_else(|error| panic!("failed to translate:\n{source}\n{error}"));
    let function = functions
        .iter()
        .find(|it| it.name() == "g")
        .unwrap_or_else(|| panic!("no `g` in:\n{source}"));
    match landav_its::lower(function.program()) {
        Ok(_) => false,
        Err(error) => error.refusals().is_some_and(|ledger| {
            ledger
                .as_slice()
                .iter()
                .any(|record| record.construct() == Construct::Call)
        }),
    }
}

/// **The newly-admitted builtin exceptions resolve.**
///
/// A sample across the hierarchy: an `OSError` subclass, a `UnicodeError`, an
/// `ImportError`, a `Warning`, and a `BaseException`-direct control-flow one.
/// Each is constant construction over its already-evaluated argument.
#[test]
fn a_builtin_exception_constructor_is_declared() {
    for exc in [
        "FileNotFoundError",
        "PermissionError",
        "UnicodeError",
        "ImportError",
        "ModuleNotFoundError",
        "RecursionError",
        "TimeoutError",
        "ConnectionResetError",
        "RuntimeWarning",
        "DeprecationWarning",
        "KeyboardInterrupt",
        "StopAsyncIteration",
    ] {
        assert!(
            !raising_holes(exc, false),
            "`{exc}` is a builtin exception, constant construction like every \
             other in the pack, and must resolve rather than hole"
        );
    }
}

/// **A user exception of the same name gets no row.**
///
/// The soundness boundary: `class TimeoutError(Exception): ...` binds the name
/// in the module, so the pack's `TimeoutError` row is disqualified by the
/// shadowing rule and the constructor stays a hole — its `__init__` is user
/// code the pack knows nothing about.
#[test]
fn a_shadowed_exception_name_gets_no_row() {
    for exc in ["TimeoutError", "ValueError", "ConnectionError"] {
        assert!(
            raising_holes(exc, true),
            "`{exc}` is shadowed by a local `class {exc}`, so the builtin row \
             must not apply — the local `__init__` is user code"
        );
    }
}
