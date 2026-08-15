//! LAN-60: `--resource` selects which resource a run is about.
//!
//! # Everything here is driven from the registry, never from a list of four
//!
//! The acceptance criteria name `ops|alloc|peak-mem|queries`, but the
//! registered set is *extensible* and the ticket says so explicitly. A suite
//! that spells the four names out asserts what M0 happens to ship, and goes
//! quiet the moment a fifth instance is registered — which is the same drift
//! the criterion 3 error message was written to avoid.
//!
//! So, with one deliberate exception ([`the_criterion_names_four_resources`],
//! which restates the criterion itself as a *subset* check), every test below
//! iterates [`ResourceKind::ALL`] or renders
//! [`ResourceKind::registered_names`]. Registering a resource extends this
//! suite's coverage with no edit here, and removing one from the registry
//! changes what the assertions expect.
//!
//! # The error message assertion, and why it is shaped the way it is
//!
//! [`an_unknown_resource_lists_exactly_the_registered_set`] does not check that
//! the four known names *appear* in the diagnostic. That check passes against a
//! hardcoded message forever, including after an instance is removed from the
//! registry — the message would go on advertising a resource the tool no longer
//! accepts, which is worse than the drift the criterion names. It instead
//! pins the rendered list and both of its boundaries, so a message carrying one
//! name more, or one fewer, than the registry fails.
//!
//! # What `--resource` can honestly do at M0, and what these tests therefore
//! do not assert
//!
//! Nothing in this build derives a cost bound. `landav-its` and
//! `landav-solvers` are empty crates, and the `landav-python` pattern rules are
//! not resource-parameterised. `--resource` therefore selects a semiring that
//! nothing propagates through.
//!
//! There is consequently **no test here that a selected resource produces a
//! number**, because producing one would be a fabrication. What is tested is
//! the opposite: that asking for a bound landav cannot derive is reported as
//! inconclusive and never as clean. A run that answered `0` — "clean under
//! `--resource ops`" — would be a false claim about the code, and the whole
//! point of the exit contract is that a green build is a claim somebody
//! checked.

mod common;

use std::io;

use landav_bound::{BoundError, ResourceKind};

use common::{CLEAN_PY, EXIT_CLEAN, EXIT_FINDINGS, EXIT_TOOL_ERROR, Project};

/// A value that is not, and should never become, a registered resource.
///
/// `cycles` is a plausible cost model somebody could reasonably type, which is
/// the case the diagnostic exists for: a caller who guessed wrong needs to be
/// told what the real set is, not merely that they were wrong.
const NOT_A_RESOURCE: &str = "cycles";

/// The fragment of the report that names a resource, generated from its
/// descriptor.
///
/// This is the *resource* half of the report. It must be present even when
/// another registered resource shares the same algebra — see
/// [`resources_sharing_one_algebra_are_reported_distinctly`].
fn names_resource(kind: ResourceKind) -> String {
    let descriptor = kind.descriptor();
    format!("`{}` ({})", descriptor.id(), descriptor.unit())
}

/// The fragment of the report that names the algebra the resource instantiates.
///
/// Rendered with the delimiters so that `peak` cannot be satisfied by the
/// substring inside `peak-mem`; without them, criterion 2 would pass for
/// `peak-mem` on an implementation that printed nothing about semirings at all.
fn names_semiring(kind: ResourceKind) -> String {
    format!("`{}` semiring", kind.descriptor().semiring().as_str())
}

