//! `LAN-82` acceptance: **the Python the frontend can read, as a contract.**
//!
//! # The measurement this exists to move
//!
//! A construct outside the analysable fragment costs one *function*: it lowers
//! to an `Unsupported` node, the function still appears in the denominator, and
//! a reader can see the refusal by name. A construct outside the **parser**
//! costs the whole *file*. `PythonError::Parse` aborts before a single `def` is
//! seen, so those functions never enter the denominator, never surface as a
//! refusal, and never count as uncovered. Coverage does not drop — it silently
//! shrinks its own base. That is the failure mode `LAN-68` exists to prevent,
//! reappearing one level lower down.
//!
//! Measured, not assumed. Over the Python 3.12 standard library the pinned
//! parser reads 567 of 570 files. The three it cannot read —
//! `test/support/socket_helper.py`, `traceback.py` and `turtle.py` — fail for
//! the *same* reason, a PEP 701 f-string reusing the enclosing quote, and take
//! 51 top-level functions with them. Over momentum's backend (7,689 files,
//! running on CPython 3.14) it reads every one.
//!
//! Fifty-one functions is not a crisis, and this file does not pretend
//! otherwise. What makes the gap worth closing is its direction: PEP 701 is
//! what a formatter now *emits*, so the count only goes up, and the reader is
//! never told. Three unreadable files reported as three problems is honest;
//! three unreadable files whose functions vanish from a coverage percentage is
//! not.
//!
//! # Why this file, when `landav-python`'s `parser_boundary.rs` exists
//!
//! That file asks whether [`landav_python::analyze_source`] — the *pattern
//! rule* path — accepts a construct. This one asks the question the bound
//! engine depends on: does [`landav_python::lower_module`] hand back a
//! function? The two paths walk the same tree with different code, and the
//! whole of `LAN-82` is a change of parser. A swap that keeps the rules
//! running while `lowering.rs`'s `Stmt::FunctionDef` arm stops matching would
//! pass `parser_boundary.rs` and empty every bound in the product. The
//! regression tests below are here to make that impossible to ship quietly.
//!
//! # Seven of these are `#[ignore]`d, deliberately, and stay runnable
//!
//! The parser is **not** being swapped in this change, and the seven tests that
//! need a newer one carry `#[ignore]` naming the syntax and this decision
//! rather than being deleted or weakened. `cargo test -- --ignored` runs them,
//! so the gap stays measurable: the day a parser reads them, they go green and
//! the attribute comes off.
//!
//! The decision, on the record so it is not re-litigated by whoever next reads
//! a red suite:
//!
//! * `rustpython-parser 0.4.0` is already the newest published release, from
//!   August 2024. The version bump this ticket imagines does not exist.
//! * The alternative, `ruff_python_parser`, is a `0.0.x` crate. Swapping means
//!   rewriting four files against a different AST, not changing a version.
//! * The measured cost of *not* swapping is three stdlib files and 51
//!   top-level functions, and zero on the other corpus.
//!
//! An `#[ignore]` here is therefore a *priced* deferral, not a silenced test.
//! The eight assertions around them are not ignored and must not become so:
//! they are the regression guards that a parser swap has to survive.
//!
//! # What "supported version" means here
//!
//! Not a string in a README. The level is whatever set of constructs the
//! assertions below agree reads, and a form that is deliberately *not* read is
//! pinned as such with a reason. Adding a version constant somewhere in `src/`
//! is welcome, but it is a restatement of these tests and not a substitute for
//! them: a constant cannot be exceeded by the parser, and a claim that cannot
//! be wrong is not a contract.

// See `common/mod.rs` for why the panic lints are relaxed in test code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_python::PythonError;

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

/// The names of the top-level functions `source` lowers to, or the parse error.
///
/// Both halves matter. `Ok(vec![])` means "read the file, found no function",
/// which is a legitimate answer for a module of type aliases and a *bug* for a
/// module holding a `def` — and `Err` means the file was never read at all.
/// Collapsing the two into a boolean is exactly the conflation this ticket is
/// about.
fn lowered_names(source: &str) -> Result<Vec<String>, PythonError> {
    landav_python::lower_module(Path::new("level.py"), source).map(|functions| {
        functions
            .iter()
            .map(|function| function.name().to_owned())
            .collect()
    })
}

/// **The** assertion: the frontend reads `source` and gets `name` out of it.
///
/// One helper rather than two assertions per test, because "the parser refused
/// the file" and "the parser read the file and the lowering dropped the
/// function" are different bugs with the same symptom — no bound — and a test
/// that cannot tell them apart sends the next reader to the wrong crate.
fn assert_reads(source: &str, name: &str) {
    match lowered_names(source) {
        Err(error) => panic!(
            "the frontend cannot parse this, so the whole file is lost - not \
             one function, all of them, and none of them appear as a refusal \
             either:\n{source}\n{error}"
        ),
        Ok(names) => assert!(
            names.iter().any(|found| found == name),
            "the file parsed but `{name}` did not reach the bound engine, so \
             it silently left the coverage denominator: got {names:?} \
             for:\n{source}"
        ),
    }
}

