//! [`Its`] - the emitted integer transition system.

use landav_bound::{Origin, Symbol};

use crate::{
    its_var::ItsVar, koat, location::Location, location_id::LocationId, transition::Transition,
};

/// An integer transition system: variables, locations, and guarded rules.
///
/// # What this represents, and what it deliberately does not
///
/// The system over-approximates the program's control flow and integer state.
/// Every execution the source program can perform corresponds to a run of this
/// system; the converse does not hold, and is not meant to. A solver asked for
/// a runtime bound on this system returns a bound that also holds for the
/// program - that implication is the entire product, and it runs in one
/// direction only.
///
/// It carries no notion of values returned, no heap, no exceptions and no
/// notion of *which* transition is "the program". It is a cost skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Its {
    pub(crate) name: Symbol,
    pub(crate) origin: Origin,
    pub(crate) vars: Vec<ItsVar>,
    pub(crate) params: Vec<ItsVar>,
    pub(crate) start: LocationId,
    pub(crate) exit: LocationId,
    pub(crate) locations: Vec<Location>,
    pub(crate) transitions: Vec<Transition>,
}

impl Its {
    /// The name of the function this system was lowered from.
    #[must_use]
    pub const fn name(&self) -> &Symbol {
        &self.name
    }

    /// Where that function was.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Every variable of the system, in canonical order.
    ///
    /// Includes both the program's variables and the fresh ones introduced to
    /// desugar counted loops. This is the tuple every location takes and every
    /// rule writes, positionally.
    #[must_use]
    pub fn vars(&self) -> &[ItsVar] {
        &self.vars
    }

    /// The variables a derived bound may be expressed in, in declaration
    /// order.
    ///
    /// The function's parameters. A bound mentioning anything else - a local,
    /// or one of the lowering's fresh counters - is not a bound the caller can
    /// evaluate, so this is the set `F-006`'s consumers filter against.
    #[must_use]
    pub fn params(&self) -> &[ItsVar] {
        &self.params
    }

    /// The initial location.
    #[must_use]
    pub const fn start(&self) -> LocationId {
        self.start
    }

    /// The location every terminating run ends at.
    ///
    /// A single exit, which `return` jumps to and which the end of the body
    /// falls through to. Having exactly one is what makes "the runtime of this
    /// function" a well-posed question for a solver.
    #[must_use]
    pub const fn exit(&self) -> LocationId {
        self.exit
    }

    /// Every location, in allocation order.
    #[must_use]
    pub fn locations(&self) -> &[Location] {
        &self.locations
    }

    /// Every rule, in emission order.
    #[must_use]
    pub fn transitions(&self) -> &[Transition] {
        &self.transitions
    }

    /// The rules leaving `location`.
    pub fn transitions_from(&self, location: LocationId) -> impl Iterator<Item = &Transition> {
        self.transitions
            .iter()
            .filter(move |transition| transition.source() == location)
    }

    /// The location with this identity, if it exists.
    #[must_use]
    pub fn location(&self, id: LocationId) -> Option<&Location> {
        self.locations.iter().find(|location| location.id() == id)
    }

    /// The system in KoAT's integer transition system format.
    ///
    /// See [`crate::koat`] for the shape of the output and the identifier
    /// escaping it performs.
    #[must_use]
    pub fn to_koat(&self) -> String {
        koat::render(self)
    }
}
