//! Reading LoAT's answer.
//!
//! Pure, like `koat_answers.rs`, and for the same reason: this is where solver
//! output becomes something a user is shown.
//!
//! # LoAT answers in a class, not in a bound
//!
//! KoAT prints a symbolic expression *and* a growth class. LoAT prints the
//! termination-competition answer format and nothing else:
//!
//! ```text
//! WORST_CASE(Omega(n^2),?)
//! ```
//!
//! The first field is the lower bound, the second the upper — LoAT never fills
//! the second in. So a LoAT answer carries a [`landav_solvers::Growth`] and no
//! [`landav_bound::Bound`], and the *only* thing it can be compared with is
//! the class KoAT announces alongside its bound. That is why
//! [`landav_solvers::Growth`] exists as a type rather than as a string on the
//! side of the KoAT parser.
//!
//! # `INF` is a finding, not a failure
//!
//! `WORST_CASE(INF,?)` means LoAT *proved* the runtime is unbounded, which is
//! a positive result about the program. `WORST_CASE(?,?)` and `MAYBE` mean it
//! found nothing. Collapsing the two would report "we learned nothing" as "the
//! program does not terminate", and the pairing rules in `pairing.rs` treat
//! them completely differently.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use landav_solvers::{Answer, Direction, Growth, Solver, SolverError, Timeout, loat_answer};

/// LoAT's build banner, which follows the answer on every run and is not one.
const BANNER: &str = "LoAT:  b41690b39c9c2af407738c15ec08252ec537fe40\n\
                      Yices: 2.6.5\n\
                      \x20      build mode: release\n";

// ---------------------------------------------------------------------------
// the answer vocabulary
// ---------------------------------------------------------------------------

/// Every answer LoAT's complexity mode can print, against the class it means.
#[test]
fn each_answer_in_the_vocabulary_maps_to_its_class() {
    let cases = [
        ("WORST_CASE(Omega(1),?)", Answer::Class(Growth::Constant)),
        (
            "WORST_CASE(Omega(n^1),?)",
            Answer::Class(Growth::Polynomial(1)),
        ),
        (
            "WORST_CASE(Omega(n^2),?)",
            Answer::Class(Growth::Polynomial(2)),
        ),
        (
            "WORST_CASE(Omega(n^17),?)",
            Answer::Class(Growth::Polynomial(17)),
        ),
        (
            "WORST_CASE(Omega(EXP),?)",
            Answer::Class(Growth::Exponential),
        ),
        ("WORST_CASE(INF,?)", Answer::Class(Growth::Unbounded)),
        ("WORST_CASE(?,?)", Answer::Unknown),
        ("MAYBE", Answer::Unknown),
    ];
    for (text, expected) in cases {
        assert_eq!(
            loat_answer::parse(text).ok(),
            Some(expected.clone()),
            "{text}"
        );
    }
}

/// A LoAT answer never carries a symbolic bound. Inventing one — from the
/// class, say — would put a term in front of a user that LoAT never proved.
#[test]
fn a_lower_bound_answer_never_carries_a_symbolic_bound() {
    for text in [
        "WORST_CASE(Omega(n^2),?)",
        "WORST_CASE(INF,?)",
        "WORST_CASE(?,?)",
    ] {
        let parsed = loat_answer::parse(text).ok();
        assert!(
            !matches!(parsed, Some(Answer::Symbolic { .. })),
            "{text} produced a symbolic bound LoAT did not state"
        );
    }
}

/// `INF` is a proof of unboundedness and `?` is the absence of one. They must
/// not collapse into each other in either direction.
#[test]
fn a_proved_infinite_lower_bound_is_not_the_same_as_no_answer() {
    assert_ne!(
        loat_answer::parse("WORST_CASE(INF,?)").ok(),
        loat_answer::parse("WORST_CASE(?,?)").ok(),
        "`INF` proves the program is unbounded; `?` proves nothing at all"
    );
    assert_eq!(
        loat_answer::parse("WORST_CASE(INF,?)").ok(),
        Some(Answer::Class(Growth::Unbounded))
    );
}

