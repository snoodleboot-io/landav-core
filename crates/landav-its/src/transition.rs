//! [`Transition`] - one rule of the emitted system.

use landav_bound::Origin;

use crate::{cost::Cost, guard::Guard, location_id::LocationId, update::Update};

/// A guarded, updating step from one control location to another.
///
/// Reads: *when* `guard` holds in the current state, the system **may** move
/// from `source` to `target`, applying `update` simultaneously.
///
/// # "May", not "must"
///
/// An integer transition system is nondeterministic. Where two transitions out
/// of one location have overlapping guards, both are available and a solver
/// must account for either being taken. That is the mechanism every
/// over-approximation in this crate is expressed through: widening a guard
/// adds executions, it never removes them, so an imprecise lowering costs
/// tightness and cannot cost soundness.
///
/// # The origin is not decoration
///
/// Every transition records the source position it came from. It is what lets
/// a per-transition bound coming back from a solver be attributed to a line of
/// the user's code, which is the difference between "this program is
/// quadratic" and "this loop is quadratic".
///
/// # The cost is what is being bounded
///
/// A bound on the system is a bound on the sum of the [`Cost`]s of the
/// transitions an execution takes. With every cost one that sum is a step
/// count, which is what this crate meant before costs were representable.
/// Carrying a cost per transition is what lets a loop body that does `n` units
/// of work per iteration come out quadratic rather than linear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    source: LocationId,
    target: LocationId,
    guard: Guard,
    update: Update,
    cost: Cost,
    origin: Origin,
}

impl Transition {
    /// A transition from `source` to `target`.
    ///
    /// The cost is explicit rather than defaulted. A transition whose cost was
    /// never considered is indistinguishable, once it reaches the solver, from
    /// one deliberately charged a single step - and the two are different
    /// claims about the program.
    #[must_use]
    pub const fn new(
        source: LocationId,
        target: LocationId,
        guard: Guard,
        update: Update,
        cost: Cost,
        origin: Origin,
    ) -> Self {
        Self {
            source,
            target,
            guard,
            update,
            cost,
            origin,
        }
    }

    /// Where the step starts.
    #[must_use]
    pub const fn source(&self) -> LocationId {
        self.source
    }

    /// Where the step ends.
    #[must_use]
    pub const fn target(&self) -> LocationId {
        self.target
    }

    /// When the step is available.
    #[must_use]
    pub const fn guard(&self) -> &Guard {
        &self.guard
    }

    /// What the step does.
    #[must_use]
    pub const fn update(&self) -> &Update {
        &self.update
    }

    /// What one traversal of the step costs.
    #[must_use]
    pub const fn cost(&self) -> &Cost {
        &self.cost
    }

    /// The source position this step came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }
}

impl core::fmt::Display for Transition {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // A unit cost is left implicit, matching the emitted form: a bare arrow
        // is one step, and spelling it out on every transition would bury the
        // ones that differ.
        if self.cost.is_step() {
            write!(
                f,
                "{} -> {} [{}] {{{}}}",
                self.source, self.target, self.guard, self.update
            )
        } else {
            write!(
                f,
                "{} -{{{}}}> {} [{}] {{{}}}",
                self.source, self.cost, self.target, self.guard, self.update
            )
        }
    }
}
