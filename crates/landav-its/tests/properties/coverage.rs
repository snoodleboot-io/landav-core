//! `LAN-68`: **clear failure when a construct is out of scope.**
//!
//! `LAN-67` proved that one refused construct produces a named, positioned
//! diagnostic and no transition system. That is not yet enough. The failure
//! mode this story is about is one level up:
//!
//! > four functions out of five refused, the report mentioned the fifth, and
//! > the reader concluded the file was analysed.
//!
//! An integer transition system missing a transition admits fewer executions
//! than the program has, so a bound derived from it can be *exceeded* — the
//! analysis is unsound by omission, and it looks exactly like a clean result.
//! The whole defence is that the refusal is visible, so the bar encoded below
//! is not "a diagnostic exists" but:
//!
//! * the report always carries a **denominator**, so a partial run cannot read
//!   as a whole one;
//! * the percentage **cannot round up** to a claim of completeness;
//! * every refused construct is **named, counted and positioned**, and the
//!   count is accumulated across units and across files;
//! * the constructs that were *never met* are listed too, because that is the
//!   half of a coverage report that says what the number is out of;
//! * a malformed program — a frontend defect — is never filed as a language
//!   construct;
//! * no line of the report is a bare "unknown".
//!
//! # These tests were written against the frozen signatures, before the bodies
//!
//! Every method on [`Coverage`] was `todo!()` when this file was written, so
//! the suite started red on purpose. It is green now, and green is the
//! expectation.

use landav_bound::Origin;
use landav_its::{Construct, Coverage, Its, LoweringError, SourceProgramBuilder, VarName, lower};

// ---------------------------------------------------------------------------
// material
// ---------------------------------------------------------------------------

fn origin(line: u32) -> Origin {
    Origin::new(format!("unit.py:{line}:1"))
}

/// A unit inside the fragment: `x = 1`, and nothing else.
fn lowers(name: &str) -> Its {
    let mut builder = SourceProgramBuilder::new(name, origin(1), vec![]);
    let one = builder.int(1, origin(2));
    let assign = builder.assign(VarName::new("x"), one, origin(2));
    let program = builder.build(vec![assign]);
    match lower(&program) {
        Ok(its) => its,
        Err(error) => panic!("the control unit must lower: {error}"),
    }
}

/// A unit that refuses `construct`, once, at `line`.
fn refuses(name: &str, construct: Construct, line: u32) -> LoweringError {
    let mut builder = SourceProgramBuilder::new(name, origin(1), vec![]);
    let offending = builder.unsupported_stmt(construct, origin(line));
    let program = builder.build(vec![offending]);
    match lower(&program) {
        Ok(_) => panic!("{construct} lowered instead of refusing"),
        Err(error) => error,
    }
}

/// A unit that refuses `construct` and names the specifics.
fn refuses_with_detail(name: &str, construct: Construct, line: u32, detail: &str) -> LoweringError {
    let mut builder = SourceProgramBuilder::new(name, origin(1), vec![]);
    let offending = builder.unsupported_stmt_detailed(construct, detail, origin(line));
    let program = builder.build(vec![offending]);
    match lower(&program) {
        Ok(_) => panic!("{construct} lowered instead of refusing"),
        Err(error) => error,
    }
}

/// A program the frontend built wrong: a body naming a statement that is not
/// in this program's arena. Not a language construct; a frontend defect.
fn malformed(name: &str) -> LoweringError {
    let mut donor = SourceProgramBuilder::new("donor", origin(1), vec![]);
    let stranger = donor.return_stmt(origin(9));
    let _ = donor.build(vec![stranger]);

    let builder = SourceProgramBuilder::new(name, origin(1), vec![]);
    let program = builder.build(vec![stranger]);
    match lower(&program) {
        Ok(_) => panic!("a handle from another program lowered instead of refusing"),
        Err(error) => error,
    }
}

// ---------------------------------------------------------------------------
// the denominator: a partial run must be impossible to read as a whole one
// ---------------------------------------------------------------------------