/// The frontend refuses `source`, and says where.
///
/// Used only for forms this file has decided **not** to claim yet. The
/// position is asserted alongside the refusal because a parse error with no
/// position is indistinguishable from a crash to the operator reading it, and
/// the whole value of a deferred limit is that a reader can see it.
fn assert_refuses(source: &str, why: &str) {
    match lowered_names(source) {
        Ok(names) => panic!(
            "this now parses (functions {names:?}), which is good news and \
             makes this test wrong. It was pinned as deferred because {why} - \
             promote it to an `assert_reads` case and delete this \
             one:\n{source}"
        ),
        Err(PythonError::Parse { line, column, .. }) => assert!(
            line >= 1 && column >= 1,
            "a refusal must carry a 1-based position or the operator cannot \
             find it, got {line}:{column} for:\n{source}"
        ),
        Err(other) => panic!(
            "expected a parse refusal naming a position, got {other} \
             for:\n{source}"
        ),
    }
}

// ---------------------------------------------------------------------------
// PEP 701 - the whole of the measured loss
// ---------------------------------------------------------------------------
//
// Every file the pinned parser loses on the 3.12 stdlib is one of these. The
// pre-701 grammar forbade the enclosing quote inside a replacement field, so
// `f"{d["k"]}"` had to be written `f"{d['k']}"`; 3.12 lifted the restriction
// and formatters now emit the natural spelling. Nothing about these programs
// is unusual - they are what `black` produces.

/// **The exact syntax that costs three stdlib files.**
///
/// `turtle.py` line 3978 and `socket_helper.py` line 306 are this shape. It is
/// the single highest-value case in the file: without it, a Python 3.12
/// codebase loses files at a rate that grows every time somebody reformats.
#[test]
#[ignore = "LAN-82 open: PEP 701 same-quote replacement field (f\"{row[\"k\"]}\"). rustpython-parser 0.4.0 predates it and no newer release exists; run with --ignored to measure the gap"]
fn an_f_string_may_reuse_the_enclosing_quote() {
    let source = r#"
def label(row) -> int:
    text = f"{row["name"]}"
    return len(text)
"#;
    assert_reads(source, "label");
}

/// **A replacement field may hold a nested f-string in the same quotes.**
///
/// `traceback.py` line 745 is this shape: an `f'...'` inside an `f'...'`. The
/// alternate-quote spelling already reads, which is what makes this a parser
/// artefact rather than a limit — the two programs mean the same thing and
/// only one of them is accepted.
#[test]
#[ignore = "LAN-82 open: PEP 701 f-string nested inside an f-string in the same quotes. rustpython-parser 0.4.0 predates it and no newer release exists; run with --ignored to measure the gap"]
fn an_f_string_may_nest_an_f_string_in_the_same_quotes() {
    let source = r#"
def label(row) -> int:
    text = f"{f"{row}"}"
    return len(text)
"#;
    assert_reads(source, "label");
}

/// **A replacement field may span lines.**
///
/// PEP 701 made the expression inside `{}` ordinary Python, which includes
/// being wrapped by a formatter when it grows past the line length. A long
/// f-string is precisely the one a formatter will break, so this is not the
/// rare case it looks like.
#[test]
#[ignore = "LAN-82 open: PEP 701 multi-line replacement field. rustpython-parser 0.4.0 predates it and no newer release exists; run with --ignored to measure the gap"]
fn an_f_string_expression_may_span_lines() {
    let source = r#"
def label(row) -> int:
    text = f"{
        row.name
    }"
    return len(text)
"#;
    assert_reads(source, "label");
}

/// **A multi-line replacement field may carry a comment.**
///
/// The consequence of the previous case rather than a separate feature: once
/// the expression is ordinary Python spread over lines, `#` means what it
/// means everywhere else. Pinned separately because a parser can support
/// line-spanning fields and still choke on the comment, and that partial
/// support would read as a fix.
#[test]
#[ignore = "LAN-82 open: PEP 701 comment inside a multi-line replacement field. rustpython-parser 0.4.0 predates it and no newer release exists; run with --ignored to measure the gap"]
fn an_f_string_expression_may_carry_a_comment() {
    let source = r#"
def label(row) -> int:
    text = f"{
        row.name  # the display name
    }"
    return len(text)
"#;
    assert_reads(source, "label");
}

