//! [`Constraint`] - one atomic condition of a [`crate::Guard`].

use crate::{its_var::ItsVar, polynomial::Polynomial, relation::Relation};

/// The assertion `polynomial R 0`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Constraint {
    polynomial: Polynomial,
    relation: Relation,
}

impl Constraint {
    /// The assertion `polynomial R 0`.
    #[must_use]
    pub const fn new(polynomial: Polynomial, relation: Relation) -> Self {
        Self {
            polynomial,
            relation,
        }
    }

    /// The polynomial being compared to zero.
    #[must_use]
    pub const fn polynomial(&self) -> &Polynomial {
        &self.polynomial
    }

    /// The comparison.
    #[must_use]
    pub const fn relation(&self) -> Relation {
        self.relation
    }

    /// Whether the constraint holds under `lookup`.
    ///
    /// `None` when the polynomial cannot be evaluated - an unbound variable,
    /// or arithmetic beyond `i128`. Deliberately not `false`: "does not hold"
    /// and "could not be decided" are different answers, and collapsing them
    /// would let a reference semantics quietly disagree with itself.
    #[must_use]
    pub fn holds(&self, lookup: &dyn Fn(&ItsVar) -> Option<i128>) -> Option<bool> {
        self.polynomial
            .evaluate(lookup)
            .map(|value| self.relation.holds(value))
    }

    /// Whether this constraint is trivially true regardless of any variable.
    #[must_use]
    pub fn is_trivially_true(&self) -> bool {
        self.polynomial
            .as_constant()
            .is_some_and(|value| self.relation.holds(i128::from(value)))
    }

    /// Whether this constraint is unsatisfiable regardless of any variable.
    #[must_use]
    pub fn is_trivially_false(&self) -> bool {
        self.polynomial
            .as_constant()
            .is_some_and(|value| !self.relation.holds(i128::from(value)))
    }
}

impl core::fmt::Display for Constraint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} {} 0", self.polynomial, self.relation)
    }
}
