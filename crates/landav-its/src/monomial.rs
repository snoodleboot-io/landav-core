//! [`Monomial`] - a coefficient times a product of variable powers.

use std::collections::BTreeMap;

use crate::its_var::ItsVar;

/// One term of a [`crate::Polynomial`]: `coefficient * x1^e1 * ... * xn^en`.
///
/// The power list is held **sorted by variable with no zero exponents**, which
/// makes it a canonical key: two monomials denote the same product exactly
/// when their power lists are equal, so [`crate::Polynomial`] can collect like
/// terms with a sort and a scan rather than a semantic comparison.
///
/// A coefficient of zero is representable here but never survives into a
/// polynomial; the constructors drop it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Monomial {
    /// Ordered before the coefficient so that `Ord` groups like terms
    /// together, which is what collection relies on.
    powers: Vec<(ItsVar, u32)>,
    coefficient: i64,
}

impl Monomial {
    /// The constant monomial.
    #[must_use]
    pub const fn constant(coefficient: i64) -> Self {
        Self {
            powers: Vec::new(),
            coefficient,
        }
    }

    /// `coefficient * var`.
    #[must_use]
    pub fn linear(coefficient: i64, var: ItsVar) -> Self {
        Self {
            powers: vec![(var, 1)],
            coefficient,
        }
    }

    /// `coefficient` times the product described by `powers`.
    ///
    /// Normalises: exponents for the same variable are summed, zero exponents
    /// are dropped, and the result is sorted. Returns `None` if summing two
    /// exponents would overflow a `u32`, which the degree cap makes
    /// unreachable in practice but which is checked rather than assumed.
    #[must_use]
    pub fn new(coefficient: i64, powers: impl IntoIterator<Item = (ItsVar, u32)>) -> Option<Self> {
        let mut collected: BTreeMap<ItsVar, u32> = BTreeMap::new();
        for (var, exponent) in powers {
            if exponent == 0 {
                continue;
            }
            let slot = collected.entry(var).or_insert(0);
            *slot = slot.checked_add(exponent)?;
        }
        Some(Self {
            powers: collected.into_iter().collect(),
            coefficient,
        })
    }

    /// The coefficient.
    #[must_use]
    pub const fn coefficient(&self) -> i64 {
        self.coefficient
    }

    /// The variable powers, sorted by variable, with no zero exponents.
    #[must_use]
    pub fn powers(&self) -> &[(ItsVar, u32)] {
        &self.powers
    }

    /// The total degree: the sum of the exponents.
    ///
    /// Saturating rather than wrapping - a wrapped degree would compare as
    /// *small* and slip past the cap it exists to enforce.
    #[must_use]
    pub fn degree(&self) -> u32 {
        self.powers.iter().fold(0_u32, |total, (_, exponent)| {
            total.saturating_add(*exponent)
        })
    }

    /// Whether this is the constant term.
    #[must_use]
    pub fn is_constant(&self) -> bool {
        self.powers.is_empty()
    }

    /// The product of two monomials, or `None` on coefficient or exponent
    /// overflow.
    #[must_use]
    pub fn multiply(&self, other: &Self) -> Option<Self> {
        let coefficient = self.coefficient.checked_mul(other.coefficient)?;
        Self::new(
            coefficient,
            self.powers
                .iter()
                .cloned()
                .chain(other.powers.iter().cloned()),
        )
    }

    /// This monomial with its coefficient replaced.
    #[must_use]
    pub fn with_coefficient(&self, coefficient: i64) -> Self {
        Self {
            powers: self.powers.clone(),
            coefficient,
        }
    }

    /// The value of this monomial under `lookup`, in `i128`.
    ///
    /// Returns `None` on overflow. Published because the property suite needs
    /// an evaluator, and evaluation of a *monomial* is simple enough that a
    /// reference implementation of it would be the same code twice; the
    /// polynomial-level and program-level references are where independence
    /// actually buys something.
    #[must_use]
    pub fn evaluate(&self, lookup: &dyn Fn(&ItsVar) -> Option<i128>) -> Option<i128> {
        let mut total = i128::from(self.coefficient);
        for (var, exponent) in &self.powers {
            let value = lookup(var)?;
            for _ in 0..*exponent {
                total = total.checked_mul(value)?;
            }
        }
        Some(total)
    }
}
