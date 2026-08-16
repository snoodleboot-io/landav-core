//! Pairing an upper answer with a lower one.
//!
//! # What a pair can say that one solver cannot
//!
//! KoAT bounds above, LoAT bounds below. Put together they say one of three
//! things, and the third is the one that matters:
//!
//! * the classes coincide — the upper bound is **tight**, and that is the only
//!   honest way to claim tightness;
//! * the lower is strictly below the upper — there is a **gap**, reported as
//!   the two classes rather than hidden;
//! * the lower is strictly **above** the upper — the program is claimed to be
//!   both at least `n^2` and at most `n`, which is impossible. One of the two
//!   solvers is wrong and nothing in the output says which.
//!
//! # The contradiction is reported, never reconciled
//!
//! There is an obvious "fix" available: keep the upper bound, since the upper
//! bound is what gets published, and drop the lower one as unreliable. That is
//! precisely wrong. A contradiction is *positive evidence* that the upper
//! bound is too small, and a reported bound the program can exceed is the one
//! failure class with a zero target. Reconciling would convert loud evidence
//! of unsoundness into a silently unsound number.
//!
//! So [`landav_solvers::Analysis::verdict`] refuses. It publishes nothing,
//! names both classes, and leaves the decision to a human.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use landav_bound::Bound;
use landav_solvers::{
    Agreement, Analysis, Answer, ArgMap, Growth, Solver, SolverError, koat_answer,
};

/// A two-variable map, both parameters.
fn map() -> ArgMap {
    ArgMap::new(vec!["i".to_owned(), "n".to_owned()], vec!["n".to_owned()])
        .unwrap_or_else(|_| ArgMap::empty())
}

/// A KoAT report over `text`.
fn upper(text: &str) -> landav_solvers::Report {
    let names = map();
    let answer = match koat_answer::parse(text, &names) {
        Ok(answer) => answer,
        Err(error) => panic!("{text} must parse: {error}"),
    };
    Solver::Koat.report(answer, text, "f", "probe.py:1", &names)
}

/// A LoAT report announcing `growth`.
fn lower(growth: Growth) -> landav_solvers::Report {
    Solver::Loat.report(
        Answer::Class(growth),
        format!("WORST_CASE({growth},?)"),
        "f",
        "probe.py:1",
        &map(),
    )
}

/// A LoAT report that found nothing.
fn lower_unknown() -> landav_solvers::Report {
    Solver::Loat.report(
        Answer::Unknown,
        "WORST_CASE(?,?)",
        "f",
        "probe.py:1",
        &map(),
    )
}

// ---------------------------------------------------------------------------
// the three outcomes
// ---------------------------------------------------------------------------

/// Equal classes are the only claim of tightness this crate makes.
#[test]
fn equal_classes_are_reported_as_a_tight_bound() {
    let Ok(analysis) = Analysis::new(
        Some(upper("Arg_1+2 {O(n)}")),
        Some(lower(Growth::Polynomial(1))),
    ) else {
        panic!("an upper and a lower report must pair");
    };
    assert_eq!(
        analysis.agreement(),
        Agreement::Tight(Growth::Polynomial(1))
    );
    assert!(
        analysis.verdict().is_ok(),
        "a tight pair publishes the upper bound"
    );
}

/// A gap is reported as the two classes, not smoothed into one.
#[test]
fn a_lower_class_below_the_upper_is_reported_as_a_gap() {
    let Ok(analysis) = Analysis::new(
        Some(upper("Arg_1^2+2 {O(n^2)}")),
        Some(lower(Growth::Polynomial(1))),
    ) else {
        panic!("an upper and a lower report must pair");
    };
    assert_eq!(
        analysis.agreement(),
        Agreement::Gap {
            lower: Growth::Polynomial(1),
            upper: Growth::Polynomial(2),
        }
    );
    assert!(
        analysis.verdict().is_ok(),
        "a gap is a tightness statement, not a soundness one; the upper bound still \
         publishes"
    );
}

/// The load-bearing one. A lower bound above the upper bound is impossible, so
/// one of the two answers is wrong; publishing the upper one would publish a
/// bound there is positive evidence the program exceeds.
#[test]
fn a_lower_class_above_the_upper_refuses_to_publish() {
    let Ok(analysis) = Analysis::new(
        Some(upper("Arg_1+2 {O(n)}")),
        Some(lower(Growth::Polynomial(2))),
    ) else {
        panic!("an upper and a lower report must pair");
    };
    assert_eq!(
        analysis.agreement(),
        Agreement::Contradiction {
            lower: Growth::Polynomial(2),
            upper: Growth::Polynomial(1),
        }
    );
    let verdict = analysis.verdict();
    assert!(
        matches!(
            verdict,
            Err(SolverError::Contradiction {
                lower: Growth::Polynomial(2),
                upper: Growth::Polynomial(1),
            })
        ),
        "a contradiction must publish nothing at all, got {verdict:?}"
    );
}

/// A proved-infinite lower bound against a finite upper bound is the sharpest
/// form of the same contradiction: LoAT says the program does not terminate
/// and KoAT says it runs in linear time.
#[test]
fn a_proved_unbounded_lower_against_a_finite_upper_is_a_contradiction() {
    let Ok(analysis) = Analysis::new(
        Some(upper("Arg_1+2 {O(n)}")),
        Some(lower(Growth::Unbounded)),
    ) else {
        panic!("an upper and a lower report must pair");
    };
    assert!(matches!(
        analysis.agreement(),
        Agreement::Contradiction { .. }
    ));
    assert!(analysis.verdict().is_err());
}

