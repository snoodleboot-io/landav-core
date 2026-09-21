//! `LAN-13` at the process boundary: **a run says what table it believed.**
//!
//! # What a supplied pack does, and why that needs saying
//!
//! A pack turns a call the analysis could not cost into one it can. The
//! function below is `Theta(2 + n*(3 + #hole0))` with a hole in it on the
//! default table, and `Theta(2 + 3n)` — complete, exact, 100% coverage — with
//! one row supplied. That is the feature.
//!
//! It is also the risk, and they are the same sentence. The second number is
//! not more derived than the first; it is the first plus a claim somebody
//! typed into a file. Nothing in the bound's own shape distinguishes a cost
//! the engine derived from a cost a row asserted, so the run has to say so
//! itself — in the text a person reads and in the JSON a gate parses.
//!
//! So these tests are about *attribution*, not arithmetic. They assert that a
//! bound resting on a supplied row names the row and the file, that a row
//! replacing another says what it replaced and what that one argued, and that
//! a pack which cannot be read stops the run instead of quietly not applying.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use common::{EXIT_CLEAN, EXIT_TOOL_ERROR, Project};
use serde_json::Value;

/// A loop calling a name no builtin row claims.
const SOURCE: &str = "\
def run(rows: list, n: int) -> int:
    total = 0
    for i in range(n):
        frobnicate(rows)
        total = total + 1
    return total
";

/// A pack declaring that callee resolvable.
const TEAM_PACK: &str = "\
[[signature]]
callee = \"frobnicate\"
cost = \"constant\"
rebinds_locals = false
mutates_arguments = true
why = \"a table lookup, measured on our deployment\"
";

/// A pack that contradicts a row the builtin argues for.
const LOOSENING_PACK: &str = "\
[[signature]]
callee = \"sorted\"
cost = \"constant\"
rebinds_locals = false
mutates_arguments = true
why = \"we only ever sort three-element lists\"
";

/// The one function of a run's JSON.
fn only_function(run: &common::Run) -> Value {
    let parsed: Value = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|error| panic!("the run did not emit JSON: {error}\n{}", run.describe()));
    parsed["functions"]
        .as_array()
        .expect("functions is an array")
        .first()
        .expect("one function")
        .clone()
}

/// **The payoff, and the exact thing that must not go unattributed.**
///
/// Without the pack the bound carries a hole; with it the bound is complete
/// and exact. Both halves are asserted together because the second is only
/// safe to publish while the run also publishes what bought it.
#[test]
fn a_supplied_row_completes_a_bound_that_was_partial() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("m.py", SOURCE)?;
    let pack = project.write("team.toml", TEAM_PACK)?;

    let without = project.check(&source, &["--json"])?;
    let with = project.check(
        &source,
        &["--json", "--signatures", &pack.to_string_lossy()],
    )?;

    assert!(
        !only_function(&without)["holes"]
            .as_array()
            .expect("holes is an array")
            .is_empty(),
        "the call must be a hole on the default table, or this test is \
         measuring nothing:\n{}",
        without.describe()
    );
    assert!(
        only_function(&with)["holes"]
            .as_array()
            .expect("holes is an array")
            .is_empty(),
        "the supplied row did not close the hole:\n{}",
        with.describe()
    );
    Ok(())
}

/// **A bound resting on a supplied row says so, with the file.**
///
/// The premise is what separates this from a bound the engine derived. Told
/// only that a number rests on a pack, the first thing a reader needs is which
/// pack — so the path is asserted, not merely the existence of a premise.
#[test]
fn the_json_attributes_the_bound_to_the_pack_it_rests_on() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("m.py", SOURCE)?;
    let pack = project.write("team.toml", TEAM_PACK)?;

    let run = project.check(
        &source,
        &["--json", "--signatures", &pack.to_string_lossy()],
    )?;
    let function = only_function(&run);
    let premises = function["premises"]
        .as_array()
        .expect("premises is an array");

    let supplied: Vec<&Value> = premises
        .iter()
        .filter(|premise| premise["trust"] == "supplied-signature")
        .collect();
    assert_eq!(
        supplied.len(),
        1,
        "exactly one call rested on a supplied row: {function}"
    );
    assert_eq!(supplied[0]["subject"], "frobnicate");
    assert!(
        supplied[0]["because"]
            .as_str()
            .expect("because is a string")
            .contains("team.toml"),
        "the premise must name the file the row came from, or a reader cannot \
         go and check it: {function}"
    );
    Ok(())
}

/// The same fact in the text, for the reader who never asks for JSON.
#[test]
fn the_text_says_which_row_the_bound_is_believed_on() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("m.py", SOURCE)?;
    let pack = project.write("team.toml", TEAM_PACK)?;

    let run = project.check(
        &source,
        &["--bounds", "--signatures", &pack.to_string_lossy()],
    )?;
    assert!(
        run.mentions("believed on") && run.mentions("frobnicate") && run.mentions("team.toml"),
        "the bound line does not attribute itself:\n{}",
        run.describe()
    );
    Ok(())
}

/// **Nothing is attributed when nothing was supplied.**
///
/// The guard against the premise becoming noise: a default run must report no
/// supplied-signature premise at all, or the marking stops distinguishing
/// anything and readers learn to skip it.
#[test]
fn a_default_run_attributes_nothing_to_a_pack() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("m.py", SOURCE)?;

    let run = project.check(&source, &["--json"])?;
    let function = only_function(&run);
    let supplied = function["premises"]
        .as_array()
        .expect("premises is an array")
        .iter()
        .any(|premise| premise["trust"] == "supplied-signature");
    assert!(
        !supplied,
        "a run with no --signatures reported a supplied premise: {function}"
    );

    let parsed: Value = serde_json::from_str(&run.stdout).expect("JSON");
    assert!(
        parsed["summary"].get("signature_overrides").is_none(),
        "a run with no pack must not carry an empty override list: {}",
        parsed["summary"]
    );
    Ok(())
}

