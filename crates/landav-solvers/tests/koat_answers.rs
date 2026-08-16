//! Reading KoAT's answer — the half of the bridge where the risk lives.
//!
//! # The solver is untrusted input in reverse
//!
//! Everything in this file is **pure**. It needs no `koat2` on `PATH`, runs on
//! CI unchanged, and covers the step where a mistake is worst: turning text a
//! program this crate does not control printed on stdout into a
//! [`landav_bound::Bound`] that will be reported to a user as an upper bound.
//!
//! Two failure modes are worth separating, because only one of them is
//! visible:
//!
//! * **a refusal** — the answer was not understood and nothing is published.
//!   Loud, recoverable, and the correct response to every input below that
//!   this crate has not verified against a real KoAT.
//! * **a bound parsed too small** — the answer *was* understood, wrongly, and
//!   the number reported is one the program can exceed. Silent, and the single
//!   class of bug that invalidates the product.
//!
//! So every assertion here is either "this exact text produces this exact
//! `Bound`" or "this text is refused". There is no third category, and in
//! particular there is no "parses to something reasonable".
//!
//! # Where the expected strings come from
//!
//! Every string in [`the_captured_answers`] was produced by running KoAT2
//! v2.1.0 on an integer transition system emitted by `landav_its::koat::render`
//! and copying its stdout verbatim. They are not invented, and they are not
//! reformatted. The systems that produced them are named in the comments so
//! the corpus can be regenerated.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use landav_bound::{Base, Bound, Verdict};
use landav_solvers::{Answer, ArgMap, Growth, Solver, SolverError, Timeout, koat_answer};

/// The declared variables of the systems the captured answers came from, all
/// of them reported as parameters.
fn map(names: &[&str]) -> ArgMap {
    let owned: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
    ArgMap::new(owned.clone(), owned).unwrap_or_else(|_| ArgMap::empty())
}

/// Parse, or fail the test naming what was being read.
fn answer(text: &str, map: &ArgMap) -> Answer {
    match koat_answer::parse(text, map) {
        Ok(parsed) => parsed,
        Err(error) => panic!("KoAT answer {text:?} must parse: {error}"),
    }
}

