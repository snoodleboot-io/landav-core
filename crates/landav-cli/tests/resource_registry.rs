//! LAN-86: `--resource` must stop advertising four resources and deriving none
//! of them.
//!
//! # The decision this suite encodes
//!
//! Each resource states **individually** what it is waiting on, and `queries`
//! **becomes derivable**. That is option 2 of the ticket plus the half the
//! ticket could not have known about.
//!
//! # Why `queries` is derivable now and was not when the ticket was written
//!
//! LAN-86 says "`queries` counts external calls, and calls are refused
//! outright. It cannot be non-zero for any function that currently lowers."
//! That was true. LAN-87 changed it: a call is no longer a refusal that
//! discards the function, it is a named [`landav_engine::Hole`] carried in the
//! result at its position in the control structure, with `construct() ==
//! "call"` and an origin. The stdlib corpus carries thousands of them. Counting
//! the calls a function issues is therefore real, derivable information, and it
//! is derivable from a structure the engine already builds.
//!
//! # The soundness rule, stated once
//!
//! A resource bound may be **loose**. It may never be **low**. `queries` is
//! the count of calls *issued*, not the count of call *sites* in the source:
//! a call site inside a loop that runs ten times issues ten calls. A consumer
//! thresholds on this number, so an under-count passes a function that blows
//! its budget, silently, with a plausible-looking answer. That is the one
//! direction this suite forbids outright; see
//! [`a_call_in_a_loop_counts_once_per_iteration`] and
//! [`a_region_that_was_not_derived_never_reports_a_confident_zero`].
//!
//! # The JSON shape these tests pin, and why JSON rather than prose
//!
//! Prose is not a contract; `machine.rs` is. Two additions:
//!
//! **Top level**, on any run that named a resource:
//!
//! ```json
//! "resource": {
//!   "id": "queries",
//!   "unit": "queries",
//!   "semiring": "additive",
//!   "derived": true,
//!   "awaiting": null
//! }
//! ```
//!
//! `derived` says whether this build produces a number for this resource.
//! `awaiting` is `null` exactly when `derived` is true, and otherwise is the
//! sentence saying what *this* resource — not the registry as a whole — is
//! waiting on. A run that named no resource carries no such object.
//!
//! **Per function**, only when the selected resource is derivable:
//!
//! ```json
//! "resource": { "bound": "10", "bound_kind": "exact", "value": 10 }
//! ```
//!
//! `bound` is the derived expression rendered the way the cost `bound` already
//! is, because a resource bound may be symbolic (`for i in range(n): fetch(i)`
//! issues `n` queries). `value` is the same quantity as a JSON number when the
//! expression is a closed constant and `null` when it is not, so a gate can
//! threshold without parsing algebra. Every numeric assertion below is on
//! `value`, over fixtures whose loop counts are literals.
//!
//! # Where the four names are spelled out, and why that is allowed here
//!
//! `resource_selection.rs` drives everything from the registry on purpose, and
//! that discipline still holds for anything about the *set*. It cannot hold
//! here: "`ops` needs a calibration profile" and "`peak-mem` needs a different
//! semiring" are claims about particular resources, and the ticket's acceptance
//! criterion is precisely that those claims stop being interchangeable. So the
//! set-shaped tests below iterate [`ResourceKind::ALL`], and only the
//! content-shaped ones name a resource.
//!
//! # Tests elsewhere that this decision invalidates
//!
//! Three existing assertions say the opposite of what is asserted here and must
//! be revisited by whoever implements this, not deleted quietly:
//!
//! * `resource_selection::a_selected_resource_is_inconclusive_rather_than_clean`
//! * `resource_selection::the_help_is_generated_and_does_not_promise_a_bound`
//! * `crate::resource`'s unit test `the_long_help_does_not_promise_a_bound`
//!
//! Each of them pins "no bound is derived for **any** resource", which becomes
//! false the moment `queries` derives one. The honest replacement is the same
//! statement per resource, which is what this suite checks.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use landav_bound::ResourceKind;
use serde_json::Value;

