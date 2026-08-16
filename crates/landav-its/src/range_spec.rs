//! [`RangeSpec`] - the half-open integer range a counted loop walks.

use core::num::NonZeroI64;

use crate::expr_id::ExprId;

/// A counted loop's iteration space: `start`, `stop` and a literal `step`.
///
/// Denotes the integers `start, start + step, start + 2 * step, ...` while
/// they remain **strictly before** `stop` in the direction of travel - below
/// it when `step` is positive, above it when `step` is negative. That is the
/// half-open convention of Python's `range`, and it is expressed here in
/// direction-neutral terms because Core does not know which language it came
/// from.
///
/// # Why the step is a literal, and non-zero
///
/// The sign of the step decides which way the loop guard points: `counter <
/// stop` for a positive step, `counter > stop` for a negative one. There is no
/// single guard covering both, so a step whose sign is unknown at lowering
/// time is a guard that cannot be written. Rather than over-approximate it to
/// an unguarded loop - sound, but non-terminating in the emitted system and so
/// worth exactly nothing to a solver - a frontend must emit
/// [`crate::Construct::UnboundedIteration`] for a symbolic step.
///
/// A zero step is unrepresentable rather than refused: [`NonZeroI64`] makes it
/// a type error. `range(a, b, 0)` raises at runtime in Python and denotes no
/// iteration space at all, so there is nothing for the fragment to be careful
/// about.
///
/// # `start` and `stop` are evaluated once
///
/// Both are expressions in the state *before* the loop, exactly once, and the
/// lowering snapshots them into fresh variables to make that true in the
/// emitted system. A loop body that assigns to a variable appearing in `stop`
/// does not change the trip count - in the source, and therefore in the ITS.
/// Getting this wrong is a soundness bug in both directions, and it has its
/// own property test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeSpec {
    /// The first value the counter takes.
    pub start: ExprId,
    /// The exclusive endpoint.
    pub stop: ExprId,
    /// The literal, non-zero stride.
    pub step: NonZeroI64,
}

impl RangeSpec {
    /// A range from `start` to `stop` with stride `step`.
    #[must_use]
    pub const fn new(start: ExprId, stop: ExprId, step: NonZeroI64) -> Self {
        Self { start, stop, step }
    }

    /// Whether the counter increases.
    #[must_use]
    pub const fn ascending(self) -> bool {
        self.step.get() > 0
    }
}