/// The banner LoAT prints after its answer is not an answer, and the warning
/// it prints before one is not either.
#[test]
fn the_answer_is_found_among_the_lines_that_are_not_answers() {
    let with_noise = format!(
        "warning: analyzing the complexity of CHCs -- is this intended?\n\
         WORST_CASE(Omega(n^1),?)\n{BANNER}"
    );
    assert_eq!(
        loat_answer::parse(&with_noise).ok(),
        Some(Answer::Class(Growth::Polynomial(1)))
    );
}

/// No answer line at all is a distinct outcome from an unreadable one: the
/// first says the solver declined to speak, the second says this crate could
/// not read what it said.
#[test]
fn output_carrying_no_answer_line_is_reported_as_such() {
    let refused = loat_answer::parse(BANNER);
    assert!(
        matches!(refused, Err(SolverError::NoAnswer { solver }) if solver == Solver::Loat),
        "got {refused:?}"
    );
    assert!(matches!(
        loat_answer::parse(""),
        Err(SolverError::NoAnswer { .. })
    ));
}

/// Two answers is not a stronger answer. Which of them is *the* answer is a
/// guess, and this crate does not guess.
#[test]
fn two_answer_lines_are_refused_rather_than_reconciled() {
    let two = "WORST_CASE(Omega(n^1),?)\nWORST_CASE(Omega(n^2),?)\n";
    assert!(
        loat_answer::parse(two).is_err(),
        "two answers must be refused, not resolved by picking one"
    );
}

/// Text outside the vocabulary, including the shapes a newer LoAT might
/// introduce.
#[test]
fn text_outside_the_verified_vocabulary_is_refused() {
    for text in [
        "WORST_CASE(Omega(n),?)",        // no `^k`: a spelling not yet observed
        "WORST_CASE(Omega(n^2),O(n^3))", // an upper field LoAT has never filled in
        "WORST_CASE(O(n^2),?)",          // an upper bound in the lower field
        "WORST_CASE(Omega(n^x),?)",      // a symbolic degree
        "WORST_CASE(Omega(n^99999999999),?)",
        "WORST_CASE(Omega(2^n),?)",
        "WORST_CASE()",
        "WORST_CASE",
        "YES",
        "NO",
        "Error: unknown format",
    ] {
        assert!(
            loat_answer::parse(text).is_err(),
            "{text:?} is outside the vocabulary this crate has verified and must be refused"
        );
    }
}

// ---------------------------------------------------------------------------
// the command line, and the licence constraint it exists to satisfy
// ---------------------------------------------------------------------------

/// LoAT is invoked exactly the way KoAT is: `fork`/`exec` on a path, with the
/// system handed over as a **file** and the options as **command-line
/// arguments**. This is not a stylistic choice and it is not a fallback for
/// the absence of bindings.
///
/// LoAT is GPL-3.0, forced by a statically linked GPL Yices 2. `landav-core`
/// is Apache-2.0 and `landav-ee` is a commercial BSL 1.1 product. Under the
/// FSF's own test, pipes and command-line arguments make two separate
/// programs, while a shared address space makes one — so linking, embedding,
/// FFI or vendoring LoAT's source would propagate GPL-3.0 onto `landav-core`
/// and fatally onto `landav-ee`. The process boundary *is* the licence
/// boundary.
///
/// This test pins the observable half of that: the argument vector.
/// `crates/landav-solvers/src/lib.rs` carries the constraint in prose, and the
/// crate's manifest is the other half — a build dependency on LoAT would show
/// up there.
#[test]
fn the_loat_command_line_is_pinned() {
    let argv: Vec<String> = Solver::Loat
        .argv(std::path::Path::new("/w/input.koat"), Timeout::DEFAULT)
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        argv,
        vec![
            "--mode".to_owned(),
            "complexity".to_owned(),
            "/w/input.koat".to_owned(),
        ],
        "LoAT takes its input as a file path and its options as arguments; it is never \
         linked, embedded or vendored"
    );
}

