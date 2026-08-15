//! [`TotalValuation`] - a map plus an explicit policy for absent variables.

use std::collections::BTreeMap;

use crate::{bound_error::BoundError, nat::Nat, valuation::Valuation, var_id::VarId};

/// A map lifted to a [`Valuation`] by an explicit policy for absent variables.
///
/// Backed by a `BTreeMap`, so iteration order is deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotalValuation {
    known: BTreeMap<VarId, Nat>,
    absent: Nat,
}

impl TotalValuation {
    /// Absent variables evaluate to `omega`. **The only sound policy for
    /// analysis**, because `omega` is the top of the lattice.
    ///
    /// It is sound but not self-blaming: the resulting `omega` reaches
    /// [`crate::Verdict::classify`], which refuses to publish it unless the
    /// caller also supplies a blame ledger.
    #[must_use]
    pub fn saturating(known: BTreeMap<VarId, Nat>) -> Self {
        Self {
            known,
            absent: Nat::OMEGA,
        }
    }

    /// Absent variables evaluate to `default`.
    ///
    /// **Only `Nat::OMEGA` is sound for analysis.** Any other default
    /// under-approximates an unknown size. This constructor exists for the
    /// law suite and the property tests, where a valuation is a *chosen
    /// point* on a grid rather than an over-approximation of an unknown.
    #[must_use]
    pub fn with_default(known: BTreeMap<VarId, Nat>, default: Nat) -> Self {
        Self {
            known,
            absent: default,
        }
    }

    /// Requires every variable in `vars` to have an explicit value.
    ///
    /// # Errors
    ///
    /// [`BoundError::UnboundVariable`] naming the **least** absent variable in
    /// [`VarId`] order. "Least", not "first": a `HashSet`-ordered "first"
    /// differs between two runs of the same binary, and this message reaches a
    /// user-visible diagnostic and a CI log diff.
    pub fn require_total(self, vars: impl IntoIterator<Item = VarId>) -> Result<Self, BoundError> {
        // The **least** absent variable, not the first one offered: the
        // caller's iteration order reaches a user-visible message otherwise.
        let mut least: Option<VarId> = None;
        for var in vars {
            if self.known.contains_key(&var) {
                continue;
            }
            least = match least {
                Some(current) if current <= var => Some(current),
                _ => Some(var),
            };
        }
        match least {
            Some(var) => Err(BoundError::UnboundVariable { var }),
            None => Ok(self),
        }
    }
}

impl Valuation for TotalValuation {
    fn value_of(&self, var: &VarId) -> Nat {
        self.known.get(var).copied().unwrap_or(self.absent)
    }
}
