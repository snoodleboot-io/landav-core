//! [`TripCount`] - how many times a counted loop runs, and how well we know it.

use landav_bound::Bound;

use crate::hole::Hole;

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
    /// Known apart from named regions the engine could not analyse.
    ///
    /// The bound mentions one variable per [`Hole`], standing for the cost of
    /// one execution of that region. Everything around them is derived as
    /// usual, so a `for` loop followed by a `while` reports `2n + #hole0`
    /// rather than nothing at all.
    ///
    /// # Why this is not simply `AtMost`
    ///
    /// An unfilled hole denotes `omega`, so a partial result *is* an upper
    /// bound in the trivial sense - and reporting it as one would throw away
    /// the useful half. `exact_elsewhere` records whether the analysed part was
    /// derived exactly, because "exact except for the `while` at line 42" is a
    /// far more actionable statement than "at most infinity".
    Partial {
        /// The cost, mentioning each hole's variable.
        bound: Bound,
        /// The regions that were not derived, in the order they were met.
        holes: Vec<Hole>,
        /// Whether everything outside the holes was exact.
        exact_elsewhere: bool,
    },

    /// Nothing is known, and there is not even a region to point at.
    ///
    /// Structural failure only - a statement the arena cannot produce. An
    /// unanalysable *construct* is a [`TripCount::Partial`] with a hole, which
    /// is strictly more informative and is what the engine normally emits.
    Unknown,
}

impl TripCount {
    /// The count as a bound, whatever its quality.
    #[must_use]
    pub const fn bound(&self) -> Option<&Bound> {
        match self {
            Self::Exact(bound) | Self::AtMost(bound) | Self::Partial { bound, .. } => Some(bound),
            Self::Unknown => None,
        }
    }

    /// The regions that were not derived.
    #[must_use]
    pub fn holes(&self) -> &[Hole] {
        match self {
            Self::Partial { holes, .. } => holes,
            _ => &[],
        }
    }

    /// Whether the bound stands on its own, with no unanalysed region in it.
    ///
    /// A complete result can be compared against a budget or another engine. A
    /// partial one cannot, until its holes are filled.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::Exact(_) | Self::AtMost(_))
    }

    /// Whether this supports a two-sided claim.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self, Self::Exact(_))
    }

    /// A whole region the engine could not derive.
    ///
    /// The bound *is* the hole: one execution of the region costs whatever it
    /// costs, and nothing around it is known either.
    #[must_use]
    pub fn opaque(hole: Hole) -> Self {
        let bound = hole.as_bound();
        Self::Partial {
            bound,
            holes: vec![hole],
            exact_elsewhere: true,
        }
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
            Self::Partial {
                bound,
                holes,
                exact_elsewhere: _,
            } => Self::Partial {
                bound,
                holes,
                exact_elsewhere: false,
            },
            other => other,
        }
    }

    /// Combine two counts taken in sequence, adding them.
    ///
    /// Exact only when both are: a sum is known exactly precisely when every
    /// term is.
    #[must_use]
    pub fn then(self, next: Self) -> Self {
        Self::combine(self, next, |left, right| Bound::sum([left, right]))
    }

    /// Combine two results with `join`, carrying holes and exactness through.
    ///
    /// # The rule that matters
    ///
    /// A hole on either side makes the result partial rather than unknown.
    /// That single change is what stops one `while` erasing every exact bound
    /// in a function: the arithmetic keeps working, and what could not be
    /// derived is named instead of swallowing what could.
    ///
    /// Exactness is conjunctive. The result is exact outside its holes only if
    /// both sides were, because one approximated term makes the total
    /// approximate no matter how precise its neighbour was.
    fn combine(left: Self, right: Self, join: impl Fn(Bound, Bound) -> Bound) -> Self {
        // Genuine structural failure still swallows everything. There is no
        // region to point at, so there is nothing to carry.
        let (Some(left_bound), Some(right_bound)) = (left.bound(), right.bound()) else {
            return Self::Unknown;
        };
        let bound = join(left_bound.clone(), right_bound.clone());

        let mut holes = left.holes().to_vec();
        holes.extend_from_slice(right.holes());

        if holes.is_empty() {
            return if left.is_exact() && right.is_exact() {
                Self::Exact(bound)
            } else {
                Self::AtMost(bound)
            };
        }
        Self::Partial {
            bound,
            holes,
            exact_elsewhere: left.exact_outside_holes() && right.exact_outside_holes(),
        }
    }

    /// Whether everything this result *did* derive was derived exactly.
    ///
    /// For a complete result this is just exactness. For a partial one it is
    /// the interesting half: it distinguishes "exact except for that `while`"
    /// from "approximate, and also there is a `while`".
    #[must_use]
    pub const fn exact_outside_holes(&self) -> bool {
        match self {
            Self::Exact(_) => true,
            Self::Partial {
                exact_elsewhere, ..
            } => *exact_elsewhere,
            Self::AtMost(_) | Self::Unknown => false,
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
        // A hole inside the body denotes the cost of **one execution** of that
        // region, so multiplying it by the trip count is right: the region runs
        // once per iteration. Filling the hole later with a per-execution bound
        // therefore yields the bound the engine would have produced had it
        // known the region all along.
        Self::combine(self, per_iteration, |count, each| {
            Bound::prod([count, each])
        })
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
        // Equal alternatives keep whatever they were: the maximum is attained
        // whichever way the test goes, so reachability never arises.
        if self == other {
            return self;
        }
        Self::combine(self, other, |left, right| Bound::max_of([left, right])).relax()
    }
}
