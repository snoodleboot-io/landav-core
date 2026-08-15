//! `LAN-66` at the process boundary: waiving a rule, and being told about it.
//!
//! * **AC 1** — `# noqa: LAV003` silences that rule on that line.
//! * **AC 2** — `[[tool.landav.suppress]]` silences named rules under a glob.
//! * **AC 3** — every waiver reaches the report and the run summary, so that
//!   none of them can rot unseen.
//!
//! # The exit-code question, asserted rather than assumed
//!
//! A file whose only finding is waived exits `0`. That is the whole feature:
//! the author has judged the cost acceptable and does not want the gate to
//! fail. A gate that failed anyway would be answered by deleting the rule, or
//! by deleting the gate, and landav would learn nothing either way.
//!
//! The accountability moves rather than disappearing. Every waiver is printed
//! and counted, so the fact leaves the exit code and enters the report — which
//! is exactly the seam `E-001` (org policy governance) is sold into: it
//! re-attaches an approver and an expiry to the same record and can then raise
//! the exit code again under a policy somebody owns. `docs/EDITIONS.md` has
//! the argument; this file pins the OSS half of it.
//!
//! A **stale** waiver also exits `0`, and that is asserted too. An unused
//! waiver is not a defect in the code under analysis; it is most often the
//! trace of somebody having *fixed* the code, and failing the build for it
//! punishes the fix. It is also not a stable property of a repository — a
//! pre-commit hook checking one changed file would see almost every waiver as
//! unused — so an exit code that depended on it would depend on which subset
//! of the tree was named.

mod common;

use std::io;

use common::{CLEAN_PY, EXIT_CLEAN, EXIT_FINDINGS, EXIT_TOOL_ERROR, FINDINGS_PY, Project};

/// A single quadratic accumulation, and nothing else.
const ONE_FINDING_PY: &str = r#"
def render(rows):
    out = ""
    for row in rows:
        out += str(row)
    return out
"#;

/// The same defect, waived on the line it happens on, with a reason.
const WAIVED_PY: &str = r#"
def render(rows):
    out = ""
    for row in rows:
        out += str(row)  # noqa: LAV003 - at most a dozen rows; see LAN-70
    return out
"#;

/// A waiver on a line that no longer has a defect under it.
const STALE_WAIVER_PY: &str = r#"
def render(rows):
    pieces = []
    for row in rows:
        pieces.append(str(row))  # noqa: LAV003 - fixed in LAN-71, comment left behind
    return "".join(pieces)
"#;

/// A waiver naming the withdrawn `LAV010`, over a real defect.
const RETIRED_WAIVER_PY: &str = r#"
def render(rows):
    out = ""
    for row in rows:
        out += str(row)  # noqa: LAV010 - the guarded lookup is deliberate
    return out
"#;

/// A waiver naming a code landav has never issued, over a real defect.
const UNKNOWN_WAIVER_PY: &str = r#"
def render(rows):
    out = ""
    for row in rows:
        out += str(row)  # noqa: LAV03
    return out
"#;

/// Directives that belong to other tools, over a real defect.
const FOREIGN_WAIVER_PY: &str = r#"
def render(rows):
    out = ""
    for row in rows:
        out += str(row)  # noqa
    return out


def render_again(rows):
    out = ""
    for row in rows:
        out += str(row)  # noqa: E501
    return out
"#;

/// A `pyproject.toml` waiving `LAV003` everywhere under `src/`.
const PYPROJECT_SUPPRESS_SRC: &str = r#"
[project]
name = "fixture"

[[tool.landav.suppress]]
path = "src/**"
rules = ["LAV003"]
reason = "the report builder is bounded by the page size; tracked in LAN-70"
"#;

// ---------------------------------------------------------------------------
// AC 1 — inline
// ---------------------------------------------------------------------------

/// The feature, end to end: the same file exits `1` without the comment and
/// `0` with it.
#[test]
fn an_inline_waiver_takes_the_run_from_one_to_zero() -> io::Result<()> {
    let project = Project::new()?;
    let noisy = project.write("noisy.py", ONE_FINDING_PY)?;
    let waived = project.write("waived.py", WAIVED_PY)?;

    let before = project.check(&noisy, &[])?;
    let after = project.check(&waived, &[])?;

    before.assert_did_not_crash();
    after.assert_did_not_crash();
    assert_eq!(
        before.code,
        EXIT_FINDINGS,
        "the unwaived file must still report a finding, or this test proves \
         nothing about the waiver.\n{}",
        before.describe()
    );
    assert_eq!(
        after.code,
        EXIT_CLEAN,
        "a file whose only finding is waived must exit 0; a gate that fails \
         anyway is a gate that gets deleted.\n{}",
        after.describe()
    );
    Ok(())
}

