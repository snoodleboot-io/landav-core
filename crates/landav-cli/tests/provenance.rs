//! `LAN-87` acceptance at the process boundary: **what a run says about a
//! function that did not lower.**
//!
//! # The claim this suite polices
//!
//! `LAN-87` does not make 441 stdlib functions *bounded*. Every bound it adds
//! carries an unfilled hole, and an unfilled hole denotes `omega`. What changes
//! is that those functions become **analysed with named blame** — cost derived
//! apart from named call sites, with a construct and a position — instead of
//! producing nothing at all.
//!
//! Shipping it as "441 functions now have bounds" would be the failure, and the
//! output is where that failure would happen. So two things are asserted here,
//! and they pull in opposite directions on purpose:
//!
//! * **Provenance.** A function that did not lower must never report a bound
//!   marked `"exact"` or `"upper"`. Those are complete claims: a consumer may
//!   compare them against a budget. A function analysed apart from a call has
//!   an unfilled hole in it and is `"partial"`, always.
//! * **Coverage stability.** `Coverage::lowered()` counts transition systems,
//!   and the engine's new reach must not move it. The headline number means
//!   "this many functions the whole toolchain can handle", and quietly
//!   redefining it to "this many functions produced some number" is how a
//!   30x-sounding improvement gets claimed for work that did not do it.
//!
//! # These drive the built binary
//!
//! Following `common/mod.rs`: the contract is observed at the process
//! boundary, and a unit test calling the same functions proves nothing about
//! what a CI job actually receives. Assertions are on JSON fields and on the
//! presence of names in prose — never on a rendered bound, whose normal form
//! is allowed to change.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use common::{Project, Run};
use serde_json::Value;

/// One function inside the fragment, three whose only obstacle is a call.
///
/// The ratio is the point: after `LAN-87` all four have something reported
/// about them and exactly one of them lowered.
const CALLS_PY: &str = r"
def counted(n: int) -> int:
    x = 0
    for i in range(n):
        x = i
    return x


def bare_call(n: int) -> int:
    helper(n)
    return 0


def returned_call(n: int) -> int:
    return helper(n)


def call_in_loop(n: int) -> int:
    for i in range(n):
        helper(i)
    return 0
";

/// The three functions whose only obstacle is a call.
const CALLING: [&str; 3] = ["bare_call", "returned_call", "call_in_loop"];

fn run_json(project: &Project, source: &str) -> io::Result<Value> {
    let target = project.write("calls.py", source)?;
    let run = project.check(&target, &["--json"])?;
    run.assert_did_not_crash();
    Ok(serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not valid JSON ({why}): {}", run.describe())))
}

fn functions(run: &Value) -> &Vec<Value> {
    run["functions"].as_array().expect("functions is an array")
}

fn function<'a>(run: &'a Value, name: &str) -> &'a Value {
    functions(run)
        .iter()
        .find(|f| f["name"] == name)
        .unwrap_or_else(|| panic!("no function named {name} in {run}"))
}

/// The line of a `--bounds` run that concerns `name`.
fn bounds_line(run: &Run, name: &str) -> String {
    run.output()
        .lines()
        .find(|line| line.contains(&format!("{name}:")))
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("no line about `{name}`.\n{}", run.describe()))
}

// ---------------------------------------------------------------------------
// provenance
// ---------------------------------------------------------------------------

/// **A function that did not lower never claims a complete bound.**
///
/// The invariant, over every function in the run. `"exact"` and `"upper"` are
/// claims a gate can act on; a function analysed apart from an unfilled hole
/// has made neither. `null` is permitted — the engine may genuinely have
/// nothing to say — but `"exact"` and `"upper"` never are.
#[test]
fn a_function_that_did_not_lower_never_reports_a_complete_bound() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, CALLS_PY)?;

    for f in functions(&run) {
        if f["lowered"] == true {
            continue;
        }
        let kind = f["bound_kind"].as_str().unwrap_or("null");
        assert!(
            kind != "exact" && kind != "upper",
            "`{}` did not lower and reports a `{kind}` bound. Both of those are \
             complete claims a budget gate may act on, and this function's cost \
             was never established: {f}",
            f["name"]
        );
    }
    Ok(())
}