/// **A format spec may reuse the enclosing quote too.**
///
/// The nested-field case, one level further in. Worth its own test because the
/// format spec is lexed by a different path in every parser that has one, so a
/// fix to the replacement field routinely leaves this behind.
#[test]
#[ignore = "LAN-82 open: PEP 701 same-quote nested field inside a format spec. rustpython-parser 0.4.0 predates it and no newer release exists; run with --ignored to measure the gap"]
fn an_f_string_format_spec_may_reuse_the_enclosing_quote() {
    let source = r#"
def label(row) -> int:
    text = f"{row:{row["width"]}}"
    return len(text)
"#;
    assert_reads(source, "label");
}

// ---------------------------------------------------------------------------
// PEP 696 - the 3.13 line
// ---------------------------------------------------------------------------
//
// 3.13 added exactly one piece of syntax: a default on a type parameter. It
// appears nowhere in either corpus today, so unlike PEP 701 it costs nothing
// measurable right now. It is asserted anyway, because it is the *only* thing
// that distinguishes "reads 3.12" from "reads 3.13", and the ticket asks for
// 3.13+. A version claim with no test behind it is the drift this file exists
// to stop.

/// **A generic function may give its type parameter a default.**
///
/// PEP 695's `def f[T](...)` already reads; this is the same grammar with
/// `= int` after the parameter. The narrowness is the point: it is one token,
/// and it is the entire syntactic difference between the two versions.
#[test]
#[ignore = "LAN-82 open: PEP 696 default on a type parameter (3.13). rustpython-parser 0.4.0 predates it and no newer release exists; run with --ignored to measure the gap"]
fn a_type_parameter_may_have_a_default() {
    let source = "\
def make[T = int](value: T) -> T:
    return value
";
    assert_reads(source, "make");
}

/// **A generic class and a type alias may too, and a `def` after them is
/// still found.**
///
/// A class and an alias lower to no function of their own, so the assertion is
/// on the `def` that follows: a parser that stops at line 1 loses the rest of
/// the module. That is the file-scale failure this ticket is about, reproduced
/// in five lines.
#[test]
#[ignore = "LAN-82 open: PEP 696 defaulted type parameter on a class or alias (3.13). rustpython-parser 0.4.0 predates it and no newer release exists; run with --ignored to measure the gap"]
fn a_defaulted_type_parameter_on_a_class_or_alias_does_not_lose_the_module() {
    let source = "\
type Pair[T = int] = tuple[T, T]


class Box[T = int]:
    pass


def unbox(box) -> int:
    return 1
";
    assert_reads(source, "unbox");
}

// ---------------------------------------------------------------------------
// already read, and must survive the parser change
// ---------------------------------------------------------------------------
//
// Every one of these parses today. They are here because closing the gap above
// means changing parsers, and a parser change is the one edit that can take
// them away - silently, because losing them looks like a coverage number
// moving rather than like a test failing. `parser_boundary.rs` guards the rule
// path; these guard the lowering path, which is where a bound comes from.

/// **A PEP 695 generic `def` still reaches the bound engine.**
///
/// The most likely casualty of a parser swap: type parameters change the shape
/// of the function node, and a lowering that matches on that node can stop
/// matching without any error anywhere.
#[test]
fn a_generic_function_still_produces_a_lowered_function() {
    let source = "\
def walk[T](items: list[T], n: int) -> int:
    total = 0
    for i in range(n):
        total = total + 1
    return total
";
    assert_reads(source, "walk");
}

/// **`except*` still reaches the bound engine.**
///
/// 3.11 exception groups. A distinct statement node from `except`, so a parser
/// that models it differently is a real risk to the lowering's `try` arm.
#[test]
fn an_except_star_handler_still_produces_a_lowered_function() {
    let source = "\
def run(n: int) -> int:
    try:
        go(n)
    except* ValueError:
        pass
    return n
";
    assert_reads(source, "run");
}

/// **`match` still reaches the bound engine.**
///
/// The largest grammar addition since 3.9 and the one with the most nodes of
/// its own. Named in the ticket as syntax that will spread through real code.
#[test]
fn a_match_statement_still_produces_a_lowered_function() {
    let source = "\
def route(command, n: int) -> int:
    match command:
        case {'op': op}:
            return n
        case [head, *tail]:
            return 1
        case _:
            return 0
";
    assert_reads(source, "route");
}

/// **PEP 646 and PEP 692 annotations still reach the bound engine.**
///
/// A starred type parameter and `**kwargs: Unpack[...]` are annotation-only,
/// so nothing about the body changes — which is why a regression here would be
/// invisible in every other test. Grouped into one case because they fail
/// together or not at all: both are a `*` where an older grammar allowed none.
#[test]
fn variadic_and_unpacked_annotations_still_produce_a_lowered_function() {
    let source = "\
def shape[*Ts](sizes: tuple[*Ts], n: int, **options: Unpack[Options]) -> int:
    total = 0
    for i in range(n):
        total = total + 1
    return total
";
    assert_reads(source, "shape");
}