/// AC 3, the inline half. The waiver is not merely honoured — it is published,
/// with its code, its position and the reason its author gave.
#[test]
fn a_waived_finding_is_reported_as_a_waiver() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("waived.py", WAIVED_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    let line = run
        .output()
        .lines()
        .find(|line| line.contains("waived.py") && line.contains("LAV003"))
        .map(str::to_owned)
        .unwrap_or_default();
    assert!(
        !line.is_empty(),
        "the waiver was honoured silently; a waiver nobody can see is a waiver \
         that rots, which is what criterion 3 exists to prevent.\n{}",
        run.describe()
    );
    assert!(
        line.contains("see LAN-70"),
        "the report drops the reason, which is the only part a later reader can \
         act on.\n{}",
        run.describe()
    );
    assert!(
        line.contains(":5:"),
        "the report does not name the line the waiver was written on, so nobody \
         can `git blame` it.\n{}",
        run.describe()
    );
    Ok(())
}

/// A waiver covers the line it is written on and no other, so it cannot be
/// used to silence a rule across a file by accident.
#[test]
fn an_inline_waiver_does_not_reach_a_neighbouring_line() -> io::Result<()> {
    let source = r#"
def render(rows):
    out = ""
    for row in rows:  # noqa: LAV003
        out += str(row)
    return out
"#;
    let project = Project::new()?;
    let target = project.write("neighbour.py", source)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_FINDINGS,
        "a waiver on the loop header must not silence the line below it.\n{}",
        run.describe()
    );
    Ok(())
}

