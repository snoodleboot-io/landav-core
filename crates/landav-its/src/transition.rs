//! [`Transition`] - one rule of the emitted system.

use landav_bound::Origin;

use crate::{guard::Guard, location_id::LocationId, update::Update};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    source: LocationId,
    target: LocationId,
    guard: Guard,
    update: Update,
    origin: Origin,
}

impl Transition {
    /// A transition from `source` to `target`.
    #[must_use]
    pub const fn new(
        source: LocationId,
        target: LocationId,
        guard: Guard,
        update: Update,
        origin: Origin,
    ) -> Self {
        Self {
            source,
            target,
            guard,
            update,
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

    /// The source position this step came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }
}

impl core::fmt::Display for Transition {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} -> {} [{}] {{{}}}",
            self.source, self.target, self.guard, self.update
        )
    }
}
