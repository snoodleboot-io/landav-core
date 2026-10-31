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
//! Measured, not assumed, and the measurement is now history: over the
//! Python 3.12 standard library `rustpython-parser 0.4.0` read 564 of the 567
//! files landav collects. The three it could not read -
//! `test/support/socket_helper.py`, `traceback.py` and `turtle.py` - failed
//! for the *same* reason, a PEP 701 f-string reusing the enclosing quote.
//! `LAN-130` swapped the parser and all three read, taking that corpus from
//! 12,760 functions to 13,039:
//!
//! | file | functions recovered |
//! |---|---:|
//! | `turtle.py` | 214 |
//! | `traceback.py` | 54 |
//! | `test/support/socket_helper.py` | 11 |
//!
//! `LAN-82` priced the same three files at 51 functions. Both numbers are
//! right: 51 counted module-level `def`s, which was all the lowering produced
//! before `LAN-104` reached methods. The *direction* was the argument for
//! paying, and it held - PEP 701 is what a formatter now emits, so the cost of
//! not swapping was rising on its own. Three unreadable files reported as
//! three problems is honest; three unreadable files whose functions vanish
//! from a coverage percentage is not.
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
//! # The swap this file was written to force, and why nothing is `#[ignore]`d
//!
//! `LAN-82` left seven tests `#[ignore]`d rather than deleted, each naming the
//! syntax it needed and the reason it could not have it: `rustpython-parser
//! 0.4.0` was already the newest release that would ever exist, the
//! alternative was a `0.0.x` crate, and the measured cost of not swapping was
//! three stdlib files. A *priced* deferral.
//!
//! `LAN-130` paid it. `ruff_python_parser` reads all five PEP 701 forms, so
//! those five came off `#[ignore]` and [`UNREAD_AT_SUPPORTED_LEVEL`] emptied.
//! The last two were PEP 696 (3.13), deferred for a different reason by then:
//! the parser read that syntax and the *declared level* was still 3.12, so it
//! was refused **by version** rather than by capability. `LAN-132` raised the
//! claim to 3.15 and took them off. **No test in this file is `#[ignore]`d.**
//!
//! That distinction is still the one this file exists to keep honest. The
//! parser reports syntax above the target separately from malformed source,
//! and `analysis::parse_guarded` folds both into one refusal, so a form being
//! "unread" can mean either "nothing can parse this" or "we do not claim this
//! version yet" - and the second must never be mistaken for the first.
//!
//! # There is no longer any Python above the declared level
//!
//! 3.15 is the newest released Python *and* the newest its parser knows, so
//! the second of those two classes is **empty**: no well-formed file exists
//! that this build is too old to read. That is asserted rather than assumed -
//! `landav-python`'s `the_declared_level_is_the_newest_python_the_parser_knows`
//! fails the moment the parser learns 3.16, which is the signal to pick a
//! 3.16-only form and put the class back. Until then the only parse refusal
//! left is source that is not Python at any version, and
//! `a_file_that_is_not_python_at_any_version_still_refuses_with_a_position`
//! below is what stands for it.
//!
//! # What 3.13, 3.14 and 3.15 cost: nothing measurable, which is still worth
//! asserting
//!
//! Raising the claim three releases moved no bound on either corpus, because
//! neither corpus contains the syntax. The value is not in today's numbers; it
//! is that a file using the new forms is now *read* and its functions
//! *counted* rather than dropped before the first `def`. Two of the forms
//! needed a decision rather than a parser:
//!
//! - a t-string is read and then refused as a named construct, because it
//!   builds a `Template` rather than a `str`;
//! - `lazy import` **removes** a cost row, because a lazily bound name pays
//!   its module's import on first read. That one is a soundness fix, and
//!   `a_lazy_import_does_not_resolve_a_module_attribute_row` below is the
//!   pin.
//!
//! # What "supported version" means here
//!
//! Not a string in a README. The level is whatever set of constructs the
//! assertions below agree reads, and a form that is deliberately *not* read is
//! pinned as such with a reason.
//!
//! [`landav_python::SUPPORTED_PYTHON`] and its exception list state that level
//! in one place, because a build that reads less Python than its release notes
//! claim is a wrong build and not merely a red suite. A constant alone would
//! be worthless — it cannot be exceeded by the parser, and a claim that cannot
//! be wrong is not a contract — so the *stated level* section at the end of
//! this file pins it from both sides: a sample per release that must read, so
//! the number cannot be raised on paper, and a sample per published exception
//! that must still refuse, so it cannot be left overstating the gap. The
//! constant is the statement; those two tests are what make it falsifiable.