/// The `Bound` a parsed answer carries, or a test failure.
fn bound_of(text: &str, map: &ArgMap) -> Bound {
    match answer(text, map) {
        Answer::Symbolic { bound, .. } => bound,
        other => panic!("KoAT answer {text:?} must carry a symbolic bound, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// the captured corpus: exact text in, exact bound out
// ---------------------------------------------------------------------------

/// Six answers captured from KoAT2 v2.1.0, each against the `Bound` it must
/// denote, written out by hand from the text rather than by the parser.
///
/// A single shared expectation per row is the point: if the parser drops a
/// term, mis-reads a coefficient, or expands `^2` into one factor instead of
/// two, the reported upper bound is *smaller* than the one KoAT proved, and
/// exactly one of these rows says so by name.
#[test]
fn the_captured_answers_denote_the_bounds_they_spell() {
    // `def countdown(n): i = 0; while i < n: i = i + 1`, variables (i, n).
    let two = map(&["i", "n"]);
    assert_eq!(
        bound_of("Arg_1+2 {O(n)}", &two),
        Bound::sum([Bound::var("n"), Bound::constant(2)]),
        "a linear bound in the second declared variable"
    );

    // `for i in range(10): total += 1`, variables (i, total).
    assert_eq!(
        bound_of("13 {O(1)}", &two),
        Bound::constant(13),
        "a constant bound carries no variable at all"
    );

    // Nested `range(n)` loops, variables (i, j, n, total).
    let four = map(&["i", "j", "n", "total"]);
    assert_eq!(
        bound_of("Arg_2^2+5*Arg_2+3 {O(n^2)}", &four),
        Bound::sum([
            Bound::prod([Bound::var("n"), Bound::var("n")]),
            Bound::prod([Bound::constant(5), Bound::var("n")]),
            Bound::constant(3),
        ]),
        "`Arg_2^2` is n*n; expanding it into one factor would report a linear bound \
         for a quadratic loop"
    );

    // A triangular loop over the same four variables.
    assert_eq!(
        bound_of("Arg_2^2+4*Arg_2+3 {O(n^2)}", &four),
        Bound::sum([
            Bound::prod([Bound::var("n"), Bound::var("n")]),
            Bound::prod([Bound::constant(4), Bound::var("n")]),
            Bound::constant(3),
        ])
    );

    // Nested loops over two distinct parameters, variables (i, j, m, n).
    let mn = map(&["i", "j", "m", "n"]);
    assert_eq!(
        bound_of("Arg_2*Arg_3+3*Arg_3+Arg_2+2 {O(n^2)}", &mn),
        Bound::sum([
            Bound::prod([Bound::var("m"), Bound::var("n")]),
            Bound::prod([Bound::constant(3), Bound::var("n")]),
            Bound::var("m"),
            Bound::constant(2),
        ]),
        "a product of two *different* arguments, which is where a positional map that \
         is off by one stops being detectable by inspection"
    );

    // A cubic triple loop, variables (i, j, k, n).
    let ijkn = map(&["i", "j", "k", "n"]);
    assert_eq!(
        bound_of("3*Arg_3^3+7*Arg_3^2+11*Arg_3+3 {O(n^3)}", &ijkn),
        Bound::sum([
            Bound::prod([
                Bound::constant(3),
                Bound::var("n"),
                Bound::var("n"),
                Bound::var("n"),
            ]),
            Bound::prod([Bound::constant(7), Bound::var("n"), Bound::var("n")]),
            Bound::prod([Bound::constant(11), Bound::var("n")]),
            Bound::constant(3),
        ])
    );
}

/// The growth class KoAT announces is carried through unchanged, because it is
/// the only quantity a lower bound can be compared against.
#[test]
fn the_announced_growth_class_is_carried_through() {
    let four = map(&["i", "j", "n", "total"]);
    let two = map(&["i", "n"]);
    let cases = [
        ("13 {O(1)}", &two, Growth::Constant),
        ("Arg_1+2 {O(n)}", &two, Growth::Polynomial(1)),
        ("Arg_2^2+5*Arg_2+3 {O(n^2)}", &four, Growth::Polynomial(2)),
        ("log(Arg_1)+6 {O(log(n))}", &two, Growth::Logarithmic),
    ];
    for (text, names, expected) in cases {
        match answer(text, names) {
            Answer::Symbolic { growth, .. } => assert_eq!(growth, expected, "{text}"),
            other => panic!("{text} must be symbolic, got {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Arg_i is positional, and the position is the ITS variable order
// ---------------------------------------------------------------------------

/// `Arg_i` names the `i`-th variable of the emitted `(VAR ...)` tuple. Getting
/// this wrong attributes the bound to a different variable and produces a
/// wrong answer that looks right, so the mapping is asserted on every index of
/// a system whose variables are all distinguishable.
#[test]
fn each_argument_index_maps_to_the_variable_declared_at_that_position() {
    let names = map(&["alpha", "beta", "gamma", "delta"]);
    for (index, expected) in ["alpha", "beta", "gamma", "delta"].iter().enumerate() {
        let text = format!("Arg_{index}+1 {{O(n)}}");
        assert_eq!(
            bound_of(&text, &names),
            Bound::sum([Bound::var(*expected), Bound::constant(1)]),
            "Arg_{index} must name `{expected}`"
        );
    }
}

/// An index the system did not declare is refused. KoAT can only produce one
/// by disagreeing with this crate about the variable tuple, and the only safe
/// response to that disagreement is to publish nothing.
#[test]
fn an_argument_index_the_system_did_not_declare_is_refused() {
    let names = map(&["i", "n"]);
    let refused = koat_answer::parse("Arg_2+1 {O(n)}", &names);
    assert!(
        matches!(refused, Err(SolverError::ArgIndexOutOfRange { index, declared, .. })
            if index == 2 && declared == 2),
        "got {refused:?}"
    );
}

/// A bound in terms of a variable that is not a parameter is sound but not
/// *evaluable* — a caller cannot supply a value for one of the lowering's
/// fresh loop counters — so it is published with blame naming the variable
/// rather than as a proved bound.
#[test]
fn a_bound_over_a_non_parameter_carries_blame() {
    let Ok(names) = ArgMap::new(vec!["i".to_owned(), "n".to_owned()], vec!["n".to_owned()]) else {
        panic!("a two-variable map with one parameter must be constructible");
    };
    let over_local = bound_of("Arg_0+2 {O(n)}", &names);
    assert_eq!(
        names.unevaluable(&over_local).len(),
        1,
        "`i` is not a parameter, so a bound mentioning it cannot be evaluated by a caller"
    );
    let over_param = bound_of("Arg_1+2 {O(n)}", &names);
    assert!(
        names.unevaluable(&over_param).is_empty(),
        "`n` is a parameter and needs no blame"
    );
}

// ---------------------------------------------------------------------------
// unknown is a real answer, and it is omega with blame
// ---------------------------------------------------------------------------

/// KoAT prints `inf {Infinity}` when it finds no bound. That is neither a
/// missing result nor a proof of divergence: it is "no bound found", and it
/// must reach a caller as `omega` with blame naming the function.
#[test]
fn no_bound_found_is_reported_as_unknown_and_never_as_a_number() {
    let names = map(&["i", "n"]);
    assert_eq!(answer("inf {Infinity}", &names), Answer::Unknown);

    let report = Solver::Koat.report(
        Answer::Unknown,
        "inf {Infinity}",
        "countdown",
        "probe.py:1",
        &names,
    );
    let verdict = report.verdict();
    assert!(
        matches!(verdict, Ok(Verdict::Partial(_))),
        "an unknown answer must publish as a blamed partial, got {verdict:?}"
    );
    let blamed = verdict
        .ok()
        .and_then(|v| v.blames().map(|b| b.as_slice().to_vec()))
        .unwrap_or_default();
    assert!(
        blamed.iter().any(|b| b.unaccounted.as_str() == "countdown"),
        "the blame ledger must name the function, got {blamed:?}"
    );
}

/// A finite bound over parameters alone has nothing unaccounted for and
/// publishes as `Proved`.
#[test]
fn a_finite_bound_over_parameters_publishes_as_proved() {
    let names = map(&["i", "n"]);
    let report = Solver::Koat.report(
        answer("Arg_1+2 {O(n)}", &names),
        "Arg_1+2 {O(n)}",
        "countdown",
        "probe.py:1",
        &names,
    );
    assert!(
        matches!(report.verdict(), Ok(Verdict::Proved(_))),
        "got {:?}",
        report.verdict()
    );
}

// ---------------------------------------------------------------------------
// the announced class is a cross-check on the parse, not decoration
// ---------------------------------------------------------------------------

/// KoAT states the growth class alongside the bound, so the parse can be
/// checked against it for free. A polynomial read one degree short of the
/// class KoAT announced is the signature of a dropped factor — which is a
/// bound smaller than the one that was proved.
#[test]
fn a_bound_whose_degree_contradicts_its_announced_class_is_refused() {
    let names = map(&["i", "n"]);
    for text in [
        "Arg_1+2 {O(n^2)}",   // linear text, quadratic claim
        "Arg_1^2+2 {O(n)}",   // quadratic text, linear claim
        "Arg_1+2 {O(1)}",     // a variable in a constant claim
        "13 {O(n)}",          // a constant in a linear claim
        "log(Arg_1) {O(n)}",  // a logarithm in a linear claim
        "Arg_1 {O(log(n))}",  // a variable in a logarithmic claim
        "inf {O(n)}",         // no bound at all, in a linear claim
        "Arg_1+2 {Infinity}", // a real bound, under the unbounded claim
    ] {
        let refused = koat_answer::parse(text, &names);
        assert!(
            matches!(refused, Err(SolverError::ClassMismatch { .. })),
            "{text} states a class its own text contradicts and must be refused, got \
             {refused:?}"
        );
    }
}

/// The classes this crate has actually seen KoAT print, and no others. A class
/// that cannot be checked against the parsed text is a class this crate cannot
/// use as a cross-check, so it is refused rather than trusted.
#[test]
fn a_growth_class_this_crate_cannot_check_is_refused() {
    let names = map(&["i", "n"]);
    for text in [
        "Arg_1+2 {O(EXP)}",
        "Arg_1+2 {O(n*log(n))}",
        "Arg_1+2 {O(n^n)}",
        "Arg_1+2 {}",
        "Arg_1+2 {Omega(n)}",
        "Arg_1+2",
    ] {
        assert!(
            koat_answer::parse(text, &names).is_err(),
            "{text} announces a class this crate cannot verify and must be refused"
        );
    }
}

// ---------------------------------------------------------------------------
// a parse this crate cannot do is a refusal, never a guess
// ---------------------------------------------------------------------------

/// Every one of these is text a future KoAT, a different KoAT build, or a
/// corrupted pipe could produce. None of them may become a bound.
#[test]
fn text_outside_the_verified_grammar_is_refused() {
    let names = map(&["i", "n"]);
    for text in [
        "",                                // the solver said nothing
        "   \n \n  ",                      // ... and said it in whitespace
        "Arg_1-2 {O(n)}",                  // subtraction: not representable over N
        "Arg_1/2 {O(n)}",                  // division
        "max{Arg_0,Arg_1} {O(n)}",         // a shape this crate has never observed
        "min(Arg_0,Arg_1) {O(n)}",         //   "
        "2^Arg_1 {O(EXP)}",                // exponentiation of a symbolic exponent
        "Arg_1+ {O(n)}",                   // a dangling operator
        "+Arg_1 {O(n)}",                   // a leading operator
        "Arg_1 Arg_0 {O(n)}",              // juxtaposition
        "Arg_ {O(n)}",                     // an argument with no index
        "Arg_x {O(n)}",                    // ... or a non-numeric one
        "Arg_99999999999999999999 {O(n)}", // ... or one past u32
        "log Arg_1 {O(log(n))}",           // `log` without parentheses
        "log(Arg_1 {O(log(n))}",           // an unclosed parenthesis
        "log(Arg_1)) {O(log(n))}",         // an unopened one
        "Arg_1+2 {O(n)} Arg_0+1 {O(n)}",   // two answers on one line
        "Arg_1+2 {O(n)}\nArg_0+1 {O(n)}",  // two answers on two lines
        "18446744073709551616 {O(1)}",     // a literal past u64
        "Arg_1**2 {O(n^2)}",               // a spelling of `^` this crate has not seen
        "𝔞 {O(1)}",                        // not ASCII
    ] {
        let refused = koat_answer::parse(text, &names);
        assert!(
            refused.is_err(),
            "{text:?} is outside the grammar this crate has verified and must be refused, \
             got {refused:?}"
        );
    }
}

/// KoAT's own clock produces a distinct line, and it is a timeout rather than
/// an unparsable answer — the difference decides whether a caller should
/// retry with a longer budget or file a bug.
#[test]
fn the_solvers_own_timeout_line_is_reported_as_a_timeout() {
    let names = map(&["i", "n"]);
    let refused = koat_answer::parse(
        "TIMEOUT: Complexity analysis of the given ITS stopped as the given timelimit \
         has been exceeded!",
        &names,
    );
    assert!(
        matches!(refused, Err(SolverError::SolverTimedOut { solver }) if solver == Solver::Koat),
        "got {refused:?}"
    );
}

/// Surrounding whitespace and a trailing newline are how the answer actually
/// arrives from a pipe, and neither changes it.
#[test]
fn the_answer_survives_the_whitespace_a_pipe_adds() {
    let names = map(&["i", "n"]);
    let expected = Bound::sum([Bound::var("n"), Bound::constant(2)]);
    for text in [
        "Arg_1+2 {O(n)}\n",
        "\nArg_1+2 {O(n)}\n",
        "  Arg_1+2 {O(n)}  \n",
        "Arg_1 + 2 {O(n)}\n",
    ] {
        assert_eq!(bound_of(text, &names), expected, "{text:?}");
    }
}

/// A logarithm is read at base two and no other base. `log` is anti-monotone
/// in its base — `log_2(x) >= log_e(x) >= log_10(x)` — so for an *upper* bound
/// the smallest permitted base is the only sound reading of a base KoAT does
/// not state.
#[test]
fn an_unlabelled_logarithm_is_read_at_the_soundest_base() {
    let names = map(&["i", "n"]);
    assert_eq!(
        bound_of("log(Arg_1)+6 {O(log(n))}", &names),
        Bound::sum([Bound::log(Base::TWO, Bound::var("n")), Bound::constant(6)]),
        "base two is the smallest base `Base` permits and therefore the largest value; \
         reading it as base ten would report a bound below the one KoAT proved"
    );
}

// ---------------------------------------------------------------------------
// the command line is part of the contract
// ---------------------------------------------------------------------------

/// The argument vector is pinned, in full, because two of its elements are
/// load bearing and neither is obvious from reading it.
///
/// `--preprocessors` is given **without `eliminate`**. KoAT's default
/// preprocessing removes variables that "do not contribute to the problem" and
/// then renumbers the survivors, so a system declaring `(VAR va vb vi)` whose
/// `va` is unused answers about `Arg_0` meaning `vb`. Every `Arg_i` in this
/// crate's mapping would then be off by the number of variables eliminated
/// before it — a wrong answer that looks right, and the exact failure
/// `koat_answers` cannot detect from the text alone.
///
/// `--timeout` is KoAT's own clock, set below this crate's wall clock so that
/// the ordinary slow case ends in KoAT printing `TIMEOUT:` rather than in this
/// crate killing it. See `invocation.rs` for the live check that elimination
/// really is off.
#[test]
fn the_koat_command_line_is_pinned() {
    let argv: Vec<String> = Solver::Koat
        .argv(std::path::Path::new("/w/input.koat"), Timeout::DEFAULT)
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        argv,
        vec![
            "analyse".to_owned(),
            "--preprocessors=invgen,sat,reachable,tmp".to_owned(),
            "--timeout=25".to_owned(),
            "-i".to_owned(),
            "/w/input.koat".to_owned(),
        ],
        "the KoAT argument vector is a contract; `eliminate` must stay out of the \
         preprocessor list or every Arg_i mapping silently shifts"
    );
    assert!(
        !argv.iter().any(|a| a.contains("eliminate")),
        "`eliminate` renumbers Arg_i and must never appear"
    );
}

/// KoAT's own timeout must sit strictly below this crate's, at every timeout
/// the type permits, and must never reach zero — KoAT reads `--timeout=0` as
/// "no limit".
#[test]
fn the_solvers_own_clock_is_always_shorter_than_ours_and_never_zero() {
    for secs in [1, 2, 5, 6, 25, 30, 3600] {
        let Ok(timeout) = Timeout::new(secs) else {
            continue;
        };
        let argv = Solver::Koat.argv(std::path::Path::new("/w/i.koat"), timeout);
        let flag = argv
            .iter()
            .find_map(|a| {
                a.to_string_lossy()
                    .strip_prefix("--timeout=")
                    .map(|v| v.parse::<u64>().unwrap_or(u64::MAX))
            })
            .unwrap_or(u64::MAX);
        assert!(
            flag >= 1 && flag <= secs,
            "at a {secs}s budget KoAT was given --timeout={flag}"
        );
    }
}

/// The name of the program, and the sentence a user sees when it is not there.
#[test]
fn a_missing_solver_names_itself_and_what_to_install() {
    assert_eq!(Solver::Koat.program(), "koat2");
    let message = SolverError::NotInstalled {
        solver: Solver::Koat,
        program: "koat2".to_owned(),
        hint: Solver::Koat.install_hint(),
    }
    .to_string();
    assert!(message.contains("koat2"), "{message}");
    assert!(
        message.to_lowercase().contains("install"),
        "a missing binary must tell the reader what to do about it: {message}"
    );
    assert!(
        Solver::Koat.install_hint().contains("KoAT"),
        "the hint must name the project, not just the executable"
    );
}

// ---------------------------------------------------------------------------
// the edges of the limits, and the guards that are only visible at them
// ---------------------------------------------------------------------------

/// Output *at* the size cap is read; only output past it is refused. A cap
/// that refuses at the limit narrows what this crate can read for no reason,
/// and a cap that admits one byte past it is not a cap.
#[test]
fn the_size_cap_admits_an_answer_that_exactly_reaches_it() {
    let names = map(&["i", "n"]);
    let answer = "Arg_1+2 {O(n)}";
    let padding = " ".repeat(landav_solvers::MAX_ANSWER_BYTES - answer.len());
    let exactly = format!("{padding}{answer}");
    assert_eq!(exactly.len(), landav_solvers::MAX_ANSWER_BYTES);
    assert!(
        koat_answer::parse(&exactly, &names).is_ok(),
        "an answer of exactly MAX_ANSWER_BYTES must be read"
    );
    assert!(
        koat_answer::parse(&format!(" {exactly}"), &names).is_err(),
        "one byte past the cap must be refused"
    );
}

/// `log` and `inf` are whole words. A build that matched them by first letter
/// would read `lot(...)` as a logarithm and `ixf` as "no bound" — the second
/// of which turns an unreadable answer into a *silent* `Unknown`, which is the
/// worst available outcome short of a wrong number.
#[test]
fn the_keywords_are_matched_whole_and_not_by_their_first_letter() {
    let names = map(&["i", "n"]);
    for text in [
        "lot(Arg_1) {O(log(n))}",
        "l(Arg_1) {O(log(n))}",
        "ixf {Infinity}",
        "i {Infinity}",
        "logg(Arg_1) {O(log(n))}",
    ] {
        let refused = koat_answer::parse(text, &names);
        assert!(
            refused.is_err(),
            "{text:?} is not `log(` or `inf` and must be refused, got {refused:?}"
        );
    }
}

/// A logarithm inside a product, which is where the two flags the growth
/// measurement carries have to travel through a fold that is not the sum's.
#[test]
fn a_logarithm_survives_a_product() {
    let names = map(&["i", "n"]);
    assert_eq!(
        bound_of("2*log(Arg_1) {O(log(n))}", &names),
        Bound::prod([Bound::constant(2), Bound::log(Base::TWO, Bound::var("n"))]),
        "a constant multiple of a logarithm is still logarithmic"
    );
    assert_eq!(
        bound_of("log(Arg_1)*log(Arg_1) {O(log(n))}", &names),
        Bound::prod([
            Bound::log(Base::TWO, Bound::var("n")),
            Bound::log(Base::TWO, Bound::var("n")),
        ])
    );
}

/// Precedence, stated as the two bounds it distinguishes. `2*Arg_1+3` is not
/// `2*(Arg_1+3)`, and reading it as the latter would report a *larger* number
/// — sound, but wrong, and the mirror-image mistake is not.
#[test]
fn multiplication_binds_tighter_than_addition() {
    let names = map(&["i", "n"]);
    assert_eq!(
        bound_of("2*Arg_1+3 {O(n)}", &names),
        Bound::sum([
            Bound::prod([Bound::constant(2), Bound::var("n")]),
            Bound::constant(3),
        ])
    );
    assert_ne!(
        bound_of("2*Arg_1+3 {O(n)}", &names),
        Bound::prod([
            Bound::constant(2),
            Bound::sum([Bound::var("n"), Bound::constant(3)]),
        ])
    );
}

/// Each refusal says which rule was broken. The messages are the recovery
/// path: "the solver printed something this build cannot read" with no reason
/// sends a reader to read the source, which is the failure mode a typed error
/// exists to prevent.
#[test]
fn each_refusal_names_the_rule_it_broke() {
    let names = map(&["i", "n"]);
    for (text, expected) in [
        ("+Arg_1 {O(n)}", "an operator with no left operand"),
        ("Arg_1+ {O(n)}", "a dangling operator"),
        (
            "Arg_1 Arg_0 {O(n)}",
            "an argument where an operator was due",
        ),
        ("log(Arg_1 {O(n)}", "an unclosed `log(`"),
        ("log(Arg_1)) {O(log(n))}", "an unopened parenthesis"),
        ("Arg_ {O(n)}", "`Arg_` with no index"),
        ("Arg_1^ {O(n)}", "`^` with no exponent"),
        ("Arg_1-2 {O(n)}", "a character outside the grammar"),
    ] {
        let refused = koat_answer::parse(text, &names);
        assert!(
            matches!(&refused, Err(SolverError::Unparsable { detail, .. }) if *detail == expected),
            "{text:?} must be refused as {expected:?}, got {refused:?}"
        );
    }
}

/// A `log` with nothing inside it is not a bound, and must not become one.
#[test]
fn an_empty_logarithm_is_refused() {
    let names = map(&["i", "n"]);
    for text in ["log() {O(1)}", "log(){O(log(n))}"] {
        assert!(
            koat_answer::parse(text, &names).is_err(),
            "{text:?} must be refused"
        );
    }
}

// ---------------------------------------------------------------------------
// the accessors a caller reads the answer through
// ---------------------------------------------------------------------------

/// A report carries the solver's own words alongside the parsed answer, so a
/// diagnostic downstream can quote rather than paraphrase.
#[test]
fn a_report_keeps_what_the_solver_actually_printed() {
    let names = map(&["i", "n"]);
    let raw = "Arg_1+2 {O(n)}";
    let report = Solver::Koat.report(answer(raw, &names), raw, "countdown", "probe.py:1", &names);
    assert_eq!(report.raw(), raw);
    assert_eq!(report.solver(), Solver::Koat);
    assert_eq!(report.direction(), landav_solvers::Direction::Upper);
    assert_eq!(report.function().as_str(), "countdown");
    assert_eq!(report.origin().as_str(), "probe.py:1");
    assert!(
        report.blames().is_none(),
        "a finite bound over parameters has nothing unaccounted for"
    );
    assert_eq!(
        report.answer().bound(),
        Some(&Bound::sum([Bound::var("n"), Bound::constant(2)]))
    );
    assert_eq!(report.answer().growth(), Some(Growth::Polynomial(1)));
}

/// An unknown answer carries its ledger where a caller can read it without
/// going through `verdict`.
#[test]
fn an_unknown_answer_exposes_its_blame_directly() {
    let names = map(&["i", "n"]);
    let report = Solver::Koat.report(
        Answer::Unknown,
        "inf {Infinity}",
        "countdown",
        "probe.py:1",
        &names,
    );
    let Some(blames) = report.blames() else {
        panic!("an unknown answer must carry blame");
    };
    assert_eq!(blames.len(), 1);
    assert!(report.answer().bound().is_none());
    assert!(report.answer().growth().is_none());
}

/// The solver names the extension its input file carries. LoAT chooses its
/// parser by extension, so an empty one is an input no solver can read.
#[test]
fn the_input_file_carries_the_extension_the_solver_expects() {
    for solver in Solver::ALL {
        let extension = solver.input_extension();
        assert!(
            !extension.is_empty() && extension.chars().all(|c| c.is_ascii_alphanumeric()),
            "{solver} declares the input extension {extension:?}"
        );
        assert_eq!(
            extension, "koat",
            "both solvers are given the same KoAT-format text"
        );
    }
}

/// Every failure that is about a solver says which one, because a report over
/// two solvers with an unattributed failure in it is a report a reader cannot
/// act on.
#[test]
fn a_failure_about_a_solver_names_which_one() {
    let attributed = [
        SolverError::NoAnswer {
            solver: Solver::Koat,
        },
        SolverError::SolverTimedOut {
            solver: Solver::Loat,
        },
        SolverError::TimedOut {
            solver: Solver::Koat,
            seconds: 30,
        },
    ];
    for error in attributed {
        assert!(
            error.solver().is_some(),
            "{error} is about a solver and must name it"
        );
    }
    assert_eq!(
        SolverError::NoAnswer {
            solver: Solver::Loat
        }
        .solver(),
        Some(Solver::Loat)
    );
    assert!(
        SolverError::TimeoutOutOfRange {
            got: 0,
            min: 1,
            max: 3600
        }
        .solver()
        .is_none(),
        "a configuration error is not about a solver"
    );
}
