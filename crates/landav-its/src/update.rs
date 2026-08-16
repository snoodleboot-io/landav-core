//! [`Update`] - the simultaneous assignment a transition performs.

use std::collections::BTreeMap;

use crate::{its_var::ItsVar, polynomial::Polynomial};

/// What a transition does to the state: a simultaneous assignment.
///
/// # Simultaneous, and over the pre-state
///
/// Every right-hand side is evaluated in the state *before* the transition, and
/// all assignments land together. `{x := y, y := x}` swaps, it does not
/// collapse both to `y`. This matters most in the counted-loop lowering, where
/// the counter and the snapshot of the endpoint are set in one step from
/// expressions that may mention each other's targets.
///
/// # Absence means identity
///
/// A variable this update does not mention keeps its value. That is the only
/// safe default: the alternative - unmentioned means unconstrained - would
/// make every transition havoc the whole state, and while that direction is
/// technically the sound one for a *runtime* bound it would make every emitted
/// system useless. Identity is exact, and the emitter writes the identity out
/// explicitly in the KoAT text, because KoAT's rule syntax names every
/// variable positionally and has no notion of an omitted one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Update {
    assignments: BTreeMap<ItsVar, Polynomial>,
}

impl Update {
    /// The update that changes nothing.
    #[must_use]
    pub fn identity() -> Self {
        Self {
            assignments: BTreeMap::new(),
        }
    }

    /// An update from a set of assignments.
    ///
    /// A later assignment to the same variable replaces an earlier one, which
    /// makes the constructor total; the lowering never builds one that way.
    #[must_use]
    pub fn new(assignments: impl IntoIterator<Item = (ItsVar, Polynomial)>) -> Self {
        Self {
            assignments: assignments.into_iter().collect(),
        }
    }

    /// The assignments, in canonical variable order.
    #[must_use]
    pub const fn assignments(&self) -> &BTreeMap<ItsVar, Polynomial> {
        &self.assignments
    }

    /// Whether this update changes nothing.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.assignments.is_empty()
    }

    /// The polynomial assigned to `var`, or `None` if `var` is unchanged.
    #[must_use]
    pub fn get(&self, var: &ItsVar) -> Option<&Polynomial> {
        self.assignments.get(var)
    }

    /// The variables this update writes, in canonical order.
    pub fn targets(&self) -> impl Iterator<Item = &ItsVar> {
        self.assignments.keys()
    }
}

impl core::fmt::Display for Update {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.assignments.is_empty() {
            return f.write_str("skip");
        }
        for (index, (var, value)) in self.assignments.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{var} := {value}")?;
        }
        Ok(())
    }
}
