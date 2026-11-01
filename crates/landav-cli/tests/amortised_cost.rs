//! `LAN-134`: **a cost that is only bounded when the calls are summed.**
//!
//! # The decision this file records
//!
//! `list.append` is O(n) in the worst case - a single call may reallocate and
//! copy - and landav reports worst case per call. So the pack refused it, and
//! for a long time that was the end of it.
//!
//! What landav actually reports, though, is a **sum of source steps along a
//! path**, never the maximum duration of one statement. The potential method
//! of amortised analysis says exactly that summing an amortised per-operation
//! cost over *any* sequence bounds that sequence's total cost. So an amortised
//! constant is a valid summand in the quantity being computed: a loop
//! appending `n` times reports `n` steps and really does cost O(n).
//!
//! The product decision was taken on that basis, and it came with a condition:
//! it may not be silent. A bound resting on amortisation publishes an
//! `amortised-cost` premise naming the callee, in the text line and in the
//! JSON, because a reader who takes the reported `1` as a limit on one call is
//! reading something no run of this analysis claims.
//!
//! # The two objections that had been stacked into one refusal
//!
//! The old row's `why` said `append` was "not merely unbounded - not SAFE",
//! because it mutates the collection and a length read before it is not the
//! length a loop after it walks.
//!
//! That is a real requirement and it was never what the refusal was holding
//! up. `mutates_arguments` on the row makes the engine forget every volatile
//! name at the node, and a collection parameter's length is volatile for this
//! reason. The evidence is that
//! `collection_length.rs::appending_to_a_collection_forgets_its_entry_length`
//! passes unchanged across this ticket, having been written when the call was
//! a region. Only the amortisation half of the objection was ever about cost.
//!
//! # The first admitted row matched on a receiver, which is why the guard here
//!
//! Every resolvable row before this one was keyed on a bare name
//! (`isinstance`) or a module attribute (`os.getcwd`). `append` is the first
//! matched on a *receiver*, and that makes the receiver rule load-bearing: on
//! a **protocol** annotation - `MutableSequence` - `.append` is user code that
//! may do anything at all, and a row resolving there would publish a bound the
//! program exceeds. The frontend therefore requires a **concrete** builtin
//! annotation, which is strictly narrower than the set it trusts for a
//! `__len__`. `a_protocol_receiver_resolves_nothing` is the most important
//! test in this file.

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

/// Every premise on `function`, as `(trust, subject)`.
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

/// The text output **with per-function bound lines**, which is where the
/// premise clause lands: a default `check` prints the run summary and the
/// findings, and `--bounds` is what asks for the numbers.
fn check_text(source: &str) -> io::Result<String> {
    let project = Project::new()?;
    let file = project.write("w.py", source)?;
    Ok(project.check(&file, &["--bounds"])?.stdout)
}

/// A counted loop whose body is one `append` on an annotated list.
const APPENDS: &str = "\
def collect(xs: list, n: int) -> int:
    total = 0
    for i in range(n):
        xs.append(i)
        total = total + 1
    return total
";

/// **The loop is counted, and the bound says it is amortised.**
///
/// Both halves, because either alone would be the wrong outcome. Counted with
/// no premise is a number that claims more than it can; a premise with no
/// bound is the refusal this ticket removed.
#[test]
fn an_append_on_a_concrete_list_is_counted_and_says_it_is_amortised() -> io::Result<()> {
    let functions = check_json(APPENDS)?;
    let collect = named(&functions, "collect");

    assert_eq!(
        collect["bound_kind"], "exact",
        "the `append` resolves, so nothing is left in the way of counting this \
         loop: {collect}"
    );
    assert!(
        holes(collect).is_empty(),
        "no hole should remain: {collect}"
    );
    assert!(
        premises(collect)
            .iter()
            .any(|(trust, subject)| trust == "amortised-cost" && subject == ".append"),
        "an exact bound that rests on amortisation must say so, naming the \
         callee: got {:?} for:\n{APPENDS}",
        premises(collect)
    );
    Ok(())
}

