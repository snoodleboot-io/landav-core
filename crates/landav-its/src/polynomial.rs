//! [`Polynomial`] - an integer polynomial over the emitted system's variables.

use std::collections::BTreeSet;

use crate::{MAX_DEGREE, MAX_MONOMIALS, construct::Construct, its_var::ItsVar, monomial::Monomial};

/// A polynomial with `i64` coefficients over [`ItsVar`]s, in canonical form.
///
/// Canonical means: like terms collected, zero coefficients dropped, monomials
/// sorted. Two polynomials are equal as Rust values exactly when they are
/// equal as functions on the integers, which is what lets the KoAT output be
/// byte-reproducible across runs and lets a test compare a lowered update
/// against a hand-written expectation.
///
/// # Every operation is checked, and overflow refuses
///
/// The coefficients are `i64` and the arithmetic is `checked_*` throughout.
/// This is a soundness property, not a robustness nicety: a wrapped
/// coefficient turns `x = 9223372036854775807 + 1` into `x = -9223372036854775808`,
/// which is a *different program*, and a bound derived from a different
/// program can be exceeded by the real one. Overflow therefore produces
/// [`Construct::ArithmeticOverflow`] and the whole lowering refuses, which is
/// the same treatment any other construct outside the fragment gets.
///
/// The two size caps exist for the same reason at a different scale.
/// [`MAX_DEGREE`] bounds the exponents and [`MAX_MONOMIALS`] bounds the term
/// count, because neither bounds the other: `(a + b + c)^8` is shallow, short
/// to write, degree 8, and has 45 terms. Without the second cap a frontend
/// could hand over an expression whose *expansion* is exponential in its
/// size, and the lowering would sit there multiplying.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Polynomial {
    monomials: Vec<Monomial>,
}

