//! `LAN-78`: the native engine and KoAT2 checked against each other.
//!
//! # What agreement means here
//!
//! **Not equality.** KoAT2 returns a sound *upper* bound and is entitled to be
//! loose; this engine returns an exact count where it has one. Demanding the
//! two match would fail on exactly the programs where the native engine is
//! doing its job - the nested loop below is one, where KoAT2 is loose by `2m`.
//!
//! The relation that must hold is `engine <= KoAT2`, pointwise. If the engine
//! ever exceeds the solver, one of them is wrong and it is worth stopping for:
//! either the engine is unsound, or the lowering and the engine have drifted
//! apart on what a step is.
//!
//! # Why this compares numbers rather than expressions
//!
//! Two bounds denoting the same function may be written differently, and the
//! normal form is allowed to change. Comparing syntax would fail for reasons
//! that are not defects. So both sides are evaluated over a grid of concrete
//! valuations and compared as numbers, which is the only comparison that means
//! anything.
//!
//! # Why this is not in the shipped crate's dependencies
//!
//! `landav-solvers` is a dev-dependency. A native engine that needed a solver
//! on `PATH` to run would not be a native engine.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{io::Write as _, num::NonZeroI64};

use landav_bound::{Bound, Nat, Origin, Symbol, Valuation, VarId};
use landav_engine::{TripCount, cost};
use landav_its::{Its, RangeSpec, SourceProgram, SourceProgramBuilder, VarName, lower};
use landav_solvers::{Answer, Config, Solver, run};

/// Announce a skip loudly enough to be seen, and fail instead when the
/// environment says solvers are mandatory.
///
/// A differential check that silently passes because the solver is missing is
/// worse than no check: it reports green for a comparison that never happened.
fn skip(what: &str, why: &str) -> bool {
    let message = format!(
        "SKIPPED: {what}: {why}\n         set LANDAV_REQUIRE_SOLVERS=1 to make this a failure\n"
    );
    assert!(
        std::env::var("LANDAV_REQUIRE_SOLVERS").as_deref() != Ok("1"),
        "LANDAV_REQUIRE_SOLVERS=1 and {what} could not run: {why}"
    );
    // Not `eprintln!`: libtest discards the print macros for a passing test,
    // which is exactly the case this line exists for.
    let _ = std::io::stderr().write_all(message.as_bytes());
    true
}