// See `common/mod.rs` for why the panic lints are relaxed in test code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_its::Construct;
use landav_python::{PythonError, SUPPORTED_PYTHON, UNREAD_AT_SUPPORTED_LEVEL};

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

/// The constructs the function called `name` refuses.
///
/// For the cases where "the file reads" is only half the claim. A form can be
/// read and still be outside the analysable fragment, and the two outcomes
/// are not comparable: the first costs one node and shows up by name, the
/// second costs every function in the file and shows up nowhere.
fn constructs_of(source: &str, name: &str) -> Vec<Construct> {
    let functions = landav_python::lower_module(Path::new("level.py"), source)
        .unwrap_or_else(|error| panic!("expected this to parse:\n{source}\n{error}"));
    let function = functions
        .iter()
        .find(|function| function.name() == name)
        .unwrap_or_else(|| panic!("no function `{name}` was lowered from:\n{source}"));
    function
        .program()
        .unsupported_nodes()
        .map(|node| node.construct())
        .collect()
}

/// The frontend refuses `source`, and says where.
///
/// Since `LAN-132` there is exactly one thing this is for: source that is not
/// Python at any version. The *other* reason a file used to be refused - being
/// written in a Python newer than the declared level - cannot happen while the
/// level is the newest release there is. The position is asserted alongside
/// the refusal because a parse error with no position is indistinguishable
/// from a crash to the operator reading it.
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
// that distinguishes "reads 3.12" from "reads 3.13", and a version claim with
// no test behind it is the drift this file exists to stop.
//
// Both tests ran under `#[ignore]` from `LAN-82` to `LAN-132`, and the reason
// changed once along the way: first the parser could not read this, then it
// could and the declared level was still 3.12. `LAN-132` raised the level, so
// they run.

/// **A generic function may give its type parameter a default.**
///
/// PEP 695's `def f[T](...)` already reads; this is the same grammar with
/// `= int` after the parameter. The narrowness is the point: it is one token,
/// and it is the entire syntactic difference between the two versions.
#[test]
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
// PEP 758 and PEP 750 - the 3.14 line
// ---------------------------------------------------------------------------
//
// Both of these were pinned as *deferred* while the declared level was 3.12,
// each with its reason written down: the syntax cost nothing to omit, and
// reaching 3.14 was to be a decision somebody made and recorded rather than
// something a dependency bump did on its own. `LAN-132` is that decision. The
// parser has read both since `LAN-130`; what changes here is the claim.

/// **An unparenthesised tuple of exception types reads, and lowers to exactly
/// what the parenthesised spelling lowers to (PEP 758).**
///
/// The two programs mean the same thing, so the assertion is equivalence
/// rather than a named construct: whatever this frontend makes of a `try`, it
/// must make the same of both spellings. That is stronger than naming a
/// refusal and it cannot rot - if the handling of `try` changes, both sides
/// move together and this still holds, while a hard-coded construct name
/// would have to be chased.
///
/// Measured while writing it: in this shape neither spelling refuses the
/// handler at all, and the only refusal is the `go(n)` call. A test asserting
/// `exceptional-control-flow` here would have been asserting something false
/// about both.
#[test]
fn an_unparenthesised_multi_except_reads() {
    let bare = "\
def run(n: int) -> int:
    try:
        go(n)
    except ValueError, TypeError:
        pass
    return n
";
    let parenthesised = "\
def run(n: int) -> int:
    try:
        go(n)
    except (ValueError, TypeError):
        pass
    return n
";
    assert_reads(bare, "run");
    assert_eq!(
        constructs_of(bare, "run"),
        constructs_of(parenthesised, "run"),
        "PEP 758 is a spelling of the parenthesised form, so the two must \
         lower alike; a difference here means one of them is being accounted \
         for and the other is not"
    );
}