use common::{EXIT_CLEAN, EXIT_TOOL_ERROR, Project, Run};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Four functions, each pinning one thing `queries` has to get right.
///
/// Every call is a bare expression statement rather than `x = fetch(n)`. That
/// is deliberate: an assignment whose right-hand side is not a proven integer
/// raises a second, `non-integer-value` hole beside the `call` hole, and the
/// count would then be asserted over a function carrying a region that is not a
/// call. These fixtures carry `call` holes and nothing else, so a wrong number
/// can only come from the counting rule.
const QUERIES_PY: &str = r"
def two_calls(n: int) -> int:
    fetch(n)
    store(n)
    return n


def no_calls(n: int) -> int:
    total = 0
    for i in range(n):
        total = i
    return total


def call_in_loop(n: int) -> int:
    for i in range(10):
        fetch(i)
    return n


def call_in_branch(n: int) -> int:
    if n > 0:
        fetch(n)
        store(n)
    else:
        fetch(n)
    return n
";

/// A `while` loop: a region the engine cannot read, holed whole.
///
/// It contains no call *that the engine can see*, which is not the same as
/// containing no call. Whatever `queries` reports for this function, it may not
/// be a confident zero.
const OPAQUE_REGION_PY: &str = r"
def opaque_region(n: int) -> int:
    while n > 0:
        n = n - 1
    return n
";

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Run `landav check <fixture> --json`, plus `extra`, and parse stdout.
///
/// Returns the raw [`Run`] as well, so assertion messages can show the whole
/// invocation rather than a JSON fragment with no context.
fn run_json(project: &Project, source: &str, extra: &[&str]) -> io::Result<(Run, Value)> {
    let target = project.write("fixture.py", source)?;
    let mut args: Vec<&str> = vec!["--json"];
    args.extend_from_slice(extra);
    let run = project.check(&target, &args)?;
    let parsed = serde_json::from_str(&run.stdout).unwrap_or_else(|why| {
        panic!("stdout was not valid JSON ({why}): {}", run.describe());
    });
    Ok((run, parsed))
}

/// The run-level resource block, or [`Value::Null`] when there is none.
fn resource_block(parsed: &Value) -> &Value {
    &parsed["resource"]
}

/// The number of queries reported for `name`, or `None` if the run reported
/// no number for it.
///
/// Deliberately total: a missing function, a missing `resource` object and a
/// non-numeric `value` all collapse to `None`, so an unimplemented surface
/// fails the *assertion* in the test that called this, with that test's own
/// message about what the number should have been. A helper that panicked
/// would replace every one of those messages with a stack trace.
fn queries_value(parsed: &Value, name: &str) -> Option<i64> {
    let function = parsed["functions"]
        .as_array()?
        .iter()
        .find(|entry| entry["name"] == name)?;
    function["resource"]["value"].as_i64()
}

/// The `bound_kind` reported for `name`'s resource bound, if any.
fn queries_kind<'a>(parsed: &'a Value, name: &str) -> Option<&'a str> {
    let function = parsed["functions"]
        .as_array()?
        .iter()
        .find(|entry| entry["name"] == name)?;
    function["resource"]["bound_kind"].as_str()
}

/// Whether `parsed` reports every function the fixture defines.
///
/// Called before the numeric assertions so that "the run did not analyse this
/// file at all" cannot masquerade as "the number is wrong".
fn assert_functions_present(parsed: &Value, run: &Run, names: &[&str]) {
    let functions = parsed["functions"].as_array();
    assert!(
        functions.is_some(),
        "the run reported no `functions` array at all.\n{}",
        run.describe()
    );
    let Some(functions) = functions else { return };
    for name in names {
        assert!(
            functions.iter().any(|entry| entry["name"] == *name),
            "the run did not report the function `{name}`, so nothing below is \
             a statement about `queries`.\n{}",
            run.describe()
        );
    }
}

/// `awaiting` for `kind`, as reported by a run that selected it.
///
/// `Ok(None)` means the resource reported itself as derived, which is the
/// other legal state.
fn awaiting(project: &Project, kind: ResourceKind) -> io::Result<(Run, Value)> {
    let id = kind.descriptor().id();
    let (run, parsed) = run_json(project, QUERIES_PY, &["--resource", id.as_str()])?;
    Ok((run, parsed))
}