fn installed(program: &str) -> bool {
    std::process::Command::new(program)
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Binds every named variable to the same value, and anything else to zero.
struct Grid {
    names: Vec<Symbol>,
    value: u64,
}

impl Valuation for Grid {
    fn value_of(&self, var: &VarId) -> Nat {
        if self.names.iter().any(|name| name == var.symbol()) {
            Nat::Fin(self.value)
        } else {
            Nat::Fin(0)
        }
    }
}

fn here() -> Origin {
    Origin::new("differential.py:1")
}

/// `for i in range(0, n): x = 0`
fn single_loop() -> SourceProgram {
    let mut build = SourceProgramBuilder::new("single", here(), vec![VarName::new("n")]);
    let start = build.int(0, here());
    let stop = build.var(VarName::new("n"), here());
    let value = build.int(0, here());
    let assign = build.assign(VarName::new("x"), value, here());
    let one = NonZeroI64::new(1).unwrap();
    let loop_stmt = build.for_range(
        VarName::new("i"),
        RangeSpec::new(start, stop, one),
        vec![assign],
        here(),
    );
    build.build(vec![loop_stmt])
}

/// `for i in range(0, n): for j in range(0, m): x = 0`
fn nested_loops() -> SourceProgram {
    let mut build =
        SourceProgramBuilder::new("nested", here(), vec![VarName::new("n"), VarName::new("m")]);
    let one = NonZeroI64::new(1).unwrap();

    let inner_start = build.int(0, here());
    let inner_stop = build.var(VarName::new("m"), here());
    let value = build.int(0, here());
    let assign = build.assign(VarName::new("x"), value, here());
    let inner = build.for_range(
        VarName::new("j"),
        RangeSpec::new(inner_start, inner_stop, one),
        vec![assign],
        here(),
    );

    let outer_start = build.int(0, here());
    let outer_stop = build.var(VarName::new("n"), here());
    let outer = build.for_range(
        VarName::new("i"),
        RangeSpec::new(outer_start, outer_stop, one),
        vec![inner],
        here(),
    );
    build.build(vec![outer])
}

/// The solver's bound for a system, or a sentence saying why there is none.
fn solver_bound(its: &Its) -> Result<Bound, String> {
    let report = run(Solver::Koat, its, &Config::default())
        .map_err(|error| format!("the solver did not answer: {error}"))?;
    match report.answer() {
        Answer::Symbolic { bound, .. } => Ok(bound.clone()),
        other => Err(format!(
            "the solver found no symbolic bound: {other:?}, from {}",
            report.raw()
        )),
    }
}

/// Assert `engine <= solver` pointwise across a grid, and report the point of
/// divergence rather than merely that one exists.
fn assert_engine_within_solver(what: &str, program: &SourceProgram) {
    if !installed(Solver::Koat.program()) && skip(what, "koat2 is not on PATH") {
        return;
    }

    let TripCount::Exact(engine) = cost(program) else {
        panic!("{what}: the engine must have an exact count for this program");
    };
    let its = lower(program).expect("the fragment must lower");
    let solver = match solver_bound(&its) {
        Ok(bound) => bound,
        Err(why) => panic!("{what}: {why}"),
    };

    let names: Vec<Symbol> = program
        .params()
        .iter()
        .map(|param| param.symbol().clone())
        .collect();

    for value in [0_u64, 1, 2, 3, 7, 16, 64] {
        let grid = Grid {
            names: names.clone(),
            value,
        };
        let ours = engine.eval(&grid);
        let theirs = solver.eval(&grid);
        assert!(
            ours.magnitude_cmp(theirs) != std::cmp::Ordering::Greater,
            "{what}: the engine exceeded the solver at every parameter = {value}.\n  \
             engine = {engine} evaluated to {ours:?}\n  \
             solver = {solver} evaluated to {theirs:?}\n  \
             One of them is wrong: either the engine is unsound, or the lowering's \
             cost model and the engine's have drifted apart."
        );
    }
}

/// A single counted loop. The two agree **exactly** here - KoAT2 is tight on
/// it - so this is the check that the shared unit is actually shared. If the
/// lowering's cost model drifts from the engine's, this is what notices.
#[test]
fn a_single_counted_loop_agrees_with_the_solver() {
    if !installed(Solver::Koat.program())
        && skip(
            "a_single_counted_loop_agrees_with_the_solver",
            "koat2 is not on PATH",
        )
    {
        return;
    }

    let program = single_loop();
    let TripCount::Exact(engine) = cost(&program) else {
        panic!("a single counted loop must be exact");
    };
    let its = lower(&program).expect("the fragment must lower");
    let solver = solver_bound(&its).unwrap_or_else(|why| panic!("{why}"));

    for value in [0_u64, 1, 5, 40] {
        let grid = Grid {
            names: vec![Symbol::from("n")],
            value,
        };
        assert_eq!(
            engine.eval(&grid),
            solver.eval(&grid),
            "a single loop is where KoAT2 is tight, so the two must match exactly.\n  \
             engine = {engine}\n  solver = {solver}"
        );
    }
}

/// Nested loops. KoAT2 is loose here and that is expected - the engine is
/// exact and the solver over-approximates by the structure a flat transition
/// graph cannot recover. The relation is `<=`, not `==`.
#[test]
fn nested_loops_stay_within_the_solvers_bound() {
    assert_engine_within_solver(
        "nested_loops_stay_within_the_solvers_bound",
        &nested_loops(),
    );
}

/// The looseness is real and worth pinning: if KoAT2 ever became exact on
/// nested loops, the argument for a native engine would be weaker and we
/// should find out from a test rather than from a blog post.
#[test]
fn the_solver_is_strictly_looser_on_nested_loops() {
    if !installed(Solver::Koat.program())
        && skip(
            "the_solver_is_strictly_looser_on_nested_loops",
            "koat2 is not on PATH",
        )
    {
        return;
    }

    let program = nested_loops();
    let TripCount::Exact(engine) = cost(&program) else {
        panic!("nested counted loops must be exact");
    };
    let its = lower(&program).expect("the fragment must lower");
    let solver = solver_bound(&its).unwrap_or_else(|why| panic!("{why}"));

    let grid = Grid {
        names: vec![Symbol::from("n"), Symbol::from("m")],
        value: 8,
    };
    let ours = engine.eval(&grid);
    let theirs = solver.eval(&grid);
    assert_eq!(
        ours.magnitude_cmp(theirs),
        std::cmp::Ordering::Less,
        "the native engine should be strictly tighter than the solver on nested \
         loops; if this fails because they are now equal, that is good news and \
         this test should be retired deliberately.\n  engine = {engine}\n  solver = {solver}"
    );
}