/// **A run that refused anything is not complete, and says the ratio.**
///
/// The single most important assertion in this file. `is_complete` is the
/// question a driver has to ask before claiming anything about the program,
/// and the summary has to carry both halves of the ratio so that a reader who
/// asks no questions still cannot miss it.
#[test]
fn a_run_that_refused_anything_is_not_complete_and_says_so() {
    let mut coverage = Coverage::new();
    let first = lowers("ok_one");
    let second = lowers("ok_two");
    coverage.record(Ok(&first));
    coverage.record(Ok(&second));
    let refusal = refuses("refuser", Construct::Call, 7);
    coverage.record(Err(&refusal));

    assert!(!coverage.is_complete(), "one unit refused");
    assert_eq!(coverage.units(), 3);
    assert_eq!(coverage.lowered(), 2);
    assert_eq!(coverage.refused(), 1);

    let summary = coverage.summary();
    assert!(
        summary.contains("2 of 3"),
        "the summary must carry the denominator, or a partial run reads as a \
         whole one: {summary}"
    );
    assert!(
        coverage.report().contains("2 of 3"),
        "so must the report: {}",
        coverage.report()
    );
}

/// **A run in which everything lowered is complete, and still says the
/// ratio.**
///
/// The clause is on every run, including the runs with nothing to report, for
/// the same reason the suppression counts are: a number nobody can see change
/// is a number nobody watches.
#[test]
fn a_run_in_which_everything_lowered_is_complete() {
    let mut coverage = Coverage::new();
    for name in ["a", "b", "c"] {
        let its = lowers(name);
        coverage.record(Ok(&its));
    }

    assert!(coverage.is_complete());
    assert_eq!(coverage.units(), 3);
    assert_eq!(coverage.refused(), 0);
    assert_eq!(coverage.refusals(), 0);
    assert_eq!(coverage.percent(), Some(100));
    let summary = coverage.summary();
    assert!(
        summary.contains("3 of 3"),
        "a complete run must state the ratio too: {summary}"
    );
}

/// **The percentage never rounds up into a claim of completeness.**
///
/// 999 units of 1000 is `99`. A report that says `100%` for a run that refused
/// a function has claimed to cover code it produced no transition for, which
/// is the omission this whole story exists to prevent — arriving by way of a
/// rounding rule rather than a missing diagnostic.
#[test]
fn the_percentage_never_rounds_up_to_complete() {
    let mut coverage = Coverage::new();
    let its = lowers("ok");
    for _ in 0..999 {
        coverage.record(Ok(&its));
    }
    let refusal = refuses("refuser", Construct::Call, 7);
    coverage.record(Err(&refusal));

    assert_eq!(coverage.units(), 1000);
    assert_eq!(
        coverage.percent(),
        Some(99),
        "999 of 1000 must not print as 100%"
    );
    assert!(!coverage.is_complete());
}

/// **`100` is reachable only when the run really is complete.**
#[test]
fn a_hundred_percent_implies_complete() {
    let its = lowers("ok");
    let refusal = refuses("refuser", Construct::Call, 7);
    for lowered in 0..12_usize {
        for failed in 0..4_usize {
            let mut coverage = Coverage::new();
            for _ in 0..lowered {
                coverage.record(Ok(&its));
            }
            for _ in 0..failed {
                coverage.record(Err(&refusal));
            }
            if coverage.percent() == Some(100) {
                assert!(
                    coverage.is_complete(),
                    "{lowered} lowered and {failed} failed reported 100%"
                );
            }
            if coverage.is_complete() && coverage.units() > 0 {
                assert_eq!(coverage.percent(), Some(100));
            }
        }
    }
}

/// **A run over no units has no percentage at all.**
///
/// `None`, not `0` and not `100`. A ratio with no denominator is not a number,
/// and printing one either way invents a claim.
#[test]
fn a_run_over_no_units_has_no_percentage() {
    let coverage = Coverage::new();
    assert_eq!(coverage.units(), 0);
    assert_eq!(coverage.percent(), None);
    assert_eq!(coverage.refusals(), 0);
    assert!(coverage.dominant().is_none());
    let summary = coverage.summary();
    assert!(
        !summary.contains('%'),
        "an empty run must not print a percentage: {summary}"
    );
    assert!(
        summary.to_lowercase().contains("no function"),
        "an empty run must say what it did rather than nothing: {summary}"
    );
}

// ---------------------------------------------------------------------------
// what was out of scope, where, and what it means
// ---------------------------------------------------------------------------

