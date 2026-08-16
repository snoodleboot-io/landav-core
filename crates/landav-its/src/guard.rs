//! [`Guard`] - the conjunction a transition is enabled under.

use std::collections::BTreeSet;

use crate::{constraint::Constraint, its_var::ItsVar};

/// A conjunction of [`Constraint`]s.
///
/// **The empty guard is `true`**, not `false`. That is the standard reading of
/// an empty conjunction, and it is also the safe one here: an unguarded
/// transition admits every state, so a lowering bug that loses a constraint
/// produces a system admitting *more* executions than the program has. Under
/// the zero-target soundness rule that is the direction errors must fall, and
/// it is the reason this is a `Vec` of conjuncts rather than an `Option`.
///
/// Constraints are held sorted and deduplicated, so a guard is canonical and
/// two runs emit byte-identical text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Guard {
    constraints: Vec<Constraint>,
}

impl Guard {
    /// The guard that admits everything.
    #[must_use]
    pub const fn always() -> Self {
        Self {
            constraints: Vec::new(),
        }
    }

    /// A guard from a conjunction of constraints.
    ///
    /// Trivially true constraints are dropped: they constrain nothing, and
    /// keeping them would put noise in the emitted text and make two equal
    /// guards compare unequal.
    #[must_use]
    pub fn new(constraints: impl IntoIterator<Item = Constraint>) -> Self {
        let mut kept: Vec<Constraint> = constraints
            .into_iter()
            .filter(|constraint| !constraint.is_trivially_true())
            .collect();
        kept.sort();
        kept.dedup();
        Self { constraints: kept }
    }

    /// The conjuncts, in canonical order.
    #[must_use]
    pub fn constraints(&self) -> &[Constraint] {
        &self.constraints
    }

    /// Whether this guard admits every state.
    #[must_use]
    pub fn is_always(&self) -> bool {
        self.constraints.is_empty()
    }

    /// Whether some conjunct is unsatisfiable on its own.
    ///
    /// A cheap, **incomplete** check: it catches `1 = 0` and nothing cleverer.
    /// Deciding satisfiability of a conjunction of polynomial constraints is
    /// not this crate's job, and a guard that is unsatisfiable for a subtler
    /// reason is merely a transition no execution takes - which costs
    /// precision, never soundness.
    #[must_use]
    pub fn is_trivially_unsatisfiable(&self) -> bool {
        self.constraints.iter().any(Constraint::is_trivially_false)
    }

    /// Whether every conjunct holds under `lookup`.
    ///
    /// `None` if any conjunct could not be decided.
    #[must_use]
    pub fn holds(&self, lookup: &dyn Fn(&ItsVar) -> Option<i128>) -> Option<bool> {
        let mut all = true;
        for constraint in &self.constraints {
            all &= constraint.holds(lookup)?;
        }
        Some(all)
    }

    /// Every variable mentioned, in canonical order.
    #[must_use]
    pub fn vars(&self) -> BTreeSet<ItsVar> {
        self.constraints
            .iter()
            .flat_map(|constraint| constraint.polynomial().vars())
            .collect()
    }
}

impl core::fmt::Display for Guard {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.constraints.is_empty() {
            return f.write_str("true");
        }
        for (index, constraint) in self.constraints.iter().enumerate() {
            if index > 0 {
                f.write_str(" && ")?;
            }
            write!(f, "{constraint}")?;
        }
        Ok(())
    }
}