/// One solver alone is the ordinary case and must not be dressed up as
/// agreement.
#[test]
fn a_single_answer_claims_no_tightness() {
    let Ok(upper_only) = Analysis::new(Some(upper("Arg_1+2 {O(n)}")), None) else {
        panic!("an upper report alone must pair");
    };
    assert_eq!(upper_only.agreement(), Agreement::Unpaired);
    assert!(upper_only.verdict().is_ok());

    let Ok(neither) = Analysis::new(None, None) else {
        panic!("an empty analysis must be constructible");
    };
    assert_eq!(neither.agreement(), Agreement::Unpaired);
}

/// A lower answer that found nothing cannot contradict anything, so it does
/// not.
#[test]
fn a_lower_solver_that_found_nothing_leaves_the_upper_bound_alone() {
    let Ok(analysis) = Analysis::new(Some(upper("Arg_1+2 {O(n)}")), Some(lower_unknown())) else {
        panic!("an upper and an unknown lower must pair");
    };
    assert_eq!(analysis.agreement(), Agreement::Unpaired);
    assert!(analysis.verdict().is_ok());
}

/// An upper answer of "no bound found" is `omega`, which nothing can exceed,
/// so no lower answer contradicts it.
#[test]
fn an_unknown_upper_bound_is_never_contradicted() {
    let unknown = Solver::Koat.report(Answer::Unknown, "inf {Infinity}", "f", "probe.py:1", &map());
    let Ok(analysis) = Analysis::new(Some(unknown), Some(lower(Growth::Unbounded))) else {
        panic!("an unknown upper and a proved-unbounded lower must pair");
    };
    assert!(!matches!(
        analysis.agreement(),
        Agreement::Contradiction { .. }
    ));
    assert!(
        analysis.verdict().is_ok(),
        "`omega` with blame is still a publishable verdict"
    );
}

// ---------------------------------------------------------------------------
// the directions cannot be swapped
// ---------------------------------------------------------------------------

/// Passing a lower-bound solver's report as the upper bound would report a
/// number the program exceeds by construction. It is refused at the
/// constructor rather than trusted to call sites.
#[test]
fn a_report_cannot_be_filed_under_the_wrong_direction() {
    let swapped = Analysis::new(Some(lower(Growth::Polynomial(1))), None);
    assert!(
        matches!(swapped, Err(SolverError::DirectionMismatch { .. })),
        "a LoAT report in the upper slot must be refused, got {swapped:?}"
    );

    let also_swapped = Analysis::new(None, Some(upper("Arg_1+2 {O(n)}")));
    assert!(
        matches!(also_swapped, Err(SolverError::DirectionMismatch { .. })),
        "a KoAT report in the lower slot must be refused, got {also_swapped:?}"
    );
}

/// The published bound is the upper one, always. A lower bound is a statement
/// about tightness and is never what a caller is handed as *the* bound.
#[test]
fn the_published_bound_is_the_upper_one() {
    let Ok(analysis) = Analysis::new(
        Some(upper("Arg_1+2 {O(n)}")),
        Some(lower(Growth::Polynomial(1))),
    ) else {
        panic!("an upper and a lower report must pair");
    };
    let published = analysis
        .verdict()
        .ok()
        .and_then(|verdict| verdict.bound().cloned());
    assert_eq!(
        published,
        Some(Bound::sum([Bound::var("n"), Bound::constant(2)]))
    );
}

/// Both reports stay reachable after pairing. A caller reporting tightness
/// needs to quote what each solver said, not just the conclusion drawn from
/// the pair.
#[test]
fn both_reports_remain_readable_through_the_analysis() {
    let Ok(analysis) = Analysis::new(
        Some(upper("Arg_1+2 {O(n)}")),
        Some(lower(Growth::Polynomial(1))),
    ) else {
        panic!("an upper and a lower report must pair");
    };
    assert_eq!(
        analysis.upper().map(landav_solvers::Report::solver),
        Some(Solver::Koat)
    );
    assert_eq!(
        analysis.lower().map(landav_solvers::Report::solver),
        Some(Solver::Loat)
    );
    assert_eq!(
        analysis.upper().map(|report| report.raw().to_owned()),
        Some("Arg_1+2 {O(n)}".to_owned())
    );

    let Ok(upper_only) = Analysis::new(Some(upper("Arg_1+2 {O(n)}")), None) else {
        panic!("an upper report alone must pair");
    };
    assert!(upper_only.upper().is_some());
    assert!(upper_only.lower().is_none());
}

/// A lower-bound report never publishes a finite bound of its own. LoAT
/// saying the program is at least quadratic is not a statement that it is at
/// most anything, and a caller that read it as one would report a number the
/// program exceeds.
#[test]
fn a_lower_report_publishes_omega_and_says_why() {
    let report = lower(Growth::Polynomial(2));
    let verdict = report.verdict();
    assert!(
        matches!(verdict, Ok(landav_bound::Verdict::Partial(_))),
        "a lower bound is never `Proved`, got {verdict:?}"
    );
    assert_eq!(
        verdict.ok().and_then(|v| v.bound().cloned()),
        Some(Bound::omega())
    );
    assert!(report.blames().is_some());
}

/// The empty positional map refuses every index, which is what makes it a safe
/// fallback rather than a silent one.
#[test]
fn the_empty_map_declares_nothing_and_refuses_everything() {
    let empty = ArgMap::empty();
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert!(empty.name(0).is_err());
    assert!(koat_answer::parse("Arg_0+1 {O(n)}", &empty).is_err());

    let two = map();
    assert!(!two.is_empty());
    assert_eq!(two.len(), 2);
}