/// **A function whose only obstacle is a call reports a partial bound with
/// named regions.**
///
/// The other half of provenance, and the deliverable. `bound: null` — today's
/// answer — is indistinguishable from "we did not look", which is what the
/// existing assertion in `structured_output.rs` pins with the rationale that
/// "reporting one would be inventing a conclusion". After `LAN-87` reporting
/// one is not an invention: the bound is real, it mentions a hole, and the hole
/// names the call and where it is.
#[test]
fn a_call_bearing_function_reports_a_partial_bound_that_names_the_call() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, CALLS_PY)?;

    for name in CALLING {
        let f = function(&run, name);
        assert_eq!(
            f["lowered"], false,
            "`{name}` must still not lower - the ITS gains no representation \
             for unknown cost: {f}"
        );
        assert_eq!(
            f["bound_kind"], "partial",
            "`{name}` is analysable apart from its call, so it has a bound and \
             that bound is partial: {f}"
        );
        assert!(
            f["bound"].is_string(),
            "a partial bound is a bound, and a consumer needs it: {f}"
        );

        let holes = f["holes"].as_array().expect("holes is an array");
        assert!(
            !holes.is_empty(),
            "a partial bound without a hole is a complete bound wearing the \
             wrong label: {f}"
        );
        assert!(
            holes.iter().any(|hole| hole["construct"] == "call"),
            "the region must be blamed on the call by name: {f}"
        );
        for hole in holes {
            assert!(
                hole["origin"].as_str().unwrap_or_default().contains(':'),
                "every region is placed as well as named: {hole}"
            );
            let variable = hole["variable"].as_str().unwrap_or_default();
            assert!(
                f["bound"].as_str().unwrap_or_default().contains(variable),
                "the bound must mention the hole {variable:?}, or filling it \
                 later changes nothing: {f}"
            );
        }

        assert!(
            !f["refused"]
                .as_array()
                .expect("refused is an array")
                .is_empty(),
            "the refusal that stopped it lowering is still reported - the bound \
             is additional information, not a replacement for the blame: {f}"
        );
    }
    Ok(())
}

/// **The prose report stops saying "no bound" for a call.**
///
/// `--bounds` today prints "no bound: this function did not lower, so nothing
/// was derived for it" for every one of these, which is the sentence this
/// ticket exists to make false. It must be replaced by something naming the
/// call and its line, because that is what a user acts on.
#[test]
fn the_bounds_report_names_the_call_instead_of_reporting_no_bound() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("calls.py", CALLS_PY)?;
    let run = project.check(&target, &["--bounds"])?;
    run.assert_did_not_crash();

    for name in CALLING {
        let line = bounds_line(&run, name);
        assert!(
            !line.contains("no bound"),
            "`{name}` is analysable apart from its call, so \"no bound\" is no \
             longer true of it: {line:?}"
        );
        assert!(
            line.contains("call"),
            "the line must name what cost the bound: {line:?}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// coverage stability
// ---------------------------------------------------------------------------

/// **The lowered count does not move.**
///
/// One function of four became a transition system before this ticket and one
/// must afterwards. `Coverage::lowered()` is the headline number and it means
/// "the whole toolchain can handle this function"; the engine deriving a
/// partial bound for the other three does not make that true of them.
#[test]
fn the_lowered_count_does_not_move_when_calls_become_holes() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, CALLS_PY)?;

    assert_eq!(
        run["summary"]["functions"], 4,
        "four functions were offered"
    );
    assert_eq!(
        run["summary"]["lowered"], 1,
        "exactly one of these four becomes a transition system, and that is \
         what `lowered` counts. If this rose to four, the number was redefined \
         to mean 'produced some output' - which is how a 30x improvement gets \
         claimed for work that did not do it: {}",
        run["summary"]
    );

    let counted: usize = functions(&run)
        .iter()
        .filter(|f| f["lowered"] == true)
        .count();
    assert_eq!(
        run["summary"]["lowered"], counted,
        "the summary count and the per-function records must agree: {}",
        run["summary"]
    );
    Ok(())
}

/// **The engine's reach is a second, larger number a consumer can compute.**
///
/// Two numbers, separately named. This asserts the *quantities* are distinct
/// rather than pinning a field name the implementation has not chosen yet: on
/// this fixture, four functions get a bound and one lowers. Today both are one,
/// which is exactly the conflation to avoid — a run in which the two numbers
/// can never differ has only one number in it.
#[test]
fn the_engines_reach_is_a_different_number_from_the_lowered_count() -> io::Result<()> {
    let project = Project::new()?;
    let run = run_json(&project, CALLS_PY)?;

    let derived = functions(&run)
        .iter()
        .filter(|f| !f["bound"].is_null())
        .count();
    let lowered = usize::try_from(run["summary"]["lowered"].as_u64().unwrap_or_default())
        .expect("a function count fits in a usize");

    assert_eq!(
        derived, 4,
        "every function here is analysable - three of them only apart from a \
         call - so the engine reaches all four: {}",
        run["functions"]
    );
    assert!(
        derived > lowered,
        "the engine's reach ({derived}) must be reportable separately from the \
         lowered count ({lowered}), and on this fixture they differ. If they \
         are always equal the run has one number wearing two names."
    );
    Ok(())
}

/// **The coverage report still explains what a refusal costs.**
///
/// Pinned verbatim because the sentence is load-bearing and the *other* half of
/// it stops being true: "no transition system" remains exactly right for a
/// refused function, while "no bound" does not. A wholesale rewrite of this
/// report that drops the first along with the second would leave a reader
/// unable to tell what the coverage percentage is a percentage *of*.
#[test]
fn the_coverage_report_still_says_a_refused_function_has_no_transition_system() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("calls.py", CALLS_PY)?;
    let run = project.check(&target, &["--coverage"])?;
    run.assert_did_not_crash();

    assert!(
        run.mentions("no transition system"),
        "the report must still say what a refusal means for the result.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("call"),
        "and it must still name the construct that caused it.\n{}",
        run.describe()
    );
    Ok(())
}