impl Polynomial {
    /// The zero polynomial.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            monomials: Vec::new(),
        }
    }

    /// A constant.
    #[must_use]
    pub fn constant(value: i64) -> Self {
        if value == 0 {
            return Self::zero();
        }
        Self {
            monomials: vec![Monomial::constant(value)],
        }
    }

    /// A single variable.
    #[must_use]
    pub fn var(var: ItsVar) -> Self {
        Self {
            monomials: vec![Monomial::linear(1, var)],
        }
    }

    /// Builds from arbitrary monomials, normalising.
    ///
    /// # Errors
    ///
    /// [`Construct::ArithmeticOverflow`] if collecting like terms overflows a
    /// coefficient; [`Construct::PolynomialDegree`] or
    /// [`Construct::PolynomialSize`] if the result exceeds a cap.
    pub fn from_monomials(
        monomials: impl IntoIterator<Item = Monomial>,
    ) -> Result<Self, Construct> {
        let mut terms: Vec<Monomial> = monomials
            .into_iter()
            .filter(|term| term.coefficient() != 0)
            .collect();
        terms.sort();

        let mut collected: Vec<Monomial> = Vec::with_capacity(terms.len());
        for term in terms {
            match collected.last() {
                Some(previous) if previous.powers() == term.powers() => {
                    let sum = previous
                        .coefficient()
                        .checked_add(term.coefficient())
                        .ok_or(Construct::ArithmeticOverflow)?;
                    // `last` was just matched, so the pop cannot fail; doing it
                    // this way keeps the function free of indexing panics.
                    collected.pop();
                    if sum != 0 {
                        collected.push(term.with_coefficient(sum));
                    }
                }
                _ => collected.push(term),
            }
        }

        let result = Self {
            monomials: collected,
        };
        result.check_limits()?;
        Ok(result)
    }

    /// The terms, in canonical order.
    #[must_use]
    pub fn monomials(&self) -> &[Monomial] {
        &self.monomials
    }

    /// Whether this is the zero polynomial.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.monomials.is_empty()
    }

    /// The constant value, if this polynomial is a constant.
    #[must_use]
    pub fn as_constant(&self) -> Option<i64> {
        match self.monomials.as_slice() {
            [] => Some(0),
            [only] if only.is_constant() => Some(only.coefficient()),
            _ => None,
        }
    }

    /// The total degree; zero for the zero polynomial.
    #[must_use]
    pub fn degree(&self) -> u32 {
        self.monomials
            .iter()
            .map(Monomial::degree)
            .max()
            .unwrap_or(0)
    }

    /// Every variable this polynomial mentions, in canonical order.
    #[must_use]
    pub fn vars(&self) -> BTreeSet<ItsVar> {
        self.monomials
            .iter()
            .flat_map(|term| term.powers().iter().map(|(var, _)| var.clone()))
            .collect()
    }

    /// The sum of two polynomials.
    ///
    /// # Errors
    ///
    /// As [`Polynomial::from_monomials`].
    pub fn add(&self, other: &Self) -> Result<Self, Construct> {
        Self::from_monomials(
            self.monomials
                .iter()
                .cloned()
                .chain(other.monomials.iter().cloned()),
        )
    }

    /// The difference of two polynomials.
    ///
    /// # Errors
    ///
    /// As [`Polynomial::from_monomials`], plus
    /// [`Construct::ArithmeticOverflow`] when negating `i64::MIN`.
    pub fn sub(&self, other: &Self) -> Result<Self, Construct> {
        self.add(&other.negate()?)
    }

    /// The negation.
    ///
    /// # Errors
    ///
    /// [`Construct::ArithmeticOverflow`] when a coefficient is `i64::MIN`,
    /// whose negation is not an `i64`. Checked rather than assumed: this is
    /// the single most common place an unchecked negation wraps.
    pub fn negate(&self) -> Result<Self, Construct> {
        let mut negated = Vec::with_capacity(self.monomials.len());
        for term in &self.monomials {
            let coefficient = term
                .coefficient()
                .checked_neg()
                .ok_or(Construct::ArithmeticOverflow)?;
            negated.push(term.with_coefficient(coefficient));
        }
        Ok(Self { monomials: negated })
    }

    /// The product of two polynomials.
    ///
    /// # Errors
    ///
    /// As [`Polynomial::from_monomials`]. The intermediate term count is
    /// checked *before* the products are formed, so a pair of large operands
    /// refuses rather than allocating their product first.
    pub fn multiply(&self, other: &Self) -> Result<Self, Construct> {
        let pairs = self
            .monomials
            .len()
            .checked_mul(other.monomials.len())
            .ok_or(Construct::PolynomialSize)?;
        if pairs > MAX_MONOMIALS {
            return Err(Construct::PolynomialSize);
        }
        let mut products = Vec::with_capacity(pairs);
        for left in &self.monomials {
            for right in &other.monomials {
                products.push(left.multiply(right).ok_or(Construct::ArithmeticOverflow)?);
            }
        }
        Self::from_monomials(products)
    }

    /// This polynomial raised to a literal non-negative power.
    ///
    /// # Errors
    ///
    /// As [`Polynomial::multiply`], plus [`Construct::PolynomialDegree`] if
    /// the exponent alone would exceed [`MAX_DEGREE`].
    pub fn power(&self, exponent: u32) -> Result<Self, Construct> {
        if exponent > MAX_DEGREE {
            return Err(Construct::PolynomialDegree);
        }
        let mut result = Self::constant(1);
        for _ in 0..exponent {
            result = result.multiply(self)?;
        }
        Ok(result)
    }

    /// The value of this polynomial under `lookup`, in `i128`.
    ///
    /// `None` if a variable is unbound or the arithmetic overflows `i128`.
    #[must_use]
    pub fn evaluate(&self, lookup: &dyn Fn(&ItsVar) -> Option<i128>) -> Option<i128> {
        let mut total: i128 = 0;
        for term in &self.monomials {
            total = total.checked_add(term.evaluate(lookup)?)?;
        }
        Some(total)
    }

    /// Fails if either cap is exceeded.
    fn check_limits(&self) -> Result<(), Construct> {
        if self.monomials.len() > MAX_MONOMIALS {
            return Err(Construct::PolynomialSize);
        }
        if self.degree() > MAX_DEGREE {
            return Err(Construct::PolynomialDegree);
        }
        Ok(())
    }
}

impl core::fmt::Display for Polynomial {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.monomials.is_empty() {
            return f.write_str("0");
        }
        for (index, term) in self.monomials.iter().enumerate() {
            let coefficient = term.coefficient();
            if index > 0 {
                f.write_str(if coefficient < 0 { " - " } else { " + " })?;
            } else if coefficient < 0 {
                f.write_str("-")?;
            }
            // `unsigned_abs` rather than `abs`: `i64::MIN.abs()` has no `i64`
            // and would be a panic in debug and a wrap in release.
            let magnitude = coefficient.unsigned_abs();
            let explicit = magnitude != 1 || term.is_constant();
            if explicit {
                write!(f, "{magnitude}")?;
            }
            for (position, (var, exponent)) in term.powers().iter().enumerate() {
                if explicit || position > 0 {
                    f.write_str("*")?;
                }
                write!(f, "{var}")?;
                if *exponent != 1 {
                    write!(f, "^{exponent}")?;
                }
            }
        }
        Ok(())
    }
}
