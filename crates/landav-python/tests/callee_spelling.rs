//! `LAN-108`: **a refused call says how its callee was spelled.**
//!
//! `fetch(x)` and `x.fetch(y)` are refused for different reasons and are
//! actionable differently. A bare name is a callee a signature row could
//! resolve; a method on a receiver this analysis has never seen is not the
//! builtin, and no row will ever resolve it. Before this the two produced the
//! same record - `call`, detail `fetch` - so a tally over a corpus could not
//! say how much of the `call` blocker a pack could actually reach.
//!
//! The spelling is Python's own: a method is written `.fetch`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

/// Every `call` refusal's detail for the one function in `source`, sorted.
fn call_details(source: &str) -> Vec<String> {
    let functions =
        landav_python::lower_module(Path::new("m.py"), source).expect("inside the fragment");
    let function = functions.into_iter().next().expect("one function");
    let mut details: Vec<String> = match landav_its::lower(function.program()) {
        Ok(_) => Vec::new(),
        Err(error) => error
            .refusals()
            .map(|ledger| {
                ledger
                    .as_slice()
                    .iter()
                    .filter(|record| record.construct().tag() == "call")
                    .filter_map(|record| record.detail().map(|d| d.as_str().to_owned()))
                    .collect()
            })
            .unwrap_or_default(),
    };
    details.sort();
    details
}

/// **The same name, spelled two ways, is two different records.**
#[test]
fn a_bare_call_and_a_method_call_to_the_same_name_are_told_apart() {
    let source = "\
def run(x, y) -> int:
    fetch(x)
    y.fetch(x)
    return 1
";
    assert_eq!(
        call_details(source),
        vec![".fetch".to_owned(), "fetch".to_owned()],
        "a bare call and a method call must not collapse to one detail: one \
         is a callee a signature row could resolve and the other never is"
    );
}

/// The receiver is not part of the spelling.
///
/// `self.items.append(v)` and `rows.append(v)` are the same *kind* of
/// refusal - a method on an unknown receiver - and the same row would answer
/// for both if a row could. Recording the receiver would make every call site
/// its own bucket, which is what the tally exists to avoid.
#[test]
fn a_method_is_spelled_by_its_name_alone() {
    let source = "\
def run(rows, v) -> int:
    rows.append(v)
    rows.inner.append(v)
    return 1
";
    assert_eq!(
        call_details(source),
        vec![".append".to_owned(), ".append".to_owned()]
    );
}

/// The protocol calls a `with` makes are methods too, and say so.
#[test]
fn a_context_manager_s_protocol_calls_are_spelled_as_methods() {
    let source = "\
def run(lock) -> int:
    with lock:
        pass
    return 1
";
    let details = call_details(source);
    assert!(
        details.contains(&".__enter__".to_owned()) && details.contains(&".__exit__".to_owned()),
        "got {details:?}: `__enter__` and `__exit__` are calls on the context \
         manager, never bare names a row could stand for"
    );
}