/// The first pair of registered resources that share one algebra.
///
/// Searched for rather than written down, so that it keeps naming a real pair
/// as the registry grows.
fn a_pair_sharing_one_algebra() -> Option<(ResourceKind, ResourceKind)> {
    for (index, first) in ResourceKind::ALL.iter().enumerate() {
        for second in &ResourceKind::ALL[index + 1..] {
            if first.descriptor().semiring() == second.descriptor().semiring() {
                return Some((*first, *second));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Criterion 1: `--resource` accepts the registered values
// ---------------------------------------------------------------------------

/// The criterion as written, restated as a **subset** check.
///
/// The one place in this file where the four names appear. A subset rather than
/// an equality on purpose: registering a fifth resource must not fail a test
/// that is about the four the story promised.
#[test]
fn the_criterion_names_four_resources() {
    let registered = ResourceKind::registered_names();
    for named in ["ops", "alloc", "peak-mem", "queries"] {
        assert!(
            registered.contains(&named),
            "`--resource {named}` is in LAN-60 criterion 1 but is not registered; \
             the registered set is {registered:?}"
        );
    }
}

/// Every registered value is accepted, whatever it is.
///
/// Driven from the registry: a resource added tomorrow is covered by this test
/// today. "Accepted" is asserted as *not rejected* — the run must not produce
/// the unknown-resource diagnostic and must not fall out of the exit contract.
#[test]
fn every_registered_resource_is_accepted() -> io::Result<()> {
    let project = Project::new()?;
    let clean = project.write("clean.py", CLEAN_PY)?;

    for kind in ResourceKind::ALL {
        let name = kind.id().as_str();
        let run = project.check(&clean, &["--resource", name])?;

        run.assert_did_not_crash();
        run.assert_code_is_sanctioned();
        assert!(
            !run.mentions("unknown resource"),
            "`--resource {name}` is registered but was refused as unknown.\n{}",
            run.describe()
        );
        assert_ne!(
            run.code,
            EXIT_TOOL_ERROR,
            "`--resource {name}` is registered, so it is not a usage error.\n{}",
            run.describe()
        );
    }
    Ok(())
}

/// The value is taken byte for byte: `OPS` is not `ops`.
///
/// [`landav_bound::ResourceId`] round-trips its name exactly, because the id is
/// what an incremental cache is keyed on; a `--resource` that case-folded would
/// make `peak-mem` and `PEAK-MEM` two keys for one analysis, or — worse, in the
/// other direction — one key for two. So a near miss is refused, with the
/// registered set, rather than guessed at.
#[test]
fn resource_values_are_matched_exactly() -> io::Result<()> {
    let project = Project::new()?;
    let clean = project.write("clean.py", CLEAN_PY)?;

    for near_miss in ["OPS", "Ops", " ops", "ops ", "peak_mem", "peakmem"] {
        let run = project.check(&clean, &["--resource", near_miss])?;

        run.assert_did_not_crash();
        assert_eq!(
            run.code,
            EXIT_TOOL_ERROR,
            "`--resource {near_miss}` is not a registered value and must be \
             refused, not folded onto one that is.\n{}",
            run.describe()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Criterion 2: each maps to a registered semiring instance
// ---------------------------------------------------------------------------

/// Criterion 2, made observable: the run says which algebra the selected
/// resource instantiates, and it is the one the registry recorded.
///
/// The expected text is the descriptor's own [`landav_bound::SemiringId`], so
/// this cannot be satisfied by printing a fixed algebra name, and it follows a
/// registry edit without an edit here.
#[test]
fn every_resource_reports_the_semiring_it_maps_to() -> io::Result<()> {
    let project = Project::new()?;
    let clean = project.write("clean.py", CLEAN_PY)?;

    for kind in ResourceKind::ALL {
        let run = project.check(&clean, &["--resource", kind.id().as_str()])?;

        run.assert_did_not_crash();
        assert!(
            run.output().contains(&names_resource(*kind)),
            "the run does not name the resource it was asked about; expected \
             {}.\n{}",
            names_resource(*kind),
            run.describe()
        );
        assert!(
            run.output().contains(&names_semiring(*kind)),
            "the run does not name the algebra `{}` maps to; expected {}.\n{}",
            kind.id(),
            names_semiring(*kind),
            run.describe()
        );
    }
    Ok(())
}

/// Three registered resources share one algebra, and the report must still tell
/// them apart.
///
/// This is the [`landav_bound::CacheKeyMaterial`] trap restated at the CLI. Its
/// documentation records what happens to anything keyed on the *semiring*
/// rather than the *resource*: it "serves the allocation bound as the operation
/// count, silently, with plausible numbers". A report that identified a run by
/// its algebra would make `--resource ops` and `--resource alloc` produce byte
/// identical output, which is the same mistake one layer up — and the operator
/// would have no way to see it, because both answers look right.
#[test]
fn resources_sharing_one_algebra_are_reported_distinctly() -> io::Result<()> {
    // Not a skip: the registry is *designed* around several resources over one
    // algebra, and `registry.rs` pins `ops` and `alloc` sharing `additive`. A
    // registry with no such pair has lost the property this test guards, and
    // must say so out loud rather than passing vacuously.
    let pair = a_pair_sharing_one_algebra();
    assert!(
        pair.is_some(),
        "no two registered resources share an algebra, so the distinction this \
         test guards no longer exists in the registry"
    );
    let Some((first, second)) = pair else {
        return Ok(());
    };

    let project = Project::new()?;
    let clean = project.write("clean.py", CLEAN_PY)?;

    let one = project.check(&clean, &["--resource", first.id().as_str()])?;
    let other = project.check(&clean, &["--resource", second.id().as_str()])?;

    one.assert_did_not_crash();
    other.assert_did_not_crash();

    assert_eq!(
        first.descriptor().semiring(),
        second.descriptor().semiring(),
        "the pair under test must share an algebra for this to mean anything"
    );
    assert_ne!(
        one.output(),
        other.output(),
        "`--resource {}` and `--resource {}` produced identical output over the \
         same file; they share the `{}` algebra, so a report identified by the \
         semiring rather than by the resource cannot be told apart by anyone \
         reading it",
        first.id(),
        second.id(),
        first.descriptor().semiring().as_str()
    );
    assert!(
        one.output().contains(&names_resource(first))
            && !one.output().contains(&names_resource(second)),
        "the run for `{}` does not name itself, or names its algebra-mate \
         instead.\n{}",
        first.id(),
        one.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Criterion 3: an unknown value gives a helpful error listing the registered set
// ---------------------------------------------------------------------------

/// The message lists **exactly** the registered set, and is generated from it.
///
/// The expected list is rendered here from
/// [`ResourceKind::registered_names`], so:
///
/// * registering a resource extends the expected list with no edit to any
///   message and no edit to this test;
/// * removing one from the registry shortens the expected list, and a message
///   that kept the old name fails — the removed name would follow the rendered
///   list, which the boundary assertions below reject.
///
/// Both boundaries are checked. Without them the assertion is a prefix match:
/// dropping `queries` from the registry leaves `ops, alloc, peak-mem` as a
/// prefix of a stale hardcoded `ops, alloc, peak-mem, queries`, and a
/// contains-check would pass while the tool advertised a value it refuses.
#[test]
fn an_unknown_resource_lists_exactly_the_registered_set() -> io::Result<()> {
    let project = Project::new()?;
    let clean = project.write("clean.py", CLEAN_PY)?;

    let run = project.check(&clean, &["--resource", NOT_A_RESOURCE])?;
    run.assert_did_not_crash();

    let registered = ResourceKind::registered_names().join(", ");
    let output = run.output();

    assert!(
        output.contains(&registered),
        "the diagnostic does not list the registered set; expected to find \
         `{registered}`.\n{}",
        run.describe()
    );

    let before = output.split(registered.as_str()).next().unwrap_or_default();
    let after = output.split(registered.as_str()).nth(1).unwrap_or_default();
    assert!(
        !after.starts_with(", "),
        "the diagnostic lists a resource after the registered set, so it is \
         advertising a value the registry does not contain.\n{}",
        run.describe()
    );
    assert!(
        !before.ends_with(", "),
        "the diagnostic lists a resource before the registered set, so it is \
         advertising a value the registry does not contain.\n{}",
        run.describe()
    );
    Ok(())
}

/// The rejected value is named, and the message is the registry's own.
///
/// Blame is mandatory (`CONTRIBUTING.md` rule 3): "unknown resource" without
/// the value is not actionable when the value came from a CI variable nobody
/// can see. The expected text is [`BoundError::UnknownResource`]'s own
/// rendering, so this test says nothing about wording and everything about
/// where the message comes from — a second, hand-written message in the CLI
/// fails it.
#[test]
fn an_unknown_resource_is_refused_with_blame() -> io::Result<()> {
    let project = Project::new()?;
    let clean = project.write("clean.py", CLEAN_PY)?;

    let run = project.check(&clean, &["--resource", NOT_A_RESOURCE])?;

    run.assert_did_not_crash();
    run.assert_code_is_sanctioned();
    assert_eq!(
        run.code,
        EXIT_TOOL_ERROR,
        "an unusable `--resource` is a usage error: the tool could not do what \
         it was asked, so it must not report a verdict.\n{}",
        run.describe()
    );
    run.assert_explains(NOT_A_RESOURCE);

    let from_the_registry = BoundError::UnknownResource {
        got: NOT_A_RESOURCE.to_owned(),
        known: ResourceKind::registered_names(),
    }
    .to_string();
    assert!(
        run.output().contains(&from_the_registry),
        "the diagnostic is not the registry's own message; expected to find \
         `{from_the_registry}`.\n{}",
        run.describe()
    );
    Ok(())
}

/// An unusable `--resource` refuses the run outright; it does not analyse
/// anyway.
///
/// The same reasoning `config.rs` uses for a configuration it cannot honour:
/// carrying on under settings the caller did not choose still produces a
/// verdict, and nothing in the output says which question it answered. The run
/// summary is the tell — if it is there, the tool analysed something after
/// being told to stop.
#[test]
fn an_unknown_resource_analyses_nothing() -> io::Result<()> {
    let project = Project::new()?;
    let clean = project.write("clean.py", CLEAN_PY)?;

    let run = project.check(&clean, &["--resource", NOT_A_RESOURCE])?;

    run.assert_did_not_crash();
    assert!(
        !run.mentions("analysed under"),
        "the run reported a verdict for a resource it had already refused.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// What the flag honestly does at M0
// ---------------------------------------------------------------------------

/// Selecting a resource landav cannot yet bound is inconclusive, never clean.
///
/// [`CLEAN_PY`] exits `0` with no `--resource`, and must stop doing so the
/// moment a bound is asked for that this build does not derive. Exit `0` is a
/// claim — "analysis ran and every bound held" — and the analysis tier that
/// would substantiate it (`landav-its`, `landav-solvers`) is not implemented.
///
/// The control assertion is the other half: without the flag the same file
/// still reports clean, so this is a consequence of the question that was
/// asked and not a regression in the default path.
#[test]
fn a_selected_resource_is_inconclusive_rather_than_clean() -> io::Result<()> {
    let project = Project::new()?;
    let clean = project.write("clean.py", CLEAN_PY)?;

    let control = project.check(&clean, &[])?;
    control.assert_did_not_crash();
    assert_eq!(
        control.code,
        EXIT_CLEAN,
        "the default path must be unchanged by this story.\n{}",
        control.describe()
    );

    for kind in ResourceKind::ALL {
        let run = project.check(&clean, &["--resource", kind.id().as_str()])?;

        run.assert_did_not_crash();
        assert_ne!(
            run.code,
            EXIT_CLEAN,
            "`--resource {}` reported clean, which claims a bound in {} that \
             this build derives no part of.\n{}",
            kind.id(),
            kind.descriptor().unit(),
            run.describe()
        );
        assert_eq!(
            run.code,
            EXIT_FINDINGS,
            "an unanswerable question about the code is a result about the \
             code, not a tool failure.\n{}",
            run.describe()
        );
        assert!(
            run.mentions("inconclusive"),
            "the run must say that nothing was concluded for `{}`, or the \
             exit code is the only signal and it is ambiguous.\n{}",
            kind.id(),
            run.describe()
        );
    }
    Ok(())
}

/// `--help` lists the registered set, generated, and does not overclaim.
///
/// Two properties, and the second is the one that rots quietly. The help text
/// is the only description of the flag most callers will ever read; if it
/// promises a bound the tool does not derive, every one of them will read the
/// inconclusive result as a bug.
#[test]
fn the_help_is_generated_and_does_not_promise_a_bound() -> io::Result<()> {
    let project = Project::new()?;

    let run = project.run(&["check", "--help"])?;

    run.assert_did_not_crash();
    assert_eq!(
        run.code,
        EXIT_CLEAN,
        "`--help` is not a failure: the caller asked for output and got it.\n{}",
        run.describe()
    );

    for kind in ResourceKind::ALL {
        let descriptor = kind.descriptor();
        assert!(
            run.output().contains(descriptor.id().as_str()),
            "`--help` does not list the registered resource `{}`.\n{}",
            descriptor.id(),
            run.describe()
        );
        assert!(
            run.output().contains(descriptor.summary()),
            "`--help` does not carry the registry's summary for `{}`, so it is \
             describing the flag from a second, hand-written list.\n{}",
            descriptor.id(),
            run.describe()
        );
    }
    assert!(
        run.mentions("no bound is derived"),
        "`--help` describes `--resource` without saying that no bound is \
         derived for any resource in this build, so it promises an answer the \
         tool does not have.\n{}",
        run.describe()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Never panic
// ---------------------------------------------------------------------------

/// Hostile `--resource` values produce a diagnostic, never a panic and never a
/// fourth exit code.
///
/// The value reaches the tool from CI configuration and shell interpolation, so
/// the empty string, an embedded flag and a very long argument are all things
/// that actually arrive. Every one of them is a usage error; none of them is
/// permitted to leave the sanctioned code set or to unwind out of `main`.
#[test]
fn hostile_resource_values_are_refused_without_panicking() -> io::Result<()> {
    let project = Project::new()?;
    let clean = project.write("clean.py", CLEAN_PY)?;
    let long = "o".repeat(64 * 1024);

    let hostile = [
        "",
        " ",
        "ops,alloc",
        "ops alloc",
        "--config",
        "-",
        "../ops",
        "ops\n",
        "оps", // Cyrillic "о"
        "🙂",
        long.as_str(),
    ];

    for value in hostile {
        let run = project.check(&clean, &["--resource", value])?;

        run.assert_did_not_crash();
        run.assert_code_is_sanctioned();
        assert_eq!(
            run.code,
            EXIT_TOOL_ERROR,
            "a `--resource` value outside the registry must be a usage error.\n{}",
            run.describe()
        );
        assert!(
            !run.stderr.trim().is_empty(),
            "a refused `--resource` wrote nothing to stderr, so CI shows an \
             exit code and no reason.\n{}",
            run.describe()
        );
    }
    Ok(())
}

/// `--resource` does not widen the outcome space.
///
/// Every registered value, over every shape of target the acceptance suite
/// knows about, still lands inside the frozen `{0, 1, 2}` contract. This is the
/// LAN-61 criterion re-run under the new flag: a story that adds an argument
/// has to show it did not add a fourth code with it.
#[test]
fn resource_selection_stays_inside_the_frozen_code_set() -> io::Result<()> {
    let project = Project::new()?;
    let clean = project.write("src/clean.py", CLEAN_PY)?;
    let empty_dir = project.mkdir("nothing_here")?;
    let missing = project.root().join("absent.py");
    let targets = [
        clean,
        empty_dir,
        missing,
        project.root().join("src"),
        project.root().to_path_buf(),
    ];

    for kind in ResourceKind::ALL {
        for target in &targets {
            let run = project.check(target, &["--resource", kind.id().as_str()])?;
            run.assert_did_not_crash();
            run.assert_code_is_sanctioned();
        }
    }
    Ok(())
}