/// **Every construct in the vocabulary can be reported, by name and by
/// reason.**
///
/// Driven off [`Construct::all`] rather than a hand-written list, so a variant
/// added later that the report cannot render fails here instead of being
/// quietly forgotten.
#[test]
fn every_construct_is_reported_by_name_and_by_reason() {
    for construct in Construct::all() {
        let mut coverage = Coverage::new();
        let refusal = refuses("unit", *construct, 11);
        coverage.record(Err(&refusal));

        let report = coverage.report();
        assert!(
            report.contains(construct.tag()),
            "{construct} is not named in the report:\n{report}"
        );
        assert!(
            report.contains(construct.describe()),
            "{construct} is reported without saying why:\n{report}"
        );
        assert!(
            report.contains("unit.py:11:1"),
            "{construct} is reported without a position:\n{report}"
        );
        assert_eq!(coverage.count_of(*construct), 1);
        assert_eq!(coverage.constructs(), vec![*construct]);
    }
}

/// **The report says what a refusal means for the result.**
///
/// Naming the construct is half of criterion 2. The other half is the
/// consequence: a refused unit produced no transition system, so nothing is
/// derived from it and no bound covers it. Without that sentence the reader
/// has a list of constructs and no reason to care.
#[test]
fn the_report_says_what_a_refusal_means_for_the_result() {
    let mut coverage = Coverage::new();
    let its = lowers("ok");
    coverage.record(Ok(&its));
    let refusal = refuses("refuser", Construct::Comprehension, 4);
    coverage.record(Err(&refusal));

    let report = coverage.report().to_lowercase();
    assert!(
        report.contains("no transition system"),
        "the report does not say a refused unit produced no system:\n{report}"
    );
    assert!(
        report.contains("no bound"),
        "the report does not say what that means for a bound:\n{report}"
    );
}

/// **Frontend-supplied specifics survive into the report.**
///
/// A [`Construct`] cannot say *which* callee or *which* variable. The detail
/// can, and a report that drops it sends the reader back to the source to work
/// out which of four calls on the line was meant.
#[test]
fn the_report_carries_the_frontend_detail() {
    let mut coverage = Coverage::new();
    let refusal = refuses_with_detail("unit", Construct::Call, 6, "sorted");
    coverage.record(Err(&refusal));

    assert!(
        coverage.report().contains("sorted"),
        "the detail was dropped:\n{}",
        coverage.report()
    );
}

