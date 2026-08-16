//! External solver bridge — KoAT (upper bounds) and LoAT (lower bounds).
//!
//! # Scope
//!
//! Component `C-13`. Features [`F-007`] (bridge, R0/M0.5) and [`F-040`]
//! (evaluation corpus and benchmark harness, R1/M1).
//!
//! # What this crate does
//!
//! It takes an [`landav_its::Its`], hands it to an external solver as a file,
//! and reads the solver's answer back as a [`landav_bound::Bound`]. That is
//! the step that turns landav from "lowers Python to an integer transition
//! system" into "tells you a bound".
//!
//! ```no_run
//! # fn demo(its: &landav_its::Its) -> Result<(), landav_solvers::SolverError> {
//! use landav_solvers::{Config, Solver, run};
//!
//! let report = run(Solver::Koat, its, &Config::default())?;
//! let verdict = report.verdict()?;
//! # let _ = verdict;
//! # Ok(())
//! # }
//! ```
//!
//! # ⚠️ The licence constraint is architectural, not advisory
//!
//! **LoAT is GPL-3.0**, forced by a statically linked GPL Yices 2.
//! `landav-core` is Apache-2.0 and `landav-ee` is a *commercial* BSL 1.1
//! product.
//!
//! **Both solvers are therefore invoked across a process boundary: `fork` and
//! `exec`, with the system passed as a file and the options as command-line
//! arguments.** Under the FSF's own test, programs that communicate through
//! pipes, sockets and command-line arguments are separate works, while
//! programs sharing an address space are one. **Linking to LoAT, calling it
//! through FFI, embedding it, or vendoring its source would propagate GPL-3.0
//! onto `landav-core` and fatally onto `landav-ee`.**
//!
//! This is not a performance trade-off that can be revisited. A process spawn
//! costs single-digit milliseconds; the solver runs this crate was verified
//! against take 180 to 630. The saving from linking is inside the noise, and
//! the price is relicensing a commercial product. KoAT2 is MIT and carries no
//! such restriction, but the mechanism is uniform anyway: a bridge with one
//! linked solver and one spawned solver is a bridge where the licence-critical
//! property is a special case, and special cases get optimised away by people
//! who did not read this paragraph.
//!
//! There is no entitlement or licence-*checking* logic anywhere in this crate.
//! The paragraph above is about the licences of third-party binaries, which is
//! a different thing entirely; see `docs/EDITIONS.md`.
//!
//! # The four things that will bite, and where each is handled
//!
//! **The solver is untrusted input in reverse.** [`koat_answer`] parses text
//! from a program this repository does not control. A parse it cannot do is a
//! *refusal* with blame, never a guess and never a quiet `omega`: a bound
//! parsed smaller than the one the solver proved is a bound the analysed
//! program can exceed, which is the one failure class with a zero target.
//!
//! **`Arg_0` is positional; the variables are named.** [`ArgMap`] holds the
//! correspondence, and its documentation records the three independent places
//! it is pinned — including the KoAT preprocessor flag without which the
//! numbering silently shifts.
//!
//! **The solver can fail, hang, or not be installed.** All three are ordinary.
//! [`run`] imposes a wall clock ([`Timeout`], thirty seconds by default and
//! argued there), a missing binary is [`SolverError::NotInstalled`] naming
//! what to install, and a child that dies on a signal is an observation the
//! parent survives.
//!
//! **Unknown is a real answer.** KoAT prints `inf {Infinity}` often — the
//! published figure is 548 of 838 curated integer-only benchmarks. That
//! becomes [`Answer::Unknown`] and publishes as
//! [`landav_bound::Verdict::Partial`] over `omega` **with blame naming the
//! function**, never as a missing result and never as a fabricated one.
//!
//! # Upper against lower
//!
//! KoAT bounds above, LoAT below. [`Analysis`] puts the two together: equal
//! classes are a tightness claim, a lower class below the upper is a reported
//! gap, and a lower class **above** the upper is a contradiction — one of the
//! two solvers is wrong, nothing in the output says which, and
//! [`Analysis::verdict`] publishes nothing at all rather than keeping the
//! upper bound there is now positive evidence against.
//!
//! # The state of the lower-bound path
//!
//! LoAT 0.9.10 (build 2024-08-15) selects its input format by file extension
//! and reads only `.smt2` and `.ari`. It has **no reader for the KoAT ITS
//! format** and rejects the emitted system with `Error: unknown format`. The
//! bridge invokes it anyway, uniformly, and reports that as a named
//! [`SolverError::Failed`] — because the day a LoAT with the KoAT reader is
//! installed, nothing here needs to change.
//!
//! Re-encoding the system as constrained Horn clauses is not a workaround, and
//! that was measured rather than assumed: LoAT warns `analyzing the complexity
//! of CHCs -- is this intended?` and then answers `WORST_CASE(INF,?)` for a
//! loop that runs exactly ten times and terminates. A lower bound of infinity
//! for a constant-time program is not an answer, so no CHC emitter ships here.
//! [`loat_answer`] parses LoAT's vocabulary regardless, because that half is
//! pure, testable, and is what a future `.ari` emitter will need.
//!
//! # The ceiling this sits under
//!
//! KoAT — state of the art, from a specialist group, after a decade — solves
//! 548 of 838 curated *integer-only* benchmarks. Roughly 65%, on the easiest
//! possible input class. That number is why contract *checking* (`F-014`)
//! ships alongside the first inference engine rather than after it: a product
//! resting solely on inference coverage would be waiting on a number that does
//! not move quickly.
//!
//! F-023 (ranking function synthesis, R2) begins reducing this dependence.
//!
//! # Edition
//!
//! OSS — Apache-2.0, ships in `landav-core`. The benchmark numbers must be
//! publicly reproducible or they are worth nothing.
//!
//! [`F-007`]: https://linear.app/snoodleboot/issue/LAN-7
//! [`F-040`]: https://linear.app/snoodleboot/issue/LAN-16