/// LoAT has no timeout option of its own, so this crate's wall clock is the
/// only thing that stops it. A `--timeout` appearing here would mean somebody
/// invented a flag.
#[test]
fn loat_is_governed_only_by_this_crates_wall_clock() {
    for secs in [1, 30, 3600] {
        let Ok(timeout) = Timeout::new(secs) else {
            continue;
        };
        let argv = Solver::Loat.argv(std::path::Path::new("/w/i.koat"), timeout);
        assert!(
            !argv.iter().any(|a| a.to_string_lossy().contains("timeout")),
            "LoAT 0.9.10 has no timeout option; at {secs}s the argv was {argv:?}"
        );
    }
}

/// LoAT bounds below. Nothing in the output says so — `WORST_CASE(Omega(...))`
/// is a shape, not a promise — so the direction is a property of the solver,
/// fixed here and read nowhere else.
#[test]
fn loat_is_declared_a_lower_bound_solver() {
    assert_eq!(Solver::Loat.direction(), Direction::Lower);
    assert_eq!(Solver::Loat.program(), "loat");
    assert!(
        Solver::Loat.install_hint().contains("LoAT"),
        "the hint must name the project"
    );
}

/// Output *at* the size cap is read; only output past it is refused.
#[test]
fn the_size_cap_admits_an_answer_that_exactly_reaches_it() {
    let answer = "WORST_CASE(Omega(n^1),?)";
    let padding = "\n".repeat(landav_solvers::MAX_ANSWER_BYTES - answer.len());
    let exactly = format!("{padding}{answer}");
    assert_eq!(exactly.len(), landav_solvers::MAX_ANSWER_BYTES);
    assert_eq!(
        loat_answer::parse(&exactly).ok(),
        Some(Answer::Class(Growth::Polynomial(1))),
        "an answer of exactly MAX_ANSWER_BYTES must be read"
    );
    assert!(
        matches!(
            loat_answer::parse(&format!("\n{exactly}")),
            Err(SolverError::OutputTooLarge { .. })
        ),
        "one byte past the cap must be refused"
    );
}

/// The direction each solver bounds in has a spelling a report can use, and
/// the two spellings are different words.
#[test]
fn each_direction_has_a_distinct_name() {
    assert_eq!(Direction::Upper.as_str(), "upper");
    assert_eq!(Direction::Lower.as_str(), "lower");
    assert_eq!(Direction::Upper.to_string(), "upper");
    assert_eq!(Direction::Lower.to_string(), "lower");
    assert_ne!(Direction::Upper.as_str(), Direction::Lower.as_str());
}

/// A growth class prints as the function it stands for, so a report can wrap
/// it in `O(...)` or `Omega(...)` according to the direction it came from.
#[test]
fn a_growth_class_prints_as_the_function_it_stands_for() {
    assert_eq!(Growth::Constant.to_string(), "1");
    assert_eq!(Growth::Logarithmic.to_string(), "log n");
    assert_eq!(Growth::Polynomial(1).to_string(), "n");
    assert_eq!(Growth::Polynomial(3).to_string(), "n^3");
    assert_eq!(Growth::Exponential.to_string(), "EXP");
    assert_eq!(Growth::Unbounded.to_string(), "INF");
}

/// A polynomial class carries its degree, and the classes that are not
/// polynomial carry none — `log n` has no degree, and neither has `INF`.
#[test]
fn only_the_polynomial_classes_have_a_degree() {
    assert_eq!(Growth::Constant.degree(), Some(0));
    assert_eq!(Growth::Polynomial(1).degree(), Some(1));
    assert_eq!(Growth::Polynomial(7).degree(), Some(7));
    assert_eq!(Growth::Logarithmic.degree(), None);
    assert_eq!(Growth::Exponential.degree(), None);
    assert_eq!(Growth::Unbounded.degree(), None);
}