/// Blanket suppression is how a codebase goes permanently quiet, so landav has
/// no blanket form and does not honour anybody else's.
#[test]
fn a_bare_noqa_and_a_foreign_code_silence_nothing() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("foreign.py", FOREIGN_WAIVER_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_FINDINGS,
        "`# noqa` and `# noqa: E501` are other tools' directives; honouring \
         either would let a ruff suppression silence landav.\n{}",
        run.describe()
    );
    assert!(
        !run.mentions("E501"),
        "landav narrated another tool's suppression; a linter that comments on \
         every foreign directive is the noisy one.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// AC 2 — per path
// ---------------------------------------------------------------------------

/// A configured waiver silences the named rule under the glob it names, with
/// no comment anywhere in the source.
#[test]
fn a_configured_waiver_silences_the_rule_it_names() -> io::Result<()> {
    let project = Project::new()?;
    project.write("pyproject.toml", PYPROJECT_SUPPRESS_SRC)?;
    let target = project.write("src/report.py", ONE_FINDING_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a per-path waiver over src/ must silence LAV003 there.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("LAV003") && run.mentions("LAN-70"),
        "the waiver and its reason must still be reported.\n{}",
        run.describe()
    );
    Ok(())
}

/// The glob is the extent, and nothing outside it is covered.
#[test]
fn a_configured_waiver_does_not_reach_outside_its_glob() -> io::Result<()> {
    let project = Project::new()?;
    project.write("pyproject.toml", PYPROJECT_SUPPRESS_SRC)?;
    project.write("src/report.py", ONE_FINDING_PY)?;
    project.write("tools/report.py", ONE_FINDING_PY)?;

    let run = project.check(project.root(), &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_FINDINGS,
        "the file outside src/ is not covered and must still be reported.\n{}",
        run.describe()
    );
    assert!(
        run.output()
            .lines()
            .any(|line| line.contains("tools/report.py") && line.contains(": finding: ")),
        "the uncovered file is not named as a finding.\n{}",
        run.describe()
    );
    Ok(())
}

/// A configured waiver waives only the codes it lists.
#[test]
fn a_configured_waiver_only_waives_the_codes_it_lists() -> io::Result<()> {
    let config = r#"
[[tool.landav.suppress]]
path = "**/*.py"
rules = ["LAV002"]
reason = "the allow-list is four entries long and will stay that way"
"#;
    let project = Project::new()?;
    project.write("pyproject.toml", config)?;
    let target = project.write("both.py", FINDINGS_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_FINDINGS,
        "LAV003 was not waived and must still be reported.\n{}",
        run.describe()
    );
    // `: finding: CODE` is the report's finding line; `: suppressed: CODE` is
    // a waiver. Matching on the keyword rather than on the code alone is what
    // keeps the two apart.
    assert!(
        run.output().contains(": finding: LAV003"),
        "LAV003 is missing from the report.\n{}",
        run.describe()
    );
    assert!(
        !run.output().contains(": finding: LAV002"),
        "LAV002 was listed in the waiver and must not be reported as a \
         finding.\n{}",
        run.describe()
    );
    assert!(
        run.output().contains(": suppressed: LAV002"),
        "LAV002 was waived and must be reported as a waiver.\n{}",
        run.describe()
    );
    Ok(())
}

/// A glob that matches no file at all is the stale path a directory move
/// leaves behind. Nothing per-file could name it, so the report is driven from
/// the configuration.
#[test]
fn a_configured_waiver_that_matched_nothing_is_named() -> io::Result<()> {
    let config = r#"
[[tool.landav.suppress]]
path = "legacy/**"
rules = ["LAV003"]
reason = "the legacy tree predates the rule"
"#;
    let project = Project::new()?;
    project.write("pyproject.toml", config)?;
    let target = project.write("clean.py", CLEAN_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a waiver that matched nothing is not a defect in the code.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("legacy/**"),
        "a waiver whose glob matched no file must still be named, or a path \
         that stopped matching goes unnoticed forever.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// AC 3 — the summary, and the waivers that did nothing
// ---------------------------------------------------------------------------

/// The counts are on every run, including the runs with none. A number that
/// appears only when it is non-zero is a number no CI job can watch.
#[test]
fn the_summary_counts_suppressions_on_every_run() -> io::Result<()> {
    let project = Project::new()?;
    let clean = project.write("clean.py", CLEAN_PY)?;
    let waived = project.write("waived.py", WAIVED_PY)?;

    let quiet = project.check(&clean, &[])?;
    let noisy = project.check(&waived, &[])?;

    let summary = |run: &common::Run| {
        run.stdout
            .lines()
            .find(|line| line.starts_with("landav:"))
            .map(str::to_owned)
            .unwrap_or_default()
    };

    assert!(
        summary(&quiet).contains("0 suppressed"),
        "a run with no waivers must still say so.\n{}",
        quiet.describe()
    );
    assert!(
        summary(&noisy).contains("1 suppressed"),
        "the summary does not count the waiver.\n{}",
        noisy.describe()
    );
    Ok(())
}

/// A waiver left behind after the code was fixed is reported and does not fail
/// the build. Failing on it would punish whoever fixed the code.
#[test]
fn a_stale_waiver_is_reported_and_does_not_fail_the_build() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("stale.py", STALE_WAIVER_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "an unused waiver is not a defect in the code under analysis; \
         escalating it is a policy decision and policy is E-001's.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("stale.py") && run.mentions("LAV003"),
        "the unused waiver must be named, or it rots.\n{}",
        run.describe()
    );
    assert!(
        run.stdout
            .lines()
            .any(|line| line.starts_with("landav:") && line.contains("1 stale waiver")),
        "the summary must count it, so that the number can be watched.\n{}",
        run.describe()
    );
    Ok(())
}

/// A retired code is reported *as retired*, not as a typo. The author spelled
/// it correctly and the rule beneath it was withdrawn; those need different
/// edits, and only a burned-number policy can tell them apart.
#[test]
fn a_retired_code_is_named_as_retired_and_waives_nothing() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("retired.py", RETIRED_WAIVER_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_FINDINGS,
        "LAV010 waives nothing, so the LAV003 finding underneath it stands.\n{}",
        run.describe()
    );
    let waiver_line = run
        .output()
        .lines()
        .find(|line| line.contains("LAV010"))
        .map(str::to_owned)
        .unwrap_or_default();
    assert!(
        waiver_line.contains("withdrawn"),
        "the waiver must be reported as naming a withdrawn rule, not as a \
         spelling mistake.\n{}",
        run.describe()
    );
    Ok(())
}

/// A code no landav rule has ever carried is the dangerous case: without a
/// report the author believes the finding is waived, and it is not.
#[test]
fn an_unknown_code_is_named_rather_than_ignored() -> io::Result<()> {
    let project = Project::new()?;
    let target = project.write("typo.py", UNKNOWN_WAIVER_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(run.code, EXIT_FINDINGS, "{}", run.describe());
    assert!(
        run.mentions("LAV03"),
        "a typo'd code must be echoed exactly as written, or the reader cannot \
         see what is wrong with it.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Configuration that cannot be honoured is refused, not guessed at
// ---------------------------------------------------------------------------

/// Every malformed waiver is a tool error naming the entry. A waiver the tool
/// half-understands silently widens or narrows what is suppressed, and both
/// are worse than refusing to run.
#[test]
fn a_malformed_waiver_is_refused_by_name() -> io::Result<()> {
    let cases: [(&str, &str, &str); 7] = [
        (
            "no reason",
            "[[tool.landav.suppress]]\npath = \"src/**\"\nrules = [\"LAV003\"]\n",
            "reason",
        ),
        (
            "empty reason",
            "[[tool.landav.suppress]]\npath = \"src/**\"\nrules = [\"LAV003\"]\nreason = \"  \"\n",
            "reason",
        ),
        (
            "no rules",
            "[[tool.landav.suppress]]\npath = \"src/**\"\nreason = \"because\"\n",
            "rules",
        ),
        (
            "empty rules",
            "[[tool.landav.suppress]]\npath = \"src/**\"\nrules = []\nreason = \"because\"\n",
            "rules",
        ),
        (
            "no path",
            "[[tool.landav.suppress]]\nrules = [\"LAV003\"]\nreason = \"because\"\n",
            "path",
        ),
        (
            "unknown key",
            "[[tool.landav.suppress]]\npath = \"src/**\"\nrules = [\"LAV003\"]\n\
             reason = \"because\"\nexpires = \"2030-01-01\"\n",
            "expires",
        ),
        (
            "not a list of tables",
            "[tool.landav]\nsuppress = \"src/**\"\n",
            "suppress",
        ),
    ];

    for (label, config, named) in cases {
        let project = Project::new()?;
        project.write("pyproject.toml", config)?;
        let target = project.write("src/report.py", ONE_FINDING_PY)?;

        let run = project.check(&target, &[])?;

        run.assert_did_not_crash();
        run.assert_code_is_sanctioned();
        assert_eq!(
            run.code,
            EXIT_TOOL_ERROR,
            "{label}: configuration that cannot be honoured must be refused, \
             never ignored.\n{}",
            run.describe()
        );
        run.assert_explains(named);
    }
    Ok(())
}

/// The `suppress` key does not open the door to every other key.
#[test]
fn an_unknown_setting_beside_suppress_is_still_refused() -> io::Result<()> {
    let config = "[tool.landav]\nfail-on-partial = true\n\n\
                  [[tool.landav.suppress]]\npath = \"src/**\"\n\
                  rules = [\"LAV003\"]\nreason = \"because\"\n";
    let project = Project::new()?;
    project.write("pyproject.toml", config)?;
    let target = project.write("src/report.py", ONE_FINDING_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(run.code, EXIT_TOOL_ERROR, "{}", run.describe());
    run.assert_explains("fail-on-partial");
    Ok(())
}

/// A waiver may name a code that does not exist. That is a fact to report, not
/// a reason to refuse the run — refusing would fail the build over a stale
/// waiver while saying nothing about the code it was protecting.
#[test]
fn a_configured_waiver_naming_an_unknown_code_still_runs() -> io::Result<()> {
    let config = r#"
[[tool.landav.suppress]]
path = "src/**"
rules = ["LAV010", "LAV999"]
reason = "carried over from the previous tool"
"#;
    let project = Project::new()?;
    project.write("pyproject.toml", config)?;
    let target = project.write("src/clean.py", CLEAN_PY)?;

    let run = project.check(&target, &[])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "a stale waiver is not a reason to refuse a verdict about the code.\n{}",
        run.describe()
    );
    assert!(
        run.mentions("LAV010") && run.mentions("LAV999"),
        "both codes must be reported.\n{}",
        run.describe()
    );
    assert!(
        run.stdout
            .lines()
            .any(|line| line.starts_with("landav:") && line.contains("2 stale waivers")),
        "the summary must count both.\n{}",
        run.describe()
    );
    Ok(())
}

/// Nothing in this feature may crash the tool or produce a code outside the
/// frozen contract, whatever is written in the comment or the configuration.
#[test]
fn hostile_waivers_never_crash_the_tool() -> io::Result<()> {
    let sources = [
        "x = 1  # noqa:\n",
        "x = 1  # noqa::::\n",
        "x = 1  # noqa: ,,,,\n",
        "x = 1  # noqa: LAV003,,,LAV003\n",
        "x = 1  # noqa: LAV\u{00e9}003\n",
        "s = \"# noqa: LAV003\"\n",
        "x = 1  # noqa: LAV003 -",
    ];
    let configs = [
        "[[tool.landav.suppress]]\npath = \"**/**/**/**\"\nrules = [\"LAV003\"]\nreason = \"r\"\n",
        "[[tool.landav.suppress]]\npath = \"[\"\nrules = [\"LAV003\"]\nreason = \"r\"\n",
        "[[tool.landav.suppress]]\npath = \"/\"\nrules = [\"\\u00e9\"]\nreason = \"r\"\n",
    ];

    for (index, source) in sources.iter().enumerate() {
        let project = Project::new()?;
        let target = project.write("hostile.py", source)?;
        let run = project.check(&target, &[])?;
        run.assert_did_not_crash();
        run.assert_code_is_sanctioned();
        assert!(!run.stdout.is_empty(), "source case {index} wrote nothing");
    }

    for config in configs {
        let project = Project::new()?;
        project.write("pyproject.toml", config)?;
        let target = project.write("src/report.py", ONE_FINDING_PY)?;
        let run = project.check(&target, &[])?;
        run.assert_did_not_crash();
        run.assert_code_is_sanctioned();
    }
    Ok(())
}
