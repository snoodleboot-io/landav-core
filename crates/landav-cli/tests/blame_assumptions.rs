//! `LAN-14` acceptance: **a partial bound names the assumption it could not
//! discharge, not only the term it could not account for.**
//!
//! # The half of the non-negotiable that was missing
//!
//! `CONTRIBUTING.md`'s third non-negotiable asks a partial result for two
//! things: name the unaccounted **term**, and name the **assumption that could
//! not be discharged**. LAN-87 delivered the first - every hole carries its
//! construct and its position, and the bound mentions its variable - and the
//! second was built and left unwired. [`landav_bound::Assumption`] and
//! [`landav_bound::Blame`] have existed since F-015; before this change the
//! word "assumption" appeared nowhere in either renderer, and
//! [`landav_bound::Assumption`] was referenced nowhere in this crate at all.
//!
//! # Why the construct is not already the assumption
//!
//! It looks redundant until the two disagree, and they disagree on the two most
//! common holes in the corpus. `construct` says what the region **is**;
//! the assumption says what could not be **established** about it, which is the
//! half a reader can act on:
//!
//! | construct | what is missing | what the reader must supply |
//! |---|---|---|
//! | `while` | a termination argument | a ranking function |
//! | `call` | the callee's cost | a cost contract, or analysis of the callee |
//! | `call` to itself | a ranking function | a measure that decreases |
//! | anything else | a cost rule for the construct | a rule, or a rewrite |
//!
//! Without the second column all four read as "landav could not derive it", and
//! all four have different fixes. Two `call` holes in one function, one of them
//! recursive, is the sharpest case: identical construct, identical position
//! format, and the work to close them is not the same work. That is
//! [`a_call_to_the_enclosing_function_is_ranked_recursion_not_an_unknown_callee`].
//!
//! # The `while` decision, and why it is not what the refusal vocabulary says
//!
//! [`landav_its::Unsupported::blame`] answers
//! [`landav_bound::Assumption::ResourceNotModelled`] for every refusal, and
//! argues explicitly that it is *not* `TerminationNotProved` - "a stronger and
//! different claim - the loop may well terminate, we simply declined to look".
//! That is right about a refusal.
//!
//! A `while` is not a refusal. It **lowers**: `landav-its` builds transitions
//! for it and no `Unsupported` node exists, which is why the corpus carries 186
//! `while` holes and zero `while` refusal records. The hole is raised by the
//! engine, which read the loop and found it had no ranking argument. So there
//! is nothing to call `blame()` on, and the answer `blame()` would give would be
//! wrong here for the reason `blame()` itself states: the engine did not decline
//! to look. The mapping is therefore keyed on the hole rather than on a refusal
//! record - see `crate::assumption` - which is also what makes it total over
//! holes rather than over refusals.
//!
//! # Why this drives the binary
//!
//! The claim is about what the **renderers** say. Both of them - the text report
//! a person reads and the JSON a gate parses - have to carry it, and the failure
//! this suite is written against is exactly one of them carrying it. That is
//! only observable at the process boundary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use serde_json::Value;

use common::{Project, Run};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// One function per assumption this build can reach, plus one carrying two.
///
/// `mixed` is the fixture that matters: two holes in one bound, whose
/// constructs differ and whose assumptions differ, so a renderer that emitted a
/// single assumption per function - or derived it from the function rather than
/// from the region - fails here and passes everywhere else.
const BLAME_PY: &str = r"
def opaque(n: int) -> int:
    while n > 0:
        n = n - 1
    return n


def caller(n: int) -> int:
    fetch(n)
    return n


def walk(n: int) -> int:
    walk(n)
    return n


def read_field(n: int) -> int:
    x = obj.field
    return n


def mixed(n: int) -> int:
    fetch(n)
    while n > 0:
        n = n - 1
    return n
";

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Run `landav check <fixture> --json` and parse stdout.
fn machine_run(project: &Project) -> io::Result<(Run, Value)> {
    let target = project.write("blame.py", BLAME_PY)?;
    let run = project.check(&target, &["--json"])?;
    let parsed = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not valid JSON ({why}): {}", run.describe()));
    Ok((run, parsed))
}