// ---------------------------------------------------------------------------
// 1. Every resource says, individually, what it is waiting on
// ---------------------------------------------------------------------------

/// Criterion 1: a resource that derives nothing says what *it* is waiting on.
///
/// Driven from [`ResourceKind::ALL`], so a resource registered tomorrow is
/// covered today. Two properties, and the second is the whole point:
///
/// * every registered resource either reports `derived: true` or carries a
///   non-empty `awaiting`; and
/// * no two `awaiting` sentences are the same string.
///
/// The second is what a blanket message fails. "No bound is derived for any
/// resource in this build" satisfies the first for all four and the second for
/// none of them, which is exactly the state the ticket is about: a reader
/// learns that everything is unavailable and cannot learn *why* `peak-mem` is,
/// which is a different reason from `ops`.
#[test]
fn every_resource_states_individually_what_it_is_waiting_on() -> io::Result<()> {
    let project = Project::new()?;
    let mut excuses: Vec<(String, String)> = Vec::new();
    let mut derived_any = false;

    for kind in ResourceKind::ALL {
        let id = kind.descriptor().id().as_str().to_owned();
        let (run, parsed) = awaiting(&project, *kind)?;
        run.assert_did_not_crash();

        let block = resource_block(&parsed);
        assert_eq!(
            block["id"].as_str(),
            Some(id.as_str()),
            "a run that selected `{id}` must say so in its machine output, \
             naming the resource and not merely its algebra.\n{}",
            run.describe()
        );

        let derived = block["derived"].as_bool();
        assert!(
            derived.is_some(),
            "`resource.derived` must say, as a boolean, whether this build \
             produces a number for `{id}`. A consumer cannot tell a missing \
             number from a zero otherwise.\n{}",
            run.describe()
        );

        if derived == Some(true) {
            derived_any = true;
            assert!(
                block["awaiting"].is_null(),
                "`{id}` reports a derived number and also reports something it \
                 is waiting on. Those are contradictory claims about the same \
                 run.\n{}",
                run.describe()
            );
            continue;
        }

        let excuse = block["awaiting"].as_str();
        assert!(
            excuse.is_some_and(|text| !text.trim().is_empty()),
            "`{id}` derives no number and does not say what it is waiting on. \
             LAN-86's acceptance is that every resource either derives a number \
             or states, individually, what it needs.\n{}",
            run.describe()
        );
        if let Some(text) = excuse {
            excuses.push((id, text.to_owned()));
        }
    }

    assert!(
        derived_any,
        "no registered resource derives a number. `queries` counts call holes, \
         which LAN-87 made real, so at least one resource must now report one."
    );

    for (index, (left_id, left)) in excuses.iter().enumerate() {
        for (right_id, right) in excuses.iter().skip(index + 1) {
            assert_ne!(
                left, right,
                "`{left_id}` and `{right_id}` are waiting on different things - \
                 a calibration profile is not an IR that represents data - and \
                 they gave the same sentence. One blanket message for the whole \
                 registry is the state LAN-86 exists to end: it tells a reader \
                 that everything is unavailable and never why."
            );
        }
    }
    Ok(())
}

/// Criterion 2: `ops` is waiting on a calibration profile, and says so.
///
/// It is the only one of the four that is a *scaling* of what the engine
/// already derives, so it is the only one whose blocker is a number rather than
/// an analysis. Naming that distinguishes it from the three that need new work,
/// and it is also the sentence that tells a reader what `landav-calibrate` is
/// for.
#[test]
fn ops_says_it_needs_a_calibration_profile() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = awaiting(&project, ResourceKind::Ops)?;

    let text = resource_block(&parsed)["awaiting"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    assert!(
        text.contains("calibration"),
        "`ops` must say it is waiting on a calibration, since a step count \
         becomes an operation count only under one. Got: {text:?}\n{}",
        run.describe()
    );
    assert!(
        text.contains("profile"),
        "`ops` must name the *profile* as the missing input, not calibration in \
         the abstract - that is the artefact `landav-calibrate` produces and the \
         thing a user can go and make. Got: {text:?}\n{}",
        run.describe()
    );
    Ok(())
}