/// **The text line says it too.**
///
/// A departure from the supplied-signature clause, which is silent unless a
/// pack was supplied. This one fires on the builtin pack, so it would be
/// silent nowhere else, and most readers read the text rather than the JSON.
#[test]
fn the_text_line_names_the_amortisation() -> io::Result<()> {
    let text = check_text(APPENDS)?;
    assert!(
        text.contains("amortised over") && text.contains(".append"),
        "the text bound line must name the amortised callee, or a reader of \
         the default output cannot tell this number is a sum rather than a \
         per-call limit:\n{text}"
    );
    Ok(())
}

/// **A protocol receiver resolves nothing.** The soundness test of the ticket.
///
/// `MutableSequence.append` is user code. It may sort, it may do I/O, it may
/// recurse - the annotation promises an interface and says nothing about cost.
/// Trusting a protocol's `__len__` to equal its iteration count is a premise
/// this project publishes and defends (`LAN-106`); pricing a protocol's
/// *method* is not a premise but a guess.
#[test]
fn a_protocol_receiver_resolves_nothing() -> io::Result<()> {
    let source = "\
from typing import MutableSequence


def collect(xs: MutableSequence, n: int) -> int:
    total = 0
    for i in range(n):
        xs.append(i)
        total = total + 1
    return total
";
    let functions = check_json(source)?;
    let collect = named(&functions, "collect");
    assert!(
        holes(collect).contains(&"call".to_owned()),
        "`append` on a protocol is user code of unknown cost, so the call must \
         keep its hole - resolving it would publish a bound the program \
         exceeds: {collect}"
    );
    assert!(
        !premises(collect)
            .iter()
            .any(|(trust, _)| trust == "amortised-cost"),
        "nothing was charged an amortised cost here, so nothing should claim \
         it was: {collect}"
    );
    Ok(())
}

/// **A receiver the function cannot vouch for resolves nothing.**
///
/// `row_for`'s rule, which this path narrows rather than replaces. An
/// unannotated parameter says nothing about what it holds; a rebound one holds
/// something else by the time the call runs, so the annotation describes what
/// the caller passed and not what is there.
#[test]
fn an_unannotated_or_rebound_receiver_resolves_nothing() -> io::Result<()> {
    for (name, source) in [
        (
            "unannotated",
            "\
def unannotated(xs, n: int) -> int:
    total = 0
    for i in range(n):
        xs.append(i)
        total = total + 1
    return total
",
        ),
        (
            "rebound",
            "\
def rebound(xs: list, n: int) -> int:
    total = 0
    xs = other()
    for i in range(n):
        xs.append(i)
        total = total + 1
    return total
",
        ),
    ] {
        let functions = check_json(source)?;
        let function = named(&functions, name);
        assert!(
            holes(function).contains(&"call".to_owned()),
            "{name}: this function cannot say what the receiver holds, so the \
             row does not apply: {function}"
        );
    }
    Ok(())
}

/// **The methods amortisation does not rescue stay holes.**
///
/// Both are here because `LAN-127`'s counts make them look like near misses,
/// and the reasons are different from each other and from `append`:
///
/// * `dict.get` is O(1) on *average*, over an assumption about how the keys
///   hash, which is a weaker claim than O(1) amortised and not one a bound may
///   rest on. `__hash__` and `__eq__` are user code besides.
/// * `list.pop` has **no row at all**, deliberately. With no argument it is
///   amortised O(1); with an index it shifts the tail and is linear; and the
///   frontend does not track which *kind* of collection a receiver is, so
///   `dict.pop(k)` - which hashes a user key - cannot be told apart from it.
#[test]
fn the_methods_amortisation_does_not_rescue_stay_holes() -> io::Result<()> {
    for (name, statement) in [("gets", "d.get(i)"), ("pops", "d.pop()")] {
        let source = format!(
            "\
def {name}(d: dict, n: int) -> int:
    total = 0
    for i in range(n):
        {statement}
        total = total + 1
    return total
"
        );
        let functions = check_json(&source)?;
        let function = named(&functions, name);
        assert!(
            holes(function).contains(&"call".to_owned()),
            "{name}: must stay a hole: {function}"
        );
    }
    Ok(())
}