/// Every hole reported for `name`, as `(construct, assumption, subject)`.
///
/// Deliberately total: a missing function and a missing field alike collapse to
/// an empty list, so the *calling* test fails with its own message about what
/// should have been reported rather than with a panic from in here.
fn holes_of(parsed: &Value, name: &str) -> Vec<(String, String, Option<String>)> {
    let Some(functions) = parsed["functions"].as_array() else {
        return Vec::new();
    };
    let Some(function) = functions.iter().find(|entry| entry["name"] == name) else {
        return Vec::new();
    };
    function["holes"]
        .as_array()
        .map(|holes| {
            holes
                .iter()
                .map(|hole| {
                    (
                        hole["construct"].as_str().unwrap_or_default().to_owned(),
                        hole["assumption"].as_str().unwrap_or_default().to_owned(),
                        hole["assumption_subject"].as_str().map(ToOwned::to_owned),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The assumptions reported for `name`, in the order the holes were met.
fn assumptions_of(parsed: &Value, name: &str) -> Vec<String> {
    holes_of(parsed, name)
        .into_iter()
        .map(|(_, assumption, _)| assumption)
        .collect()
}

/// The `--bounds` line for `name`, or the empty string.
fn bounds_line(run: &Run, name: &str) -> String {
    run.output()
        .lines()
        .find(|line| line.contains(&format!(": {name}: ")))
        .unwrap_or_default()
        .to_owned()
}

// ---------------------------------------------------------------------------
// 1. Every hole carries one
// ---------------------------------------------------------------------------

/// No hole is reported without an assumption.
///
/// The property, rather than four examples: a region named with no obligation
/// beside it is the state LAN-14 is about, and it is reachable by adding a
/// construct without extending the mapping. Driven over every hole the fixture
/// produces, so a construct that starts holing tomorrow is covered today.
#[test]
fn every_hole_names_an_assumption_that_could_not_be_discharged() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = machine_run(&project)?;
    run.assert_did_not_crash();

    let functions = parsed["functions"].as_array();
    assert!(
        functions.is_some(),
        "the run reported no functions at all.\n{}",
        run.describe()
    );
    let mut seen = 0usize;
    for name in ["opaque", "caller", "walk", "read_field", "mixed"] {
        let holes = holes_of(&parsed, name);
        assert!(
            !holes.is_empty(),
            "`{name}` was expected to carry at least one hole; with none there \
             is nothing for this suite to be about.\n{}",
            run.describe()
        );
        for (construct, assumption, _) in holes {
            seen += 1;
            assert!(
                !assumption.trim().is_empty(),
                "the `{construct}` hole in `{name}` names the region and not the \
                 obligation. A report that says only \"landav could not derive \
                 this\" sends a reader to grep; naming the assumption tells them \
                 whether they owe a ranking function or a cost contract.\n{}",
                run.describe()
            );
        }
    }
    assert!(seen >= 5, "the fixture stopped producing holes");
    Ok(())
}

/// The assumptions are named, never coded.
///
/// The same rule the constructs already follow, and for the same reason: an
/// agent's transcript is read by a person, and `termination-not-proved`
/// survives that reading where `A-02` does not. Also catches the placeholder
/// this build renders for an [`landav_bound::Assumption`] variant it does not
/// know about - which is deliberately ugly precisely so it fails here.
#[test]
fn assumptions_are_named_rather_than_coded() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = machine_run(&project)?;

    for name in ["opaque", "caller", "walk", "read_field", "mixed"] {
        for assumption in assumptions_of(&parsed, name) {
            assert!(
                assumption.contains('-')
                    && assumption
                        .chars()
                        .all(|glyph| glyph.is_ascii_lowercase() || glyph == '-'),
                "`{assumption}` in `{name}` is not a name a reader can act on. \
                 An obligation is reported by what it is, never by an \
                 identifier a consumer has to look up in a table.\n{}",
                run.describe()
            );
            assert!(
                !assumption.contains("not-named"),
                "`{name}` carries an assumption this build has no name for, \
                 which means an `Assumption` variant reached the report without \
                 anybody deciding what a reader is told about it.\n{}",
                run.describe()
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 2. The mapping, where it is load-bearing
// ---------------------------------------------------------------------------

/// A `while` is a missing **termination** argument, not a missing cost rule.
///
/// The decision this suite exists to pin, and the one that is easy to get
/// backwards: `Unsupported::blame` answers `ResourceNotModelled` for everything
/// it is asked about, and a `while` is never asked about it - it lowers, so
/// there is no `Unsupported` node for it anywhere. Reporting a missing cost
/// rule here would send a reader to write a lowering rule for a construct the
/// lowering already handles, when what is missing is a ranking function.
#[test]
fn a_while_loop_reports_a_termination_argument_that_was_not_made() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = machine_run(&project)?;

    let holes = holes_of(&parsed, "opaque");
    assert_eq!(
        holes.len(),
        1,
        "one `while` is one region.\n{}",
        run.describe()
    );
    let (construct, assumption, _) = &holes[0];
    assert_eq!(construct, "while", "{}", run.describe());
    assert_eq!(
        assumption,
        "termination-not-proved",
        "the engine read this loop and found no ranking argument for it. \
         `resource-not-modelled` would say the cost model has no rule for \
         `while`, which is what a refusal means and is not what happened: a \
         `while` lowers, there is no refusal record for it anywhere in the run, \
         and the two send a reader to different files.\n{}",
        run.describe()
    );
    Ok(())
}

/// A call reports whose cost is unknown, by name.
///
/// "A call could not be accounted for" is not actionable; "the cost of `fetch`
/// is not known here" says exactly what to supply. The callee is the frontend's
/// own record, joined to the hole by position, rather than anything this layer
/// reconstructs.
#[test]
fn a_call_names_the_callee_whose_cost_is_unknown() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = machine_run(&project)?;

    let holes = holes_of(&parsed, "caller");
    assert_eq!(holes.len(), 1, "{}", run.describe());
    let (construct, assumption, subject) = &holes[0];
    assert_eq!(construct, "call", "{}", run.describe());
    assert_eq!(assumption, "callee-cost-unknown", "{}", run.describe());
    assert_eq!(
        subject.as_deref(),
        Some("fetch"),
        "the assumption must name the callee it is about, or a reader with \
         three calls in one function learns only that one of them is a \
         problem.\n{}",
        run.describe()
    );
    Ok(())
}

/// A call to the enclosing function is **recursion**, not an unknown callee.
///
/// The two are the same construct at the same kind of position and their
/// remedies are different: an unknown callee is closed by analysing or
/// annotating something else, and recursion is closed by a measure that
/// decreases. A mapping that keyed on the construct alone would report both as
/// `callee-cost-unknown` and pass every other test in this file.
///
/// Mutual recursion is **not** distinguished, and nothing here pretends
/// otherwise: `f` calls `g` calls `f` needs a call graph across functions, and
/// this layer sees one function at a time. Such a cycle reports
/// `callee-cost-unknown`, which is true and weaker than the truth.
#[test]
fn a_call_to_the_enclosing_function_is_ranked_recursion_not_an_unknown_callee() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = machine_run(&project)?;

    let holes = holes_of(&parsed, "walk");
    assert_eq!(holes.len(), 1, "{}", run.describe());
    let (construct, assumption, _) = &holes[0];
    assert_eq!(
        construct,
        "call",
        "the construct is unchanged - that is the point.\n{}",
        run.describe()
    );
    assert_eq!(
        assumption,
        "recursion-not-ranked",
        "`walk` calls `walk`. The obstacle is not that some other function's \
         cost is unknown; it is that this one recurses and nothing ranked it, \
         and the two are closed by different work.\n{}",
        run.describe()
    );
    assert_ne!(
        assumptions_of(&parsed, "walk"),
        assumptions_of(&parsed, "caller"),
        "a self-call and an ordinary call reported the same obligation, so the \
         assumption is being derived from the construct and adds nothing the \
         construct did not already say.\n{}",
        run.describe()
    );
    Ok(())
}

/// Anything else is a construct the cost model has no rule for, which is what
/// the refusal vocabulary already says and the reason it gives.
///
/// An attribute read is the fixture on purpose. It is the one construct in this
/// file that is *not* on any widening lane's list - `x.y` runs a `property`,
/// which is arbitrary user code, so `Partial(1 + omega)` is the true answer and
/// the read is pinned shut deliberately. A construct being swept into the
/// fragment tomorrow would take this test's subject away with it.
#[test]
fn any_other_construct_reports_a_missing_cost_rule() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = machine_run(&project)?;

    let holes = holes_of(&parsed, "read_field");
    assert!(!holes.is_empty(), "{}", run.describe());
    for (construct, assumption, subject) in holes {
        assert_eq!(
            assumption,
            "resource-not-modelled",
            "`{construct}` is a construct the cost model has no rule for.\n{}",
            run.describe()
        );
        assert!(
            subject.is_some_and(|text| !text.trim().is_empty()),
            "a missing cost rule must say what it is missing a rule *for*, or \
             it restates the construct field and carries nothing.\n{}",
            run.describe()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. Per region, and in both renderers
// ---------------------------------------------------------------------------

/// Two holes in one function report two different assumptions.
///
/// The assumption is a property of the **region**, not of the function. An
/// implementation that decided one assumption per bound - the first hole's, or
/// the "worst" one - would satisfy every test above and lose exactly the
/// information a reader with a mixed function needs.
#[test]
fn two_regions_in_one_function_report_their_own_assumptions() -> io::Result<()> {
    let project = Project::new()?;
    let (run, parsed) = machine_run(&project)?;

    let holes = holes_of(&parsed, "mixed");
    assert_eq!(
        holes.len(),
        2,
        "`mixed` has a call and a `while`, which are two regions.\n{}",
        run.describe()
    );
    let assumptions: Vec<&str> = holes
        .iter()
        .map(|(_, assumption, _)| assumption.as_str())
        .collect();
    assert!(
        assumptions.contains(&"callee-cost-unknown"),
        "the call in `mixed` lost its assumption: {assumptions:?}\n{}",
        run.describe()
    );
    assert!(
        assumptions.contains(&"termination-not-proved"),
        "the `while` in `mixed` lost its assumption: {assumptions:?}\n{}",
        run.describe()
    );
    Ok(())
}

/// The text report carries the assumption too, in the "apart from" clause.
///
/// Both stated consumers matter and they read different surfaces. A person at a
/// terminal reads the `--bounds` line and nothing else, and a JSON-only
/// implementation would leave them with the same "could not derive it" they had
/// before. The words are asserted rather than the layout: this suite is not
/// entitled to pin a sentence, only that the obligation is in it.
#[test]
fn the_text_report_states_the_assumption_beside_the_region() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("blame.py", BLAME_PY)?;
    let run = project.check(&target, &["--bounds"])?;
    run.assert_did_not_crash();

    let opaque = bounds_line(&run, "opaque");
    assert!(
        opaque.contains("while at"),
        "the region must still be named and placed: {opaque:?}\n{}",
        run.describe()
    );
    assert!(
        opaque.to_lowercase().contains("termination"),
        "the `while` line names the region and never says that what is missing \
         is a termination argument, so the reader is told what landav could not \
         do and not what they could supply: {opaque:?}\n{}",
        run.describe()
    );

    let caller = bounds_line(&run, "caller");
    assert!(
        caller.contains("fetch"),
        "the call line must name the callee whose cost is unknown: \
         {caller:?}\n{}",
        run.describe()
    );

    let walk = bounds_line(&run, "walk");
    assert!(
        walk.to_lowercase().contains("recurs"),
        "a self-call must read as recursion rather than as another unknown \
         callee: {walk:?}\n{}",
        run.describe()
    );
    Ok(())
}

/// The two renderers agree, region by region.
///
/// Not a restatement of the tests above. They check each surface against the
/// decision; this checks the surfaces against **each other**, which is the
/// failure that survives when both are correct in isolation and one of them is
/// updated. The text is checked for the assumption's own words rather than for
/// its machine name, so the two are genuinely independent renderings of one
/// value and not one string printed twice.
#[test]
fn the_two_renderers_agree_about_every_region() -> io::Result<()> {
    let project = Project::new()?;
    let (_, parsed) = machine_run(&project)?;
    let target = project.write("blame.py", BLAME_PY)?;
    let text = project.check(&target, &["--bounds"])?;

    // The clause each machine name is expected to read as, in the prose the
    // text renderer produces. Deliberately a word a reader would use, so a
    // renderer that printed the machine name into the sentence does not pass.
    let reads_as = [
        ("termination-not-proved", "termination"),
        ("recursion-not-ranked", "recurs"),
        ("callee-cost-unknown", "cost of"),
        ("resource-not-modelled", "no rule"),
    ];

    for name in ["opaque", "caller", "walk", "read_field", "mixed"] {
        let line = bounds_line(&text, name).to_lowercase();
        assert!(
            !line.is_empty(),
            "`--bounds` reported no line for `{name}`.\n{}",
            text.describe()
        );
        for assumption in assumptions_of(&parsed, name) {
            let expected = reads_as
                .iter()
                .find(|(machine, _)| *machine == assumption)
                .map(|(_, prose)| *prose)
                .unwrap_or_else(|| panic!("no prose is pinned for the assumption `{assumption}`"));
            assert!(
                line.contains(expected),
                "the JSON reports `{assumption}` for a region in `{name}` and \
                 the text line says nothing a reader would recognise as it. Two \
                 renderers disagreeing about one region is worse than either \
                 being silent, because whichever one a consumer trusts is a \
                 coin toss: {line:?}\n{}",
                text.describe()
            );
        }
    }
    Ok(())
}