/// Criterion 3: `alloc` and `peak-mem` are waiting on an IR that represents
/// data.
///
/// Nothing in an integer-scalar fragment allocates, so these two are not
/// blocked on a constant or on a solver; they are meaningless until the Landav
/// IR has values with sizes. Asserted for both, because that is the shared
/// reason and it is the one a reader most needs, and because a message that
/// only covered one of them would leave the other on the blanket text.
#[test]
fn alloc_and_peak_mem_say_they_need_the_ir_to_represent_data() -> io::Result<()> {
    let project = Project::new()?;

    for kind in [ResourceKind::Alloc, ResourceKind::PeakMem] {
        let id = kind.descriptor().id();
        let (run, parsed) = awaiting(&project, kind)?;
        let text = resource_block(&parsed)["awaiting"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase();

        assert!(
            text.contains("data"),
            "`{id}` must say that it is waiting on the IR to represent data. \
             The analysable fragment is integer scalars and nothing in it \
             allocates, so no calibration and no solver unblocks this one. \
             Got: {text:?}\n{}",
            run.describe()
        );
        assert!(
            text.contains("represent"),
            "`{id}` must name *representation* as what is missing, so a reader \
             can tell this apart from `ops`, which is waiting on a number for an \
             analysis that already exists. Got: {text:?}\n{}",
            run.describe()
        );
    }
    Ok(())
}

/// Criterion 4: `peak-mem` additionally names the algebra that makes it a
/// second analysis rather than a scaling.
///
/// `ops`, `alloc` and `queries` all instantiate the additive semiring;
/// `peak-mem` instantiates `peak`. A maximum over a program's lifetime is not
/// obtainable by scaling a sum, so `peak-mem` is blocked on everything `alloc`
/// is blocked on *and* on an analysis nothing in the engine performs. A reader
/// told only "the IR must represent data" would reasonably expect `peak-mem` to
/// arrive with `alloc`.
///
/// The semiring name is taken from the descriptor, so this cannot be satisfied
/// by printing a fixed word, and it follows a registry edit with no edit here.
#[test]
fn peak_mem_also_names_its_different_semiring() -> io::Result<()> {
    let project = Project::new()?;
    let peak = ResourceKind::PeakMem.descriptor();
    let alloc = ResourceKind::Alloc.descriptor();
    assert_ne!(
        peak.semiring(),
        alloc.semiring(),
        "this test is about the algebra that sets `peak-mem` apart; if the two \
         now share one, the test is asserting something that stopped being true"
    );

    let (run, parsed) = awaiting(&project, ResourceKind::PeakMem)?;
    let text = resource_block(&parsed)["awaiting"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();

    assert!(
        text.contains("semiring"),
        "`peak-mem` must say that its algebra is part of what it is waiting on. \
         Got: {text:?}\n{}",
        run.describe()
    );
    assert!(
        text.contains(&peak.semiring().as_str().to_lowercase()),
        "`peak-mem` must name the `{}` semiring it instantiates. A peak is a \
         maximum over the run, not a scaled sum, so it is a different analysis \
         from `alloc` and not a later delivery of the same one. Got: {text:?}\n{}",
        peak.semiring().as_str(),
        run.describe()
    );
    Ok(())
}

/// A resource that derives nothing must not also emit a per-function number.
///
/// The two halves of the surface have to agree. A run reporting `derived:
/// false` at the top and a number per function would be the fabrication the
/// whole exit contract exists to prevent, and the number would look entirely
/// plausible.
#[test]
fn a_resource_that_derives_nothing_reports_no_per_function_number() -> io::Result<()> {
    let project = Project::new()?;

    for kind in ResourceKind::ALL {
        let id = kind.descriptor().id();
        let (run, parsed) = awaiting(&project, *kind)?;
        let derived = resource_block(&parsed)["derived"].as_bool();
        assert!(
            derived.is_some(),
            "a run that selected `{id}` reported nothing about whether it \
             derived a number, so there is no surface on which the two halves \
             can be checked against each other.\n{}",
            run.describe()
        );
        if derived != Some(false) {
            continue;
        }
        for name in ["two_calls", "no_calls", "call_in_loop", "call_in_branch"] {
            assert_eq!(
                queries_value(&parsed, name),
                None,
                "`{id}` reports that it derives nothing and then reports a \
                 number for `{name}`. One of the two is a lie, and a consumer \
                 has no way to tell which.\n{}",
                run.describe()
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 5. `queries` derives a number
// ---------------------------------------------------------------------------

/// Criterion 5: a function with two calls reports two, one with none reports
/// zero.
///
/// The zero half matters as much as the two. A resource that only ever reported
/// numbers for functions containing calls would be indistinguishable from one
/// that reported nothing, and `no_calls` lowers completely - the run knows
/// everything about it, so `0` here is a claim the run is entitled to make.
#[test]
fn queries_derives_a_number_for_a_function_with_calls_and_for_one_without() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = run_json(&project, QUERIES_PY, &["--resource", "queries"])?;
    run.assert_did_not_crash();
    assert_functions_present(&parsed, &run, &["two_calls", "no_calls"]);

    assert_eq!(
        resource_block(&parsed)["derived"].as_bool(),
        Some(true),
        "`queries` counts calls, and since LAN-87 a call is a hole carried in \
         the result rather than a refusal that discards the function. There is \
         something to count.\n{}",
        run.describe()
    );

    assert_eq!(
        queries_value(&parsed, "two_calls"),
        Some(2),
        "`two_calls` issues `fetch` once and `store` once, on a straight-line \
         path. Two calls, so two queries.\n{}",
        run.describe()
    );
    assert_eq!(
        queries_value(&parsed, "no_calls"),
        Some(0),
        "`no_calls` lowers completely and issues nothing. Zero is a claim the \
         run has established, and it must be reported as the number 0 rather \
         than as an absent value - `machine.rs` already says that a missing \
         bound is not a zero, and the converse holds too: a known zero must not \
         be reported as missing.\n{}",
        run.describe()
    );
    assert_eq!(
        queries_kind(&parsed, "no_calls"),
        Some("exact"),
        "nothing about `no_calls` is unknown, so its query count is exact and \
         not merely an upper bound. Reporting `upper` here throws away the \
         two-sided claim the engine actually established.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 6. Soundness: a call in a loop
// ---------------------------------------------------------------------------

/// Criterion 6, and the assertion this suite exists for: a call inside a loop
/// is counted **once per iteration**.
///
/// `call_in_loop` has one `fetch` call *site*, inside `for i in range(10)`. It
/// issues ten calls. `queries` is documented in the registry as "external calls
/// issued", so the answer is 10.
///
/// Counting hole occurrences in [`landav_engine::TripCount::holes`] gives 1,
/// because the ledger holds one entry per syntactic region. That is the
/// obvious implementation and it is wrong in the one direction that cannot be
/// tolerated. The engine already has the right answer: it charges a hole at its
/// position in the control structure, so the cost bound for this function is
/// `(1 + (10 * (2 + #hole0)))` - the hole variable is *multiplied* by the trip
/// count. The query count must be read off the same arithmetic.
#[test]
fn a_call_in_a_loop_counts_once_per_iteration() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = run_json(&project, QUERIES_PY, &["--resource", "queries"])?;
    assert_functions_present(&parsed, &run, &["call_in_loop"]);

    let reported = queries_value(&parsed, "call_in_loop");
    assert_ne!(
        reported,
        Some(1),
        "`call_in_loop` reported 1 query: the number of call *sites*, not the \
         number of calls *issued*. The loop runs ten times and issues ten. \
         UNDER-COUNTING IS THE ONE DIRECTION A RESOURCE BOUND MAY NEVER GO. A \
         bound that is loose is merely unhelpful; a bound that is low is a gate \
         passing a function that issues ten times its budget, and the number \
         looks entirely reasonable on the way past.\n{}",
        run.describe()
    );
    assert_eq!(
        reported,
        Some(10),
        "`for i in range(10): fetch(i)` issues exactly ten calls. Anything \
         below ten is unsound; anything above ten is sound but is not what this \
         analysis can see, since the trip count here is a literal and the engine \
         derives it exactly.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 7. A call in a branch
// ---------------------------------------------------------------------------

/// Criterion 7: a branch takes the **maximum** of its arms, not the sum.
///
/// `call_in_branch` issues two calls on the `if` arm and one on the `else` arm.
/// Exactly one arm runs, so the worst case is two.
///
/// Three candidate answers, and only one is right:
///
/// * **1** - the cheaper arm, or the count of distinct callees. Unsound: the
///   expensive arm is reachable and issues two.
/// * **3** - the sum of the arms. Sound, but it charges for calls no execution
///   can issue, and it disagrees with how the engine already charges a branch
///   ([`landav_engine::TripCount::branching`] joins with `Bound::max_of`). A
///   resource that used a different control-flow rule from the cost analysis
///   would drift from it on every nested branch, and the two numbers are
///   reported side by side.
/// * **2** - the maximum. Worst case, attained, and consistent with the engine.
///
/// The arms are deliberately asymmetric. With one call in each arm, max and sum
/// give 1 and 2, and the test would pass against a summing implementation the
/// day someone wrote `if c: fetch(n) else: fetch(n)`.
#[test]
fn a_call_in_a_branch_is_the_worse_arm_and_not_the_sum() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = run_json(&project, QUERIES_PY, &["--resource", "queries"])?;
    assert_functions_present(&parsed, &run, &["call_in_branch"]);

    let reported = queries_value(&parsed, "call_in_branch");
    assert_ne!(
        reported,
        Some(1),
        "`call_in_branch` reported 1: the cheaper arm. The `if` arm issues two \
         calls and is reachable, so this is an under-count, which is the one \
         direction that is never allowed.\n{}",
        run.describe()
    );
    assert_eq!(
        reported,
        Some(2),
        "a branch issues the calls of whichever arm runs, and exactly one arm \
         runs. The worst case is the `if` arm's two. Reporting 3 sums the arms: \
         sound, but it charges for a call no execution issues, and it uses a \
         different control-flow rule from the cost analysis sitting beside it in \
         the same JSON, which already joins branches with a maximum.\n{}",
        run.describe()
    );
    Ok(())
}

/// The soundness rule restated where it is easiest to break: a region the
/// engine could not read must never report a confident zero.
///
/// `opaque_region`'s `while` loop is holed whole. The engine sees no call in
/// it - and cannot, because it did not read it. A body that calls out on every
/// iteration is entirely consistent with what the run knows.
///
/// So the count for this function may be `partial` (a bound mentioning the
/// unfilled hole, which denotes omega), or it may be absent. It may not be
/// `0`, and it may not be `exact` or `upper`: those are complete claims, and
/// `machine.rs` already forbids a complete claim over an unfilled hole for the
/// cost bound for exactly this reason.
#[test]
fn a_region_that_was_not_derived_never_reports_a_confident_zero() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = run_json(&project, OPAQUE_REGION_PY, &["--resource", "queries"])?;
    run.assert_did_not_crash();
    assert_functions_present(&parsed, &run, &["opaque_region"]);

    assert_eq!(
        resource_block(&parsed)["derived"].as_bool(),
        Some(true),
        "`queries` must derive for this run too. A resource that reports itself \
         derivable only over files it happens to like is not derivable; this \
         file is ordinary Python.\n{}",
        run.describe()
    );
    assert_ne!(
        queries_value(&parsed, "opaque_region"),
        Some(0),
        "the `while` body was never read, so it may call out on every \
         iteration. Reporting 0 queries for it states that it issues none, \
         which the run has no evidence for - and a zero is the most dangerous \
         number here, because every gate passes it.\n{}",
        run.describe()
    );
    assert_eq!(
        queries_kind(&parsed, "opaque_region"),
        Some("partial"),
        "a query count over an unanalysed region carries an unfilled hole, and \
         an unfilled hole denotes omega. `exact` and `upper` are complete \
         claims a consumer may compare against a budget, and reporting neither \
         at all leaves the function silently absent from a gate's sweep. Only \
         `partial` is honest here.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 8. A derivable resource is no longer inconclusive
// ---------------------------------------------------------------------------

/// Criterion 8: `--resource queries` stops reporting the run inconclusive once
/// it can derive a number.
///
/// The inconclusive verdict was correct while nothing propagated: a question
/// was asked and nothing was concluded, and exit `0` would have been a claim
/// nobody had checked. That reasoning runs out the moment the question is
/// answered. Continuing to report inconclusive would train callers to ignore
/// the verdict on the one resource that works.
///
/// The control is the same file without the flag: it is clean, so any change
/// here is a consequence of the question asked and not of the fixture.
#[test]
fn queries_is_no_longer_inconclusive_when_it_derives_a_number() -> io::Result<()> {
    let project = Project::new()?;

    let (control, control_json) = run_json(&project, QUERIES_PY, &[])?;
    assert_eq!(
        control.code,
        EXIT_CLEAN,
        "the control run - no `--resource` - must be clean, or this test is \
         measuring something else.\n{}",
        control.describe()
    );
    assert_eq!(control_json["outcome"], "clean");

    let (run, parsed) = run_json(&project, QUERIES_PY, &["--resource", "queries"])?;
    run.assert_did_not_crash();
    run.assert_code_is_sanctioned();

    assert_ne!(
        parsed["outcome"],
        "inconclusive",
        "the run derived a query count for every function in this file and then \
         reported that it had concluded nothing. `Outcome::Inconclusive` means \
         a question was asked and not answered; this one was answered.\n{}",
        run.describe()
    );
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "the same file exits 0 without the flag and derives a number with it, \
         so asking a question landav can answer must not fail the build.\n{}",
        run.describe()
    );
    assert!(
        !run.mentions("no bound was derived for `queries`"),
        "the run printed the unaccounted-for-resource line for a resource it \
         had just accounted for.\n{}",
        run.describe()
    );
    Ok(())
}

/// `--help` stops making the blanket claim, and says it per resource instead.
///
/// The only prose assertion in this file, and it earns its place: the long help
/// is the one description of `--resource` most callers ever read, and it
/// currently ends with "No bound is derived for any resource in this build".
/// That sentence becomes false when `queries` derives one, and a stale caveat
/// is a lie the tool tells about itself - which is the argument
/// `crate::resource`'s own `NO_BOUND` documentation already makes.
#[test]
fn the_help_no_longer_says_no_resource_derives_a_bound() -> io::Result<()> {
    let project = Project::new()?;
    let run = project.run(&["check", "--help"])?;

    run.assert_did_not_crash();
    assert_eq!(run.code, EXIT_CLEAN, "{}", run.describe());
    assert!(
        !run.mentions("no bound is derived for any resource"),
        "`--help` still says no resource derives a bound, and one now does. A \
         caveat left behind after it stopped being true sends every reader of \
         `--resource queries` looking for the bug that is not there.\n{}",
        run.describe()
    );
    for kind in ResourceKind::ALL {
        let descriptor = kind.descriptor();
        assert!(
            run.output().contains(descriptor.id().as_str()),
            "`--help` must still list every registered resource; `{}` is \
             missing.\n{}",
            descriptor.id(),
            run.describe()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 9. An unknown resource still fails cleanly
// ---------------------------------------------------------------------------

/// Criterion 9: nothing above weakened the rejection of a value that is not in
/// the registry.
///
/// `cycles` is a plausible cost model somebody would type. It must exit `2`
/// with blame - the value and the registered set - analyse nothing, and emit no
/// JSON, because a consumer that got a parsable run for a rejected invocation
/// would read it as an answer.
#[test]
fn an_unknown_resource_still_fails_cleanly() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("fixture.py", QUERIES_PY)?;

    let run = project.check(&target, &["--json", "--resource", "cycles"])?;

    run.assert_did_not_crash();
    run.assert_code_is_sanctioned();
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "an unrecognised `--resource` is a usage error: the caller asked a \
         question the tool cannot parse, and analysing anyway would answer a \
         question nobody asked.\n{}",
        run.describe()
    );
    run.assert_explains("cycles");
    assert!(
        run.output()
            .contains(&ResourceKind::registered_names().join(", ")),
        "the rejection must list the registered set, rendered from the registry \
         itself, so a caller who guessed wrong is told what the real values \
         are.\n{}",
        run.describe()
    );
    assert!(
        serde_json::from_str::<Value>(&run.stdout).is_err(),
        "a rejected invocation emitted a parsable run on stdout. A consumer \
         would read that as an answer to a question the tool refused.\n{}",
        run.describe()
    );
    Ok(())
}