/// **No line of the report is a bare "unknown".**
///
/// Non-negotiable 3, checked against the text a person actually reads rather
/// than against the type that produced it. Every line that mentions a refusal
/// must name a construct from the vocabulary.
#[test]
fn no_line_of_the_report_is_a_bare_unknown() {
    let mut coverage = Coverage::new();
    for (index, construct) in Construct::all().iter().enumerate() {
        let line = u32::try_from(index).unwrap_or(0) + 2;
        let refusal = refuses("unit", *construct, line);
        coverage.record(Err(&refusal));
    }
    let malformed = malformed("built_wrong");
    coverage.record(Err(&malformed));

    let report = coverage.report();
    let tags: Vec<&str> = Construct::all()
        .iter()
        .map(|construct| construct.tag())
        .collect();
    for line in report.lines() {
        let lower = line.to_lowercase();
        assert!(
            !lower.contains("unknown"),
            "a report line says `unknown` without naming anything: {line}"
        );
        if lower.contains("unit.py:") {
            assert!(
                tags.iter().any(|tag| line.contains(tag)),
                "a positioned line names no construct from the vocabulary: {line}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// aggregation: the headline is the count, not the first occurrence
// ---------------------------------------------------------------------------

/// **The same construct refused many times is counted, not repeated once.**
///
/// One refusal is a footnote; four hundred of the same one is the next thing
/// to implement. That only reads off a report that counts.
#[test]
fn refusals_are_counted_across_units() {
    let mut coverage = Coverage::new();
    for line in [3, 9, 14] {
        let refusal = refuses("caller", Construct::Call, line);
        coverage.record(Err(&refusal));
    }
    let attribute = refuses("reader", Construct::Attribute, 5);
    coverage.record(Err(&attribute));

    assert_eq!(coverage.count_of(Construct::Call), 3);
    assert_eq!(coverage.count_of(Construct::Attribute), 1);
    assert_eq!(coverage.count_of(Construct::Coroutine), 0);
    assert_eq!(coverage.refusals(), 4);
    assert_eq!(coverage.refused(), 4, "four units refused");
    assert_eq!(
        coverage.dominant(),
        Some((Construct::Call, 3)),
        "the headline is the most frequent construct"
    );
    assert_eq!(
        coverage.ranked(),
        vec![(Construct::Call, 3), (Construct::Attribute, 1)],
        "most frequent first"
    );
}

/// **Ties rank by tag, not by declaration order.**
///
/// The tags are the stable identifiers — [`Construct::tag`] is written out
/// precisely so that renaming or reordering a variant cannot move a diagnostic
/// code. A ranking that broke ties on the enum's order would reorder a pinned
/// report on a pure readability refactor.
#[test]
fn equal_counts_rank_by_tag() {
    let mut coverage = Coverage::new();
    for construct in [
        Construct::Subscript,
        Construct::Attribute,
        Construct::Coroutine,
    ] {
        let refusal = refuses("unit", construct, 3);
        coverage.record(Err(&refusal));
    }

    let order: Vec<&str> = coverage
        .ranked()
        .into_iter()
        .map(|(construct, _)| construct.tag())
        .collect();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(order, sorted, "equal counts must rank by tag");
}

/// **Reports merge, so a run over a tree aggregates across files.**
#[test]
fn reports_merge_across_files() {
    let mut first = Coverage::new();
    let its = lowers("ok");
    first.record(Ok(&its));
    let one = refuses("a", Construct::Call, 3);
    first.record(Err(&one));

    let mut second = Coverage::new();
    let two = refuses("b", Construct::Call, 8);
    second.record(Err(&two));
    let three = refuses("c", Construct::BitwiseOperator, 2);
    second.record(Err(&three));

    first.merge(&second);

    assert_eq!(first.units(), 4);
    assert_eq!(first.lowered(), 1);
    assert_eq!(first.count_of(Construct::Call), 2);
    assert_eq!(first.count_of(Construct::BitwiseOperator), 1);
    assert_eq!(first.refusals(), 3);
    assert_eq!(first.dominant(), Some((Construct::Call, 2)));
}

/// **Merging an empty report changes nothing.**
///
/// A file holding no function must not move the denominator, or every
/// `__init__.py` in a tree would dilute the number.
#[test]
fn merging_an_empty_report_changes_nothing() {
    let mut coverage = Coverage::new();
    let its = lowers("ok");
    coverage.record(Ok(&its));
    let before = coverage.clone();

    coverage.merge(&Coverage::new());

    assert_eq!(coverage, before);
    assert_eq!(coverage.units(), 1);
}

// ---------------------------------------------------------------------------
// the half of the report that says what the number is out of
// ---------------------------------------------------------------------------

/// **The constructs met and the constructs never met partition the
/// vocabulary.**
///
/// [`Construct::all`] is public for this. "We never met a comprehension" and
/// "we have no name for comprehensions" are different answers, and only the
/// unmet list distinguishes them.
#[test]
fn met_and_never_met_partition_the_vocabulary() {
    let mut coverage = Coverage::new();
    for construct in [Construct::Call, Construct::PatternMatch] {
        let refusal = refuses("unit", construct, 3);
        coverage.record(Err(&refusal));
    }

    let met = coverage.constructs();
    let unmet = coverage.not_encountered();
    assert_eq!(met, vec![Construct::Call, Construct::PatternMatch]);
    assert_eq!(met.len() + unmet.len(), Construct::all().len());
    for construct in Construct::all() {
        assert_eq!(
            met.contains(construct),
            !unmet.contains(construct),
            "{construct} is in neither list, or in both"
        );
    }

    let report = coverage.report();
    assert!(
        report.contains(Construct::Comprehension.tag()),
        "a construct that was never met must still be listed:\n{report}"
    );
    assert!(
        report.contains(&Construct::all().len().to_string()),
        "the report must say how large the vocabulary is:\n{report}"
    );
}

/// **A run that met nothing lists the whole vocabulary as never met.**
#[test]
fn a_clean_run_has_met_no_construct() {
    let mut coverage = Coverage::new();
    let its = lowers("ok");
    coverage.record(Ok(&its));

    assert!(coverage.constructs().is_empty());
    assert_eq!(coverage.not_encountered().len(), Construct::all().len());
}

// ---------------------------------------------------------------------------
// a frontend defect is not a language construct
// ---------------------------------------------------------------------------

/// **A malformed program is counted apart and never as a construct.**
///
/// [`LoweringError::Malformed`] is a frontend bug. Filing it under an
/// unsupported language construct would send somebody to write a lowering rule
/// for a construct that does not exist — and would inflate the very number the
/// team is using to decide what to build next.
#[test]
fn a_malformed_program_is_not_filed_as_a_construct() {
    let mut coverage = Coverage::new();
    let error = malformed("built_wrong");
    coverage.record(Err(&error));

    assert_eq!(coverage.units(), 1);
    assert_eq!(coverage.malformed(), 1);
    assert_eq!(coverage.refused(), 0);
    assert_eq!(coverage.refusals(), 0);
    assert!(!coverage.is_complete(), "it still did not lower");
    for construct in Construct::all() {
        assert_eq!(
            coverage.count_of(*construct),
            0,
            "a frontend defect was filed as {construct}"
        );
    }
    let report = coverage.report();
    assert!(
        report.contains("built_wrong"),
        "the malformed unit is not named:\n{report}"
    );
    assert!(
        report.to_lowercase().contains("frontend"),
        "the report does not say whose defect it is:\n{report}"
    );
}

// ---------------------------------------------------------------------------
// determinism and totality
// ---------------------------------------------------------------------------

/// **Two runs over identical input produce byte-identical reports.**
///
/// A CI baseline diffs the report against itself, and a report whose ordering
/// depends on a hash seed fails a build for no reason — after which the check
/// is removed.
#[test]
fn the_report_is_deterministic() {
    let build = || {
        let mut coverage = Coverage::new();
        let its = lowers("ok");
        coverage.record(Ok(&its));
        for (construct, line) in [
            (Construct::Call, 9),
            (Construct::Attribute, 3),
            (Construct::Call, 3),
            (Construct::Subscript, 12),
        ] {
            let refusal = refuses("unit", construct, line);
            coverage.record(Err(&refusal));
        }
        coverage
    };

    assert_eq!(build().report(), build().report());
    assert_eq!(build().summary(), build().summary());
}

/// **`Display` is the one-line form.**
///
/// The summary is what goes on a run's summary line; the report is what goes
/// behind a flag. Wiring `Display` to the multi-line form would put a
/// twenty-line report inside a one-line summary the first time somebody used
/// `{coverage}`.
#[test]
fn display_is_the_one_line_form() {
    let mut coverage = Coverage::new();
    let refusal = refuses("unit", Construct::Call, 3);
    coverage.record(Err(&refusal));

    assert_eq!(coverage.to_string(), coverage.summary());
    assert_eq!(
        coverage.to_string().lines().count(),
        1,
        "the summary must be one line"
    );
}

/// **Every failure is retrievable, in the order it was offered.**
///
/// A driver that wants to attribute a refusal to a file needs the errors
/// themselves, not only the counts.
#[test]
fn every_failure_is_retrievable_in_order() {
    let mut coverage = Coverage::new();
    let first = refuses("alpha", Construct::Call, 3);
    let second = refuses("beta", Construct::Coroutine, 4);
    coverage.record(Err(&first));
    let its = lowers("gamma");
    coverage.record(Ok(&its));
    coverage.record(Err(&second));

    let names: Vec<String> = coverage
        .failures()
        .iter()
        .map(|error| error.function().to_string())
        .collect();
    assert_eq!(names, vec!["alpha".to_owned(), "beta".to_owned()]);
}

/// **Nothing here panics on a large run.**
///
/// Non-negotiable 2. The report is built from untrusted input by way of a
/// frontend, and a panic destroys the blame path that makes the whole story
/// worth having.
#[test]
fn a_large_run_is_reported_without_panicking() {
    let mut coverage = Coverage::new();
    let its = lowers("ok");
    for index in 0..2_000_u32 {
        if index.is_multiple_of(3) {
            coverage.record(Ok(&its));
        } else {
            let construct =
                Construct::all()[usize::try_from(index).unwrap_or(0) % Construct::all().len()];
            let refusal = refuses("unit", construct, index % 500 + 1);
            coverage.record(Err(&refusal));
        }
    }

    assert_eq!(coverage.units(), 2_000);
    assert!(coverage.percent().is_some_and(|percent| percent < 100));
    assert!(!coverage.report().is_empty());
}

// ---------------------------------------------------------------------------
// closing the gaps a mutation run found
// ---------------------------------------------------------------------------
//
// `cargo mutants` over this lane's files left five mutants alive. Each one is
// a hole in the assertions above rather than a defect in the code, and each is
// closed below by an assertion rather than by relaxing anything. They are kept
// in their own section so that what the mutation run bought is legible.

/// **Merging adds the lowered counts from *both* sides.**
///
/// [`reports_merge_across_files`] merges a report with lowered units into one
/// with none, so it cannot tell `+=` from `-=` on the lowered counter — a merge
/// that *subtracted* the other side's successes would pass it, and would then
/// under-report coverage on every run over more than one file. Both sides carry
/// successes here.
#[test]
fn merging_adds_the_lowered_count_from_both_sides() {
    let its = lowers("ok");
    let mut first = Coverage::new();
    first.record(Ok(&its));
    first.record(Ok(&its));

    let mut second = Coverage::new();
    second.record(Ok(&its));
    second.record(Ok(&its));
    second.record(Ok(&its));
    let refusal = refuses("refuser", Construct::Call, 3);
    second.record(Err(&refusal));

    first.merge(&second);

    assert_eq!(first.lowered(), 5, "both sides' successes must be kept");
    assert_eq!(first.units(), 6);
    assert_eq!(first.percent(), Some(83));
}

/// **A run with no malformed unit says so, and does not mention one.**
///
/// The malformed count is the one number in the report that separates a
/// frontend defect from a language construct, so a count that was always
/// positive would put "1 malformed source program(s)" on the summary of every
/// run in the product and teach readers to ignore the clause. Checked against
/// both the number and the text, on a refusal-only run and on a clean one.
#[test]
fn a_run_with_no_malformed_unit_never_mentions_one() {
    let its = lowers("ok");
    let refusal = refuses("refuser", Construct::Call, 3);

    let mut refused_only = Coverage::new();
    refused_only.record(Err(&refusal));
    assert_eq!(refused_only.malformed(), 0);
    assert_eq!(refused_only.refused(), 1);
    assert!(
        !refused_only.summary().contains("malformed"),
        "a refusal is not a frontend defect: {}",
        refused_only.summary()
    );
    assert!(
        !refused_only.report().contains("malformed"),
        "nor in the report:\n{}",
        refused_only.report()
    );

    let mut clean = Coverage::new();
    clean.record(Ok(&its));
    assert_eq!(clean.malformed(), 0);
    assert!(!clean.summary().contains("malformed"));

    // And the control, so that the clause is not simply absent everywhere.
    let mut broken = Coverage::new();
    let error = malformed("built_wrong");
    broken.record(Err(&error));
    assert_eq!(broken.malformed(), 1);
    assert!(
        broken.summary().contains("malformed"),
        "the summary must name a frontend defect when there is one: {}",
        broken.summary()
    );
}

/// **The report's per-construct ranking is a section of its own, with counts.**
///
/// Every construct name and reason also appears on its positioned line, so a
/// report that dropped the ranking entirely still mentioned both — the
/// aggregate would vanish and no assertion above would notice. The aggregate is
/// the actionable half: it is what says the same construct stopped the lowering
/// eleven times.
#[test]
fn the_ranking_is_a_section_of_the_report_and_carries_counts() {
    let mut coverage = Coverage::new();
    for line in [3, 8] {
        let refusal = refuses("caller", Construct::Call, line);
        coverage.record(Err(&refusal));
    }

    let report = coverage.report();
    assert!(
        report.contains("out of scope, by construct"),
        "the ranking has no section of its own:\n{report}"
    );
    assert!(
        report.contains("×2"),
        "the ranking does not carry the count, so the aggregate is unreadable:\n{report}"
    );
}

/// **The positions are under a heading.**
///
/// A bare list of `file:line:col` lines wedged between the ranking and the
/// vocabulary is not a report anyone can read, and losing the heading is a
/// silent change: every position still prints.
#[test]
fn the_positions_are_under_a_heading() {
    let mut coverage = Coverage::new();
    let refusal = refuses("unit", Construct::Call, 4);
    coverage.record(Err(&refusal));

    let report = coverage.report();
    assert!(
        report.contains("where:"),
        "the positions are printed with no heading:\n{report}"
    );
    let heading = report.lines().position(|line| line.trim() == "where:");
    let position = report.lines().position(|line| line.contains("unit.py:4:1"));
    assert!(
        heading.zip(position).is_some_and(|(head, pos)| head < pos),
        "the heading does not precede the positions:\n{report}"
    );
}