/// **A template string reads, and is then refused by name (PEP 750).**
///
/// The *shape* of this assertion is the point, and it is the shape that
/// changed. While the level was 3.12 a `t"..."` was a parse refusal, so this
/// test asserted that the whole file was lost. At 3.15 it parses: the file is
/// read, the function is counted, and the t-string is an ordinary named
/// refusal inside it. That is exactly what raising the level buys - a
/// construct outside the fragment costs one node and appears by name, a
/// construct outside the parser costs every function in the file and appears
/// nowhere.
///
/// It is pinned in a *condition* because that is where the construct is
/// reachable. `message = t"..."` refuses one level earlier as
/// `non-integer-value` naming the local it rebinds (`LAN-100`), precisely as
/// an f-string assignment does; a value's own kind is only asked for where an
/// integer or a truth value is demanded. Measured, not assumed - in that
/// position an f-string refuses as `collection`, and the whole reason
/// [`Construct::TemplateString`] exists is that a `Template` is not a string.
#[test]
fn a_template_string_reads_and_is_refused_as_a_construct() {
    let source = "\
def label(name) -> int:
    if t\"hello {name}\":
        return 1
    return 0
";
    assert_reads(source, "label");
    let refused = constructs_of(source, "label");
    assert!(
        refused.contains(&Construct::TemplateString),
        "a t-string must be refused under its own name, not as a string \
         value: got {refused:?}"
    );
}

// ---------------------------------------------------------------------------
// PEP 810 and PEP 798 - the 3.15 line
// ---------------------------------------------------------------------------
//
// 3.15 is the newest released Python and the newest the parser knows, so this
// is the top of the file in both senses. One of the three forms below is not
// a coverage matter at all: `lazy import` changes what a cost row may claim,
// and that rule is pinned where the rows live, in `module_rows.rs`.

/// **A `lazy import` reads, and the module after it is not lost (PEP 810).**
///
/// The assertion is on the `def` below the import rather than on the import
/// itself, for the reason the PEP 696 class case gives: an import lowers to no
/// function of its own, so a parser that stops at line 1 takes the rest of the
/// module with it and nothing says so.
///
/// What a lazy import *costs* is a separate question with a separate answer:
/// a name bound this way pays its module's import on first read, so it must
/// not resolve against a `module.attr` cost row. That is a soundness rule, and
/// `module_rows.rs` holds it.
#[test]
fn a_lazy_import_does_not_lose_the_module() {
    let source = "\
lazy import os
lazy from threading import Lock


def label(n: int) -> int:
    total = 0
    for i in range(n):
        total = total + 1
    return total
";
    assert_reads(source, "label");
}

/// **A starred element in a comprehension reads (PEP 798).**
///
/// `[*row for row in rows]` was a syntax error before 3.15 and is now the
/// comprehension spelling of what `[*a, *b]` already did in a display. It
/// stays refused as `comprehension` - this ticket is about reading the
/// grammar, not about widening the fragment - and the refusal being *placed*
/// is what distinguishes that from losing the file.
#[test]
fn a_starred_element_in_a_comprehension_reads() {
    let source = "\
def label(rows) -> int:
    items = [*row for row in rows]
    return len(items)
";
    assert_reads(source, "label");
    assert!(
        constructs_of(source, "label").contains(&Construct::Comprehension),
        "the comprehension must still be refused by name"
    );
}

/// **A unary-plus pattern reads.**
///
/// `case +1:` is 3.15 grammar; before it, only `-1` was allowed as a pattern
/// literal, which is the kind of asymmetry a reader never guesses at. Still
/// `pattern-match`, and emphatically not a parse error - a `match` statement
/// is usually most of a function, so losing the file to one `case` line is
/// the expensive failure.
#[test]
fn a_unary_plus_pattern_reads() {
    let source = "\
def label(value) -> int:
    match value:
        case +1:
            return 1
        case _:
            return 0
";
    assert_reads(source, "label");
    assert!(
        constructs_of(source, "label").contains(&Construct::PatternMatch),
        "the match must still be refused by name"
    );
}

