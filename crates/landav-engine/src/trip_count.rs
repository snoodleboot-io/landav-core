//! [`TripCount`] - how many times a counted loop runs, and how well we know it.

use landav_bound::Bound;

/// What is known about the number of iterations a loop performs.
///
/// The distinction between the two informative variants is the whole point of
/// this crate. An [`Exact`](TripCount::Exact) count supports a claim of `Theta`;
/// an [`AtMost`](TripCount::AtMost) supports only `O`. Collapsing them would
/// let a bound that happens to be tight be reported identically to one that was
/// rounded up, and the user cannot tell those apart from the outside.
///
/// # Why exactness is achievable at all
///
/// A counted loop's iteration space is fixed before the loop begins - see
/// `RangeSpec`, whose `start` and `stop` are evaluated exactly once - and the
/// fragment refuses `break`, `continue` and exceptions, so nothing can leave
/// early. Those two facts together mean the trip count is not something to be
/// inferred; it is arithmetic on values the program already computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TripCount {
    /// The loop runs exactly this many times.
    Exact(Bound),
    /// The loop runs at most this many times, and possibly fewer.
    ///
    /// Reached when the true count is known but not *expressible*: the bound
    /// algebra is weakly monotone by construction and so has neither
    /// subtraction nor division, which puts `max(0, stop - start)` and
    /// `ceil(n / k)` out of reach. The over-approximation is sound and the
    /// looseness is recorded rather than hidden.
    AtMost(Bound),
    /// Nothing is known.
    ///
    /// A `while` loop with no ranking argument, or a range whose endpoints this
    /// engine cannot read. Not an error - the caller still has the external
    /// solver - but it is the end of any claim this crate can make.
    Unknown,
}

impl TripCount {
    /// The count as a bound, whatever its quality.
    #[must_use]
    pub const fn bound(&self) -> Option<&Bound> {
        match self {
            Self::Exact(bound) | Self::AtMost(bound) => Some(bound),
            Self::Unknown => None,
        }
    }

    /// Whether this supports a two-sided claim.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self, Self::Exact(_))
    }

    /// Weaken to an upper bound.
    ///
    /// Used where a caller learns something that invalidates the lower half of
    /// the claim but not the upper - an unanalysable branch beside an exact
    /// one, say. Weakening is always sound; the reverse never is, and there is
    /// deliberately no method for it.
    #[must_use]
    pub fn relax(self) -> Self {
        match self {
            Self::Exact(bound) => Self::AtMost(bound),
            other => other,
        }
    }

    /// Combine two counts taken in sequence, adding them.
    ///
    /// Exact only when both are: a sum is known exactly precisely when every
    /// term is.
    #[must_use]
    pub fn then(self, next: Self) -> Self {
        match (self, next) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Exact(left), Self::Exact(right)) => {
                Self::Exact(Bound::sum([left, right]))
            }
            (left, right) => {
                // At least one side is loose, so the sum is. `bound` cannot be
                // `None` here - the `Unknown` cases are already handled above -
                // but the match is written to be total rather than to rely on
                // that reasoning surviving an edit.
                match (left.bound(), right.bound()) {
                    (Some(l), Some(r)) => Self::AtMost(Bound::sum([l.clone(), r.clone()])),
                    _ => Self::Unknown,
                }
            }
        }
    }

    /// The cost of running `body` once per iteration of a loop counted by
    /// `self`, plus one step per iteration for the loop's own test and
    /// increment.
    ///
    /// # Why the body's cost multiplies rather than adds
    ///
    /// This is the step that makes nesting work, and it is the only place the
    /// analysis does anything a flat transition system could not. Two nested
    /// counted loops multiply their counts because the inner loop's iteration
    /// space does not depend on how many times the outer one has run - which
    /// is a fact about `RangeSpec` being evaluated once, not a fact this
    /// function establishes.
    ///
    /// When the inner count *does* depend on the outer counter - triangular
    /// nesting - the product over-approximates, and the result is relaxed
    /// accordingly by the caller.
    #[must_use]
    pub fn iterating(self, body: Self) -> Self {
        let per_iteration = body.then(Self::Exact(Bound::one()));
        match (self, per_iteration) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Exact(count), Self::Exact(each)) => {
                Self::Exact(Bound::prod([count, each]))
            }
            (count, each) => match (count.bound(), each.bound()) {
                (Some(c), Some(e)) => Self::AtMost(Bound::prod([c.clone(), e.clone()])),
                _ => Self::Unknown,
            },
        }
    }

    /// The worse of two alternatives.
    ///
    /// # Always an upper bound, never exact
    ///
    /// For a *worst-case* question the maximum over branches is the right
    /// answer, and it is attained whenever the expensive branch is reachable.
    /// This engine performs no reachability analysis, so it has no evidence
    /// that it is - and a `Theta` claim asserts a lower bound, which would be
    /// wrong for a branch no execution takes.
    ///
    /// Equal alternatives are the exception: the maximum is attained whichever
    /// way the test goes, so reachability does not arise.
    #[must_use]
    pub fn branching(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (left, right) if left == right => left,
            (left, right) => match (left.bound(), right.bound()) {
                (Some(l), Some(r)) => Self::AtMost(Bound::max_of([l.clone(), r.clone()])),
                _ => Self::Unknown,
            },
        }
    }
}
