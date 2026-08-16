//! [`LoweringError`] - why a program did not become an integer transition
//! system.

use landav_bound::{Blames, Symbol};
use thiserror::Error;

use crate::refusals::Refusals;

/// Why lowering did not produce an [`crate::Its`].
///
/// # There is no partial success
///
/// Neither variant carries an ITS alongside the failure, and that is the
/// central soundness decision of this crate rather than an API simplification.
///
/// A system built from the parts of a program that *were* understood admits
/// fewer executions than the program has: the refused construct might be a
/// loop, and the transitions it would have contributed are simply absent. A
/// solver handed that system returns a bound that is correct **for the
/// system** and can be exceeded by the program. Under a zero-target soundness
/// rule the only safe response to "I could not lower part of this" is to
/// publish nothing and say why - which is what these variants do.
///
/// Over-approximation is the safe direction and this crate uses it wherever it
/// can (see the crate-level table). It is not available here: over-approximating
/// an *unknown effect on unknown variables* means havocking the entire state,
/// which admits every execution and derives no bound worth reporting. So the
/// refusal is real, and `LAN-68`'s coverage report is how it becomes
/// actionable.
///
/// # Both variants carry blame
///
/// Neither can be constructed without naming its subject and its reason:
/// [`LoweringError::Refused`] carries a non-empty [`Refusals`] with a position
/// on every record, and [`LoweringError::Malformed`] names the function and
/// what was wrong with it. A blameless lowering failure is unrepresentable,
/// which is non-negotiable 3 stated as a type.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum LoweringError {
    /// The program contains constructs outside the fragment.
    ///
    /// Every one of them is listed, not just the first: see [`Refusals`].
    #[error("{function}: {} construct(s) outside the numeric fragment:\n{refusals}", refusals.len())]
    Refused {
        /// The function that was being lowered.
        function: Symbol,
        /// Every refused construct, with positions.
        refusals: Refusals,
    },

    /// The program itself is inconsistent.
    ///
    /// A handle naming no node, a body referring to a statement from another
    /// program, or an arena that overflowed [`crate::MAX_ARENA_NODES`]. This
    /// is a **frontend bug**, not a property of the analysed source, and it is
    /// kept separate from [`LoweringError::Refused`] for exactly that reason:
    /// a coverage report that counted frontend bugs as unsupported language
    /// constructs would send someone to write a lowering rule for a construct
    /// that does not exist.
    #[error("{function}: malformed source program: {detail}")]
    Malformed {
        /// The function that was being lowered.
        function: Symbol,
        /// What was inconsistent about it.
        detail: Symbol,
    },
}

impl LoweringError {
    /// The refused constructs, if this failure is a refusal.
    #[must_use]
    pub const fn refusals(&self) -> Option<&Refusals> {
        match self {
            Self::Refused { refusals, .. } => Some(refusals),
            Self::Malformed { .. } => None,
        }
    }

    /// The function this failure is about.
    #[must_use]
    pub const fn function(&self) -> &Symbol {
        match self {
            Self::Refused { function, .. } | Self::Malformed { function, .. } => function,
        }
    }

    /// This failure as a [`landav_bound::Blames`] ledger.
    ///
    /// The `F-015` seam: a caller that wants to publish a partial bound rather
    /// than nothing at all gets the blame records from here without this crate
    /// depending on the bound algebra's report types. Always non-empty - a
    /// [`LoweringError::Malformed`] produces a single record naming the
    /// function.
    #[must_use]
    pub fn blames(&self) -> Blames {
        match self {
            Self::Refused { refusals, .. } => refusals.blames(),
            Self::Malformed { function, detail } => {
                use landav_bound::{Assumption, Blame, Origin};
                Blames::new(Blame {
                    unaccounted: function.clone(),
                    assumption: Assumption::ResourceNotModelled {
                        detail: detail.clone(),
                    },
                    origin: Origin::new(function.as_str()),
                })
            }
        }
    }
}