/// **An f-string with a backslash in the replacement field still reads.**
///
/// Also PEP 701, and — measured — the one part of it the pinned parser already
/// accepts. It is pinned as *working* so that a parser swap cannot trade one
/// half of the PEP for the other and be recorded as progress.
#[test]
fn a_backslash_inside_a_replacement_field_still_reads() {
    let source = "\
def join(rows: list) -> int:
    text = f\"{'\\n'.join(rows)}\"
    return len(text)
";
    assert_reads(source, "join");
}

// ---------------------------------------------------------------------------
// deliberately not claimed
// ---------------------------------------------------------------------------

/// **Deferred: PEP 758, `except A, B:` without parentheses (3.14).**
///
/// Not a gap — a limit, and the difference is that this one is chosen. The
/// declared level is 3.13, this is 3.14-only syntax, and it appears zero times
/// across both corpora (momentum runs *on* 3.14 and has not written one). It
/// is also cost-free to omit: the parenthesised spelling means the same thing,
/// is still valid, and is what every existing file contains.
///
/// Pinned rather than left untested so that reaching 3.14 is a decision
/// somebody makes and records here, not something a dependency bump does on
/// its own.
#[test]
fn an_unparenthesised_multi_except_is_deferred() {
    let source = "\
def run(n: int) -> int:
    try:
        go(n)
    except ValueError, TypeError:
        pass
    return n
";
    assert_refuses(
        source,
        "it is 3.14-only syntax, the declared level is 3.13, and the \
         parenthesised spelling it replaces is unaffected",
    );
}

/// **Deferred: PEP 750 template strings (3.14).**
///
/// Same argument as PEP 758 and a stronger one: a `t"..."` is not a string at
/// all but a `Template` object, so reading it would mean teaching the lowering
/// a new kind of value rather than accepting a new spelling of an old one.
/// That is fragment work, which `LAN-82` puts out of scope explicitly.
#[test]
fn a_template_string_is_deferred() {
    let source = "\
def greet(name) -> int:
    message = t\"hello {name}\"
    return len(message)
";
    assert_refuses(
        source,
        "it introduces a new runtime value rather than a new spelling, so \
         reading it is fragment work and LAN-82 puts that out of scope",
    );
}

// ---------------------------------------------------------------------------
// the hole a parser upgrade would not have closed, and no longer exists
// ---------------------------------------------------------------------------

/// **A top-level `async def` produces a lowered function.** `LAN-93`, closed.
///
/// This assertion used to run the other way, and said so: it pinned as current
/// behaviour that `lower_module` matched `Stmt::FunctionDef` and had no arm for
/// `Stmt::AsyncFunctionDef`, so an `async def` parsed cleanly and produced no
/// function at all — not refused, not holed, not counted, and so absent from
/// the denominator every coverage number on this project is computed over. It
/// belongs in this file because that is the *same* failure as an unreadable
/// file, a function that can be neither covered nor refused, reached by a
/// different route rather than by the parser. It also said that whoever closed
/// it had to come here and say so. This is that.
///
/// `lower_module` now recognises both node types through one borrowed view of
/// the parts they share. Measured over both corpora with nothing else changed:
///
/// | corpus | `functions` before | after |
/// |---|---|---|
/// | `/usr/lib/python3.12` | 3057 | 3072 (+15) |
/// | momentum's backend | 14952 | 15305 (+353) |
///
/// The 353 is 219 distinct functions counted twice, because that corpus's
/// `.venv` carries a `lib64 -> lib` symlink the directory walk follows. That is
/// a separate defect and has its own ticket; it is quoted here as what landav's
/// own walk sees, which is what the denominator is made of.
///
/// What the body of an async function *reports* — an `await` as a placed
/// `coroutine` hole, a counted loop around it still counted — is `LAN-93`'s
/// subject and is pinned in `async_functions.rs`. All that is pinned here is
/// that the function arrives at all.
#[test]
fn a_top_level_async_def_is_lowered() {
    let source = "\
async def drain(source, n: int) -> int:
    total = 0
    for i in range(n):
        total = total + 1
    return total
";
    let names = lowered_names(source).expect("`async def` parses - it always did");
    assert!(
        names.iter().any(|name| name == "drain"),
        "an `async def` produced no lowered function ({names:?}), which puts it \
         outside the coverage denominator rather than inside it as a \
         refusal:\n{source}"
    );
}