// ---------------------------------------------------------------------------
// deliberately not claimed
// ---------------------------------------------------------------------------
//
// **This section is empty of version deferrals, and that is a measured claim
// rather than an omission.** It held two: PEP 758 and PEP 750, both 3.14, both
// now read. Nothing replaced them because nothing can - 3.15 is the newest
// released Python and the newest the parser knows, so there is no well-formed
// Python file this build is too old to read. `landav-python`'s
// `the_declared_level_is_the_newest_python_the_parser_knows` is the tripwire
// that fails when that stops being true.
//
// What remains is the other class of refusal, which no version claim can ever
// empty: source that is not Python at all.

/// **A file that is not Python at any version is still refused, with a
/// position.**
///
/// The last parse refusal left, and the one that must survive every future
/// level raise. Python 2 `print` syntax is chosen because it will never start
/// parsing as Python 3, so this case cannot quietly change what it tests - the
/// failing-for-the-wrong-reason trap that the above-level fixtures fell into
/// the moment the frontend caught up with them.
///
/// The position is the substance of the assertion. A whole file dropped is the
/// most expensive refusal this frontend can make, and an operator who cannot
/// see which line caused it cannot act on it.
#[test]
fn a_file_that_is_not_python_at_any_version_still_refuses_with_a_position() {
    assert_refuses(
        "\
def ok(n):
    print \"this module was written for Python 2\"
    return n
",
        "no Python 3 parser will ever accept a `print` statement, so this \
         stands for the one class of refusal a version claim cannot empty",
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

// ---------------------------------------------------------------------------
// the stated level
// ---------------------------------------------------------------------------
//
// `LAN-82` asks for the supported version to be stated in one place and
// asserted by a test. The statement is [`landav_python::SUPPORTED_PYTHON`] and
// its exception list, and this section is the assertion — the half that stops
// the statement from being decoration.
//
// A version constant read on its own is unfalsifiable: nothing about `"3.12"`
// can be contradicted by a parser. What the two tests below do is pin it from
// both sides. The samples say the frontend reads *at least* the stated
// version, so the number cannot be raised without teaching the frontend the
// syntax first; the exception list says the frontend reads no more than it
// admits to, so a form that quietly starts parsing forces the list to shrink.
// Between them the claim can be wrong, which is the only reason to publish it.

/// One version-defining construct per release, and the release that added it.
///
/// Each source must lower a function called `label`, so that a sample proves
/// the construct reached the bound engine rather than merely parsed — the
/// distinction the whole file is built on.
const AT_LEVEL: &[(&str, &str, &str)] = &[
    (
        "3.10",
        "a `match` statement",
        "\
def label(value) -> int:
    match value:
        case 1:
            return 1
        case _:
            return 0
",
    ),
    (
        "3.11",
        "an `except*` handler",
        "\
def label(value) -> int:
    try:
        go(value)
    except* ValueError:
        return 0
    return 1
",
    ),
    (
        "3.12",
        "a PEP 695 type parameter list",
        "\
def label[T](value: T) -> int:
    return 1
",
    ),
    (
        "3.12",
        "a PEP 695 `type` alias",
        "\
type Row = tuple[int, int]


def label(row) -> int:
    return 1
",
    ),
    (
        "3.13",
        "a PEP 696 default on a type parameter",
        "\
def label[T = int](value: T) -> int:
    return 1
",
    ),
    (
        "3.14",
        "a PEP 758 unparenthesised `except` tuple",
        "\
def label(n: int) -> int:
    try:
        go(n)
    except ValueError, TypeError:
        pass
    return n
",
    ),
    (
        "3.14",
        "a PEP 750 template string",
        "\
def label(name) -> int:
    message = t\"hello {name}\"
    return 1
",
    ),
    (
        "3.15",
        "a PEP 810 `lazy import`",
        "\
lazy import os


def label(n: int) -> int:
    return 1
",
    ),
    (
        "3.15",
        "a PEP 798 starred element in a comprehension",
        "\
def label(rows) -> int:
    items = [*row for row in rows]
    return 1
",
    ),
];

/// One sample per entry of [`UNREAD_AT_SUPPORTED_LEVEL`], keyed by its name.
///
/// The names are compared for equality with the published list, so an entry
/// added there without a sample here — or a sample here naming nothing — is a
/// failure rather than a silently untested claim.
/// The samples behind [`UNREAD_AT_SUPPORTED_LEVEL`], which is **empty** since
/// `LAN-130`.
///
/// It held the five PEP 701 f-string forms, each sampled here so the published
/// claim could not drift from what the parser actually did. Every one now
/// reads, so both this table and the constant are empty, and the five tests
/// above that assert they read are no longer `#[ignore]`d - which is the same
/// fact asserted from the other side.
///
/// A form goes back in here the moment one at the declared level turns out
/// unreadable, paired with its entry in the constant.
const UNREAD: &[(&str, &str)] = &[];

/// `"3.12"` as `(3, 12)`, so releases order by number and not by string.
///
/// `"3.9"` sorts after `"3.12"` as text, which would let the claim be lowered
/// by a sample that looks like an addition.
fn release(text: &str) -> (u32, u32) {
    let (major, minor) = text
        .split_once('.')
        .unwrap_or_else(|| panic!("a release is `major.minor`, got {text:?}"));
    let parsed = |part: &str| {
        part.parse::<u32>()
            .unwrap_or_else(|_| panic!("a release part is a number, got {part:?} in {text:?}"))
    };
    (parsed(major), parsed(minor))
}

/// **The stated version is the highest one the frontend actually reads.**
///
/// Both directions are the assertion. Every sample must read, so the claim
/// cannot be above the frontend; and the highest sample must equal the claim,
/// so the claim cannot be below it either — a construct that starts reading
/// is only worth having if the published number moves with it.
#[test]
fn the_stated_level_is_the_highest_release_that_reads() {
    for (added, construct, source) in AT_LEVEL {
        assert!(
            release(added) <= release(SUPPORTED_PYTHON),
            "{construct} arrived in {added}, above the stated \
             {SUPPORTED_PYTHON}. Either the sample belongs in the deferred \
             section or `SUPPORTED_PYTHON` is out of date"
        );
        assert_reads(source, "label");
    }

    let highest = AT_LEVEL
        .iter()
        .map(|(added, ..)| release(added))
        .max()
        .expect("the table is not empty");

    assert_eq!(
        highest,
        release(SUPPORTED_PYTHON),
        "`SUPPORTED_PYTHON` is {SUPPORTED_PYTHON}, but the newest construct \
         proved to read here is from {highest:?}. A version claim is only \
         worth publishing while a sample stands behind it: add the construct \
         that defines the newer release, or lower the constant"
    );
}

/// **Every published exception is still unread, and every one is sampled.**
///
/// This is the test a parser swap was meant to break, and `LAN-130` broke it:
/// the five PEP 701 forms all parse now, so both the published list and the
/// samples are empty and the loop below runs zero times. That is not a dead
/// test - it is the assertion that the list has not quietly refilled, and it
/// fails the moment a published entry has no sample or an unread sample has no
/// entry. Names are compared as a set so neither side can drift alone.
#[test]
fn every_unread_form_is_sampled_and_still_unread() {
    let mut published: Vec<&str> = UNREAD_AT_SUPPORTED_LEVEL.to_vec();
    let mut sampled: Vec<&str> = UNREAD.iter().map(|(name, _)| *name).collect();
    published.sort_unstable();
    sampled.sort_unstable();
    assert_eq!(
        published, sampled,
        "`UNREAD_AT_SUPPORTED_LEVEL` and the samples here name different \
         forms. An entry with no sample is an untested claim about the gap"
    );

    for (name, source) in UNREAD {
        match lowered_names(source) {
            Ok(names) => panic!(
                "{name} now parses (functions {names:?}), so the frontend \
                 reads more than `UNREAD_AT_SUPPORTED_LEVEL` admits. Drop the \
                 entry, un-`#[ignore]` the test above that asserts it reads, \
                 and re-measure the stdlib file count in this file's \
                 header:\n{source}"
            ),
            Err(PythonError::Parse { line, column, .. }) => assert!(
                line >= 1 && column >= 1,
                "{name} is refused without a usable position ({line}:{column}), \
                 so an operator cannot find the file's first bad line"
            ),
            Err(other) => panic!("expected a parse refusal for {name}, got {other}"),
        }
    }
}
