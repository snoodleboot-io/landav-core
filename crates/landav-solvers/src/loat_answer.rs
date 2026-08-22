//! Reading LoAT's answer.
//!
//! # Retired: nothing in landav invokes LoAT, and nothing should
//!
//! **LoAT is not a dependency of this project and must not be installed as
//! one.** It is GPL-3.0 - forced, not chosen, by a statically linked Yices 2
//! and CLN - and landav ships an Apache-2.0 core alongside a commercial BSL
//! offering. The decision and its reasoning are recorded in the team document
//! *Lower bounds: build in-house - licence and patent findings*.
//!
//! Two further facts make this settled rather than merely cautious:
//!
//! * **The invocation never worked.** LoAT selects its reader by file
//!   extension and accepts only SMT-LIB and ARI. This crate emits KoAT format,
//!   which LoAT rejects outright. There was no working lower-bound path to
//!   give up.
//! * **Lower bounds come from elsewhere now.** `landav-engine` derives exact
//!   `Theta` for the counted-loop fragment from source structure - both sides
//!   at once, with no solver - which is a stronger result than pairing an
//!   upper-bound solver with a lower-bound one would have produced.
//!
//! # Why this module is kept rather than deleted
//!
//! Parsing an answer is pure, and this half is correct and covered by 45
//! tests. If an `.ari` emitter is ever written - the one thing that would make
//! LoAT invocable at all - this is what it would need, and rewriting it from
//! scratch would be work for no reason.
//!
//! So it stays as a **parser without a caller**, deliberately. It is not a
//! recommendation, and the presence of a reader for a tool's output is not an
//! argument for installing that tool.
//!
//! # A closed vocabulary, not a grammar
//!
//! LoAT prints the termination-competition answer format, which is a small
//! fixed set of strings rather than an expression language:
//!
//! ```text
//! WORST_CASE(Omega(1),?)
//! WORST_CASE(Omega(n^2),?)
//! WORST_CASE(Omega(EXP),?)
//! WORST_CASE(INF,?)
//! WORST_CASE(?,?)
//! MAYBE
//! ```
//!
//! The two fields are the lower and the upper bound. LoAT is a lower-bound
//! tool and has never been observed to fill the second in, so an answer that
//! does is a build this crate has not verified and is refused.
//!
//! There is no symbolic bound anywhere in that list. A LoAT answer is a
//! [`crate::Growth`] and nothing else, which is why [`crate::Answer::Class`]
//! exists: synthesising an expression from a class would put a term in front
//! of a user that no solver proved.
//!
//! # The answer is one line among several that are not answers
//!
//! LoAT prints a build banner - its own commit, the Yices version, the build
//! date - on every run, and a `warning:` line before the answer when the input
//! is a shape it considers unusual. Neither is an answer. So this parser looks
//! for lines belonging to the vocabulary above and requires **exactly one**:
//! none is [`SolverError::NoAnswer`], and two is a refusal rather than a
//! choice between them.
//!
//! # What this build of LoAT can and cannot read
//!
//! LoAT 0.9.10 (build 2024-08-15) selects its input format by file extension
//! and recognises only `.smt2` and `.ari`. It has **no reader for the KoAT ITS
//! format** that `landav-its` emits, and rejects the file with `Error: unknown
//! format` and exit status 1. That surfaces through [`crate::run`] as a named
//! [`SolverError::Failed`], which is the honest outcome: the bridge works, the
//! solver cannot read the input.
//!
//! Encoding the system as constrained Horn clauses is *not* a substitute, and
//! this was measured rather than assumed. LoAT warns `analyzing the complexity
//! of CHCs -- is this intended?` and then answers `WORST_CASE(INF,?)` for a
//! loop that runs exactly ten times and terminates. A lower bound of infinity
//! for a constant-time program is not a usable answer, so no CHC emitter is
//! provided here; a future lower-bound path needs either a LoAT build with the
//! KoAT reader restored or a correct `.ari` emitter, and either way this
//! module's vocabulary is what it will parse.

use crate::{
    MAX_ANSWER_BYTES, answer::Answer, growth::Growth, solver::Solver, solver_error::SolverError,
};

/// The prefix every complexity answer carries.
const ANSWER: &str = "WORST_CASE(";

/// The answer for "nothing was proved", which LoAT prints without the
/// `WORST_CASE` wrapper.
const MAYBE: &str = "MAYBE";

/// Parse one LoAT answer out of its entire standard output.
///
/// # Errors
///
/// [`SolverError::OutputTooLarge`] past [`MAX_ANSWER_BYTES`],
/// [`SolverError::NoAnswer`] when no line belongs to the vocabulary, and
/// [`SolverError::Unparsable`] when two do or when one nearly does.
pub fn parse(stdout: &str) -> Result<Answer, SolverError> {
    if stdout.len() > MAX_ANSWER_BYTES {
        return Err(SolverError::OutputTooLarge {
            got: stdout.len(),
            limit: MAX_ANSWER_BYTES,
            solver: Solver::Loat,
        });
    }

    let candidates: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with(ANSWER) || *line == MAYBE)
        .collect();

    match candidates.as_slice() {
        [] => Err(SolverError::NoAnswer {
            solver: Solver::Loat,
        }),
        [only] => answer(only),
        _ => Err(SolverError::Unparsable {
            solver: Solver::Loat,
            at: candidates.join(" | ").chars().take(200).collect(),
            detail: "more than one answer line",
        }),
    }
}

/// One vocabulary line as an answer.
fn answer(line: &str) -> Result<Answer, SolverError> {
    if line == MAYBE {
        return Ok(Answer::Unknown);
    }
    let refuse = |detail: &'static str| SolverError::Unparsable {
        solver: Solver::Loat,
        at: line.chars().take(200).collect(),
        detail,
    };

    let fields = line
        .strip_prefix(ANSWER)
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| refuse("a `WORST_CASE(` that is not closed"))?;
    let Some((lower, upper)) = fields.split_once(',') else {
        return Err(refuse("a `WORST_CASE(...)` with one field"));
    };
    // LoAT is a lower-bound tool. An upper field it has never been observed to
    // fill in means a build this crate has not verified, and reading the lower
    // field anyway would be reading half of an answer whose other half is a
    // surprise.
    if upper.trim() != "?" {
        return Err(refuse("an upper bound in a lower-bound answer"));
    }

    match lower.trim() {
        "?" => Ok(Answer::Unknown),
        "INF" => Ok(Answer::Class(Growth::Unbounded)),
        "Omega(1)" => Ok(Answer::Class(Growth::Constant)),
        "Omega(EXP)" => Ok(Answer::Class(Growth::Exponential)),
        other => other
            .strip_prefix("Omega(n^")
            .and_then(|rest| rest.strip_suffix(')'))
            .and_then(|degree| degree.parse::<u32>().ok())
            .filter(|degree| *degree >= 1)
            .map(|degree| Answer::Class(Growth::Polynomial(degree)))
            .ok_or_else(|| refuse("a lower-bound class outside the verified vocabulary")),
    }
}
