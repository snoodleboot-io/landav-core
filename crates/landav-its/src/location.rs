//! [`Location`] - a control location and what it stands for.

use landav_bound::Symbol;

use crate::location_id::LocationId;

/// A control location, with a human-readable label.
///
/// The label carries no meaning to a solver - KoAT sees only the rendered
/// `l7` - but it is what makes an emitted system readable when a bound comes
/// back wrong and someone has to work out which loop it came from. It is
/// derived from the construct that created the location: `entry`, `exit`,
/// `while.head`, `for.body`, and so on.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location {
    id: LocationId,
    label: Symbol,
}

impl Location {
    /// A location with `id` and `label`.
    #[must_use]
    pub fn new(id: LocationId, label: impl Into<Symbol>) -> Self {
        Self {
            id,
            label: label.into(),
        }
    }

    /// The identity.
    #[must_use]
    pub const fn id(&self) -> LocationId {
        self.id
    }

    /// What this location stands for.
    #[must_use]
    pub const fn label(&self) -> &Symbol {
        &self.label
    }

    /// The name this location is rendered with, `l0`, `l1`, ... .
    #[must_use]
    pub fn rendered_name(&self) -> String {
        self.id.to_string()
    }
}