/// **An override is reported with the argument it overrode.**
///
/// The builtin refuses `sorted` and says why in writing. A pack may overrule
/// that — it is the reason to have a pack — but the run must put the two next
/// to each other, because this particular override loosens a row and makes
/// every sort inside a loop report a bound the program can exceed.
#[test]
fn an_override_is_reported_next_to_what_it_replaced() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("m.py", SOURCE)?;
    let pack = project.write("loosen.toml", LOOSENING_PACK)?;

    let run = project.check(
        &source,
        &["--json", "--signatures", &pack.to_string_lossy()],
    )?;
    let parsed: Value = serde_json::from_str(&run.stdout).expect("JSON");
    let overrides = parsed["summary"]["signature_overrides"]
        .as_array()
        .expect("signature_overrides is an array");

    assert_eq!(overrides.len(), 1, "{}", parsed["summary"]);
    assert_eq!(overrides[0]["callee"], "sorted");
    assert!(
        overrides[0]["replaced_why"]
            .as_str()
            .expect("replaced_why is a string")
            .len()
            > 20,
        "the replaced row's argument was dropped, which is the half a reader \
         cannot reconstruct: {}",
        overrides[0]
    );

    let text = project.check(&source, &["--signatures", &pack.to_string_lossy()])?;
    assert!(
        text.mentions("overrides the row for `sorted`"),
        "the override is in the JSON but invisible to a person:\n{}",
        text.describe()
    );
    Ok(())
}

/// Packs apply in the order written, and the loser is still recorded.
#[test]
fn the_last_pack_named_wins_and_the_middle_one_is_still_reported() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("m.py", SOURCE)?;
    let first = project.write("first.toml", TEAM_PACK)?;
    let second = project.write(
        "second.toml",
        "\
[[signature]]
callee = \"frobnicate\"
cost = \"unbounded\"
rebinds_locals = true
mutates_arguments = true
why = \"on reflection it calls out to the network\"
",
    )?;

    let run = project.check(
        &source,
        &[
            "--json",
            "--signatures",
            &first.to_string_lossy(),
            "--signatures",
            &second.to_string_lossy(),
        ],
    )?;
    let parsed: Value = serde_json::from_str(&run.stdout).expect("JSON");
    let overrides = parsed["summary"]["signature_overrides"]
        .as_array()
        .expect("signature_overrides is an array");
    assert_eq!(overrides.len(), 1, "{}", parsed["summary"]);
    assert!(
        overrides[0]["overridden_by"]
            .as_str()
            .expect("a string")
            .contains("second.toml"),
        "the later pack must be the one in force: {}",
        overrides[0]
    );

    assert!(
        !only_function(&run)["holes"]
            .as_array()
            .expect("holes is an array")
            .is_empty(),
        "the second pack refuses the callee, so the hole must be back:\n{}",
        run.describe()
    );
    Ok(())
}

/// **A pack that cannot be read stops the run.**
///
/// The alternative — carrying on without it — reports bounds derived from a
/// table the caller did not choose, and those bounds look exactly like bounds
/// that used the pack. Asked for a pack, landav either uses it or says it
/// could not.
#[test]
fn a_pack_that_cannot_be_read_is_an_error_and_not_a_silent_default() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("m.py", SOURCE)?;

    let missing = project.check(&source, &["--signatures", "nowhere/absent.toml"])?;
    missing.assert_did_not_crash();
    assert_eq!(
        missing.code,
        EXIT_TOOL_ERROR,
        "a missing pack did not stop the run:\n{}",
        missing.describe()
    );
    assert!(
        missing.mentions("absent.toml"),
        "the error must name the file the caller typed:\n{}",
        missing.describe()
    );

    let broken = project.write("broken.toml", "[[signature]]\ncallee = \"x\"\n")?;
    let run = project.check(&source, &["--signatures", &broken.to_string_lossy()])?;
    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "an unparseable pack did not stop the run:\n{}",
        run.describe()
    );
    Ok(())
}

/// A pack from a newer landav is refused by version, end to end.
///
/// `landav-fdk` pins this at the parse boundary; this is the same refusal
/// reaching the operator, because the message only helps if it survives the
/// trip to a terminal.
#[test]
fn a_pack_from_a_newer_landav_says_so_at_the_command_line() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("m.py", SOURCE)?;
    let future = project.write("future.toml", "format = 99\n")?;

    let run = project.check(&source, &["--signatures", &future.to_string_lossy()])?;
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    assert!(
        run.mentions("format 99") && run.mentions("newer landav"),
        "the operator was not told their landav is the old half of the \
         problem:\n{}",
        run.describe()
    );
    Ok(())
}

/// A run that used a pack and found nothing wrong is still clean.
///
/// Attribution must not be mistaken for a finding: a premise says what a bound
/// rests on, not that something is wrong with it.
#[test]
fn attribution_does_not_change_the_verdict() -> io::Result<()> {
    let project = Project::new()?;
    let source = project.write("m.py", SOURCE)?;
    let pack = project.write("team.toml", TEAM_PACK)?;

    let run = project.check(&source, &["--signatures", &pack.to_string_lossy()])?;
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a supplied pack changed the verdict:\n{}",
        run.describe()
    );
    Ok(())
}
