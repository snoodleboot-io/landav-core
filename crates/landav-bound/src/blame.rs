//! [`Blame`] - one unaccounted term.

use crate::{assumption::Assumption, origin::Origin, symbol::Symbol};

/// Why a bound is partial: what was not accounted for, which assumption could
/// not be discharged, and where.
///
/// `Ord` is derived and is part of the contract: [`crate::Blames`] keeps
/// itself sorted, so the order in which blame records reach a report, a JSON
/// document or a CI log is a function of their content and not of whichever
/// hash container the engine happened to iterate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Blame {
    /// The name of the unaccounted term as it appears in the reported bound.
    /// Frontend-supplied; Core attaches no meaning.
    pub unaccounted: Symbol,
    /// The obligation that could not be discharged.
    pub assumption: Assumption,
    /// An opaque frontend-supplied location.
    pub origin: Origin,
}