#![doc(html_root_url = "https://docs.rs/landav-solvers")]
#![forbid(unsafe_code)]

pub mod analysis;
pub mod answer;
pub mod arg_map;
pub mod config;
pub mod direction;
pub mod growth;
pub mod invoke;
pub mod koat_answer;
pub mod loat_answer;
pub mod report;
pub mod solver;
pub mod solver_error;
pub mod timeout;
mod workspace;

pub use crate::{
    analysis::{Agreement, Analysis},
    answer::Answer,
    arg_map::ArgMap,
    config::Config,
    direction::Direction,
    growth::Growth,
    invoke::run,
    report::Report,
    solver::Solver,
    solver_error::SolverError,
    timeout::{Timeout, poll_budget},
};

/// The shortest wall clock a solver invocation may be given.
///
/// One second. Below that the child is killed before `exec` has finished on a
/// loaded machine, which turns every run into a timeout.
pub const MIN_TIMEOUT_SECS: u64 = 1;

/// The longest wall clock a solver invocation may be given.
///
/// One hour. Not a useful budget for a single function — it is a sanity limit,
/// so that a configuration typo cannot produce an invocation that outlives the
/// job it is part of.
pub const MAX_TIMEOUT_SECS: u64 = 3600;

/// How long [`run`] sleeps between looks at the child.
///
/// Ten milliseconds: short enough that a solver answering in 180 ms is not
/// held up measurably, long enough that a thirty-second wait is three thousand
/// wakeups rather than three million.
pub const POLL_INTERVAL_MILLIS: u64 = 10;

/// How far below this crate's wall clock KoAT's own `--timeout` is set.
///
/// Five seconds. The point is that in the ordinary slow case KoAT stops
/// *itself* and prints `TIMEOUT:`, which is an orderly outcome attributable to
/// the analysis being hard; this crate's clock is then only reached by a
/// solver that has ignored its own, which is a different fault and deserves a
/// different message.
pub const KOAT_TIMEOUT_GRACE_SECS: u64 = 5;

/// The most output that is read back from a solver.
///
/// One mebibyte. Output past it is **refused**, never truncated: the prefix of
/// `Arg_0^2+3` is `Arg_0`, which is a smaller upper bound than the one the
/// solver stated, and a bound read too small is the failure class with a zero
/// target.
pub const MAX_ANSWER_BYTES: usize = 1 << 20;

/// The most tokens one answer may contain.
///
/// Bounds the parser's operand and operator stacks independently of the byte
/// cap, which a pathological answer of single-character tokens would otherwise
/// leave as the only limit.
pub const MAX_ANSWER_TOKENS: usize = 4096;

/// The most steps the growth measurement takes over a parsed bound.
///
/// The measurement walks the bound's DAG with an explicit worklist, and the
/// worklist's own exit condition is "it emptied" — which is a property of the
/// body being exactly right rather than an independent bound. Eight steps per
/// permitted token is far more than any real answer needs and turns a
/// mis-stepped walk into a refusal instead of a hang. See
/// `tests/frozen_solver_invariants.rs`.
pub const MAX_MEASURE_STEPS: usize = MAX_ANSWER_TOKENS * 8;

/// The deepest `log(` nesting one answer may contain.
///
/// Thirty-two. The parse is iterative, so this is a resource cap rather than a
/// termination guard; no observed KoAT answer nests at all.
pub const MAX_NESTING: usize = 32;

/// The largest exponent an `Arg_i^k` may carry.
///
/// Sixty-four. `Arg_i^k` is expanded into a product of `k` factors, so `k`
/// chosen by solver output is an allocation chosen by solver output — and a
/// `Vec` that cannot grow calls `handle_alloc_error`, which **aborts**, past
/// the reach of `unwrap_used`, `panic` and `#![forbid(unsafe_code)]` alike.
/// The guard runs before the first factor is built. See
/// `tests/frozen_solver_invariants.rs`.
pub const MAX_EXPONENT: u32 = 64;

/// The most variables a system may declare.
///
/// The cap is what keeps the out-of-range check on a solver-supplied `Arg_i`
/// meaningful: an unbounded declaration list would make every index in range,
/// and so make the check vacuous.
pub const MAX_ARGS: usize = 4096;
