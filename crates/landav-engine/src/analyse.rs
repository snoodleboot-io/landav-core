//! Walking a structured program and accumulating what it costs.

use landav_bound::Bound;
use landav_its::{RangeSpec, SourceExpr, SourceProgram, SourceStmt, StmtId};

use crate::{expr_bound, trip_count::TripCount};

/// What the program costs, in **source steps**: one per statement executed,
/// plus one per loop iteration for the loop's own test and increment.
///
/// # This is not the same unit the solver reports
///
/// Measured rather than assumed. For `for i in range(n): for j in range(m): x = 0`
/// this engine gives `n * (1 + 2m)`, and KoAT2 over the lowered system gives
/// `3mn + 3m + 4n + 2`. The gap is not precision - it is that the lowering
/// emits several transitions per source construct (a guard test, a body, a
/// counter increment) and the solver counts all of them.
///
/// Neither number is wrong; they answer different questions. This one is the
/// more stable of the two, because the ITS transition count is an artifact of
/// lowering choices that could change without the program changing.
///
/// **The consequence is that the two cannot currently be compared**, so the
/// "report tightness when upper and lower meet" plan does not work as written.
/// Reconciling them is what `Cost` on a transition is for: charging the
/// bookkeeping transitions nothing and the source-bearing ones one step would
/// put both engines in this unit. That is tracked separately and is not done
/// here.
#[must_use]
pub fn cost(program: &SourceProgram) -> TripCount {
    body_cost(program, program.body())
}

/// The cost of a statement list, in sequence.
fn body_cost(program: &SourceProgram, body: &[StmtId]) -> TripCount {
    body.iter()
        .fold(TripCount::Exact(Bound::zero()), |acc, id| {
            acc.then(stmt_cost(program, *id))
        })
}

/// The cost of one statement.
fn stmt_cost(program: &SourceProgram, id: StmtId) -> TripCount {
    let Some(stmt) = program.stmt(id) else {
        return TripCount::Unknown;
    };
    match stmt {
        // One step, exactly. These are the leaves the whole sum is built from,
        // and the only reason the total can ever be exact.
        SourceStmt::Assign { .. } | SourceStmt::Return => TripCount::Exact(Bound::one()),

        SourceStmt::If {
            then_body,
            else_body,
            ..
        } => {
            let taken = body_cost(program, then_body).branching(body_cost(program, else_body));
            // One step for the test itself, whichever way it goes.
            taken.then(TripCount::Exact(Bound::one()))
        }

        // A `while` loop needs a ranking argument, which this engine does not
        // yet have. Deliberately `Unknown` rather than a guess: the caller has
        // an external solver for exactly this case, and a fabricated bound here
        // would displace a real one.
        SourceStmt::While { .. } => TripCount::Unknown,

        SourceStmt::ForRange { range, body, .. } => {
            let count = count_of(program, *range);
            count.iterating(body_cost(program, body))
        }

        // Lowering refuses these outright, so a program containing one never
        // reaches a solver either. Reported rather than skipped.
        SourceStmt::Unsupported { .. } => TripCount::Unknown,
    }
}

/// How many times a counted loop runs.
///
/// # Where exactness comes from, and where it stops
///
/// The iteration space is fixed before the loop starts, so this is arithmetic
/// rather than inference. What limits it is not knowledge but *expressibility*:
/// the bound algebra is weakly monotone by construction and therefore has no
/// subtraction and no division, so `max(0, stop - start)` and `ceil(n / k)`
/// cannot be written down even when both are perfectly well understood.
///
/// The cases that survive:
///
/// * `range(0, e)` with unit step - the count is `e` itself, and `e` is already
///   a bound whenever it is monotone.
/// * any range whose endpoints are both literals - the count is arithmetic on
///   two numbers, done here in `i128` so it cannot overflow.
///
/// Everything else is an over-approximation, and says so.
fn count_of(program: &SourceProgram, range: RangeSpec) -> TripCount {
    let start = program.expr(range.start);
    let stop = program.expr(range.stop);
    let step = range.step.get();

    // Both endpoints literal: compute the count outright. `i128` because
    // `stop - start` can exceed `i64` when the two straddle the range, and a
    // wrapped subtraction here would be a silently wrong trip count.
    if let (Some(SourceExpr::Int { value: from }), Some(SourceExpr::Int { value: to })) =
        (start, stop)
    {
        let (from, to, step) = (i128::from(*from), i128::from(*to), i128::from(step));
        let span = if step > 0 { to - from } else { from - to };
        let stride = step.abs();
        let count = if span <= 0 {
            0
        } else {
            // Ceiling division, done without floats.
            (span + stride - 1) / stride
        };
        return u64::try_from(count)
            .map_or(TripCount::Unknown, |n| TripCount::Exact(Bound::constant(n)));
    }

    // Symbolic stop, unit ascending step, and a start pinned to zero: the count
    // *is* the stop expression. This is `for i in range(n)`, which is the
    // overwhelming majority of counted loops in real Python.
    if step == 1 && matches!(start, Some(SourceExpr::Int { value: 0 })) {
        return expr_bound::read(program, range.stop).map_or(TripCount::Unknown, TripCount::Exact);
    }

    // A symbolic start would need `stop - start`, and a stride above one would
    // need division. Neither is expressible, and neither has a sound
    // over-approximation that does not first require knowing the start is
    // non-negative - which nothing here establishes.
    TripCount::Unknown
}
