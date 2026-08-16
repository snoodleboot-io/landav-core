//! Summing a loop body's cost over the values its counter takes.
//!
//! # Why a sum and not a recurrence
//!
//! A counted loop's iteration space is fixed before it starts - `RangeSpec`
//! evaluates its endpoints exactly once - and the fragment refuses `break`,
//! `continue` and exceptions, so nothing leaves early. The cost of the loop is
//! therefore a **definite sum with known limits**:
//!
//! ```text
//! cost(for i in range(0, T): body)  =  sum over k in [0, T) of cost(body)[i := k]
//! ```
//!
//! There is nothing to solve for. That is strictly easier than the recurrence
//! formulations in the literature, and it is a consequence of the refusal set
//! rather than of any cleverness here. **If `break` or an early `return` inside
//! a loop is ever accepted, this argument collapses** - the trip count stops
//! being determined in advance - and this module has to be revisited rather
//! than extended.
//!
//! # What it buys
//!
//! Without it, a body whose cost depends on the counter has to be
//! over-approximated by substituting the trip count for the counter. For
//! triangular nesting that gives `2n^2 + n` where the truth is `n^2` - right
//! shape, about twice too large.
//!
//! # Where the rationals go
//!
//! Faulhaber's formulae have rational coefficients: `sum over k < T of k` is
//! `T^2/2 - T/2`. The halving is unavoidable in the middle of the calculation
//! and usually absent from the end of it, because a coefficient in the body
//! clears the denominator and the negative terms cancel. So the arithmetic is
//! carried in [`Rat`] and converted back only if every surviving coefficient is
//! a non-negative integer. Where it is not, the caller falls back and says so -
//! see [`Rat::to_natural`].

use landav_bound::{Bound, BoundKind, Nat, VarId};

use crate::rational::Rat;

/// The highest power of the counter this module will handle.
///
/// Matches `landav_its::MAX_DEGREE`, which is what the lowering already
/// enforces on expressions, so a body cost cannot arrive with a higher degree
/// than this through any supported path. Declared rather than imported to keep
/// the dependency one-directional; the pair is asserted equal in the tests.
const MAX_DEGREE: usize = 8;

/// One `rational * bound` term. A coefficient is a sum of these.
///
/// The split exists because a coefficient may be symbolic - the body of
/// `for i in range(n): for j in range(m): ...` costs `2m` per iteration, which
/// is a bound - while the arithmetic of summation is rational. Keeping them
/// apart means the rationals can cancel without ever having to divide a bound.
#[derive(Debug, Clone)]
struct Term {
    scale: Rat,
    symbol: Bound,
}

/// A formal sum of [`Term`]s: the coefficient of one power of the counter.
type Coefficient = Vec<Term>;

/// A polynomial in the loop counter, indexed by degree.
///
/// `poly[d]` is the coefficient of `counter^d`.
#[derive(Debug, Clone)]
struct CounterPoly(Vec<Coefficient>);

impl CounterPoly {
    fn zero() -> Self {
        Self(Vec::new())
    }

    /// A degree-zero polynomial.
    ///
    /// # Constants go in the scale, not the symbol
    ///
    /// This split is load-bearing rather than cosmetic. Cancellation happens by
    /// grouping terms with the *same symbol* and adding their rationals, so a
    /// numeric factor left in the symbol makes two terms that should combine
    /// look distinct.
    ///
    /// Concretely: `sum over k < T of (1 + 2k)` produces `1 * one` from the
    /// constant and `-1/2 * two` from Faulhaber. With the `2` stuck in the
    /// symbol those never meet, the `-1/2` survives, and an answer that is
    /// exactly `T^2` is refused for having a denominator. With it in the scale
    /// they are both over `one`, sum to zero, and drop out.
    fn constant(bound: Bound) -> Self {
        let term = match bound.kind() {
            BoundKind::Const(Nat::Fin(value)) => Term {
                scale: Rat::whole(i128::from(*value)),
                symbol: Bound::one(),
            },
            // `omega` is not a rational and there is no arithmetic to do with
            // it; anything else is genuinely symbolic and stays that way.
            _ => Term {
                scale: Rat::whole(1),
                symbol: bound,
            },
        };
        Self(vec![vec![term]])
    }

    /// The counter itself: `1 * counter^1`.
    fn counter() -> Self {
        Self(vec![
            Vec::new(),
            vec![Term {
                scale: Rat::whole(1),
                symbol: Bound::one(),
            }],
        ])
    }

    fn degree(&self) -> usize {
        self.0.len().saturating_sub(1)
    }

    fn add(mut self, other: Self) -> Self {
        if other.0.len() > self.0.len() {
            self.0.resize(other.0.len(), Vec::new());
        }
        for (slot, terms) in self.0.iter_mut().zip(other.0) {
            slot.extend(terms);
        }
        self
    }

    /// Polynomial multiplication. `None` when the product would exceed
    /// [`MAX_DEGREE`] or a coefficient overflows.
    fn multiply(self, other: Self) -> Option<Self> {
        if self.0.is_empty() || other.0.is_empty() {
            return Some(Self::zero());
        }
        let degree = self.degree().checked_add(other.degree())?;
        if degree > MAX_DEGREE {
            return None;
        }
        let mut out = vec![Vec::new(); degree + 1];
        for (left_degree, left_terms) in self.0.iter().enumerate() {
            for (right_degree, right_terms) in other.0.iter().enumerate() {
                for left in left_terms {
                    for right in right_terms {
                        out[left_degree + right_degree].push(Term {
                            scale: left.scale.times(right.scale)?,
                            symbol: Bound::prod([left.symbol.clone(), right.symbol.clone()]),
                        });
                    }
                }
            }
        }
        Some(Self(out))
    }
}

/// Read a cost as a polynomial in `counter`, or `None` if it is not one.
///
/// # What is refused, and why refusal is the right answer
///
/// `Max` is the interesting case. A cost containing the counter under a
/// maximum is not a polynomial in it, and there is no distributive law to
/// recover one. `Pow` and any opaque node are the same. Returning `None` sends
/// the caller to the over-approximation, which is sound.
///
/// A cost that does **not** mention the counter is a degree-zero polynomial
/// whatever its shape, so those pass through whole - which is the common case
/// and the reason this is worth doing at all.
fn read(cost: &Bound, counter: &VarId) -> Option<CounterPoly> {
    if !cost.may_contain_var(counter) {
        return Some(CounterPoly::constant(cost.clone()));
    }
    match cost.kind() {
        BoundKind::Var(_) => Some(CounterPoly::counter()),
        BoundKind::Sum(terms) => terms
            .as_slice()
            .iter()
            .try_fold(CounterPoly::zero(), |acc, term| {
                Some(acc.add(read(term, counter)?))
            }),
        BoundKind::Prod(terms) => {
            let mut product = CounterPoly::constant(Bound::one());
            for term in terms.as_slice() {
                product = product.multiply(read(term, counter)?)?;
            }
            Some(product)
        }
        // `Const` cannot mention the counter and is handled above. Everything
        // remaining - `Max`, and any transcendental node - has no polynomial
        // reading once the counter is inside it.
        _ => None,
    }
}

/// Faulhaber: the coefficients of `sum over k in [0, T) of k^d`, as a
/// polynomial in `T` indexed by power.
///
/// # The recurrence, rather than a table
///
/// Telescoping `(k+1)^(d+1) - k^(d+1)` from `0` to `T-1` gives
///
/// ```text
/// T^(d+1) = sum over j <= d of C(d+1, j) * S_j(T)
/// ```
///
/// so `S_d(T) = [T^(d+1) - sum over j < d of C(d+1, j) * S_j(T)] / (d + 1)`.
///
/// Computed rather than tabulated because a nine-entry table of rational
/// coefficients is nine chances to mistype a number, and the recurrence is
/// checkable against the two closed forms everybody knows.
fn faulhaber(degree: usize) -> Option<Vec<Rat>> {
    let mut table: Vec<Vec<Rat>> = Vec::with_capacity(degree + 1);
    for d in 0..=degree {
        // Start from `T^(d+1)`.
        let mut coefficients = vec![Rat::ZERO; d + 2];
        coefficients[d + 1] = Rat::whole(1);

        for (j, lower) in table.iter().enumerate() {
            let binomial = binomial(d + 1, j)?;
            for (power, coefficient) in lower.iter().enumerate() {
                let scaled = coefficient.times(Rat::whole(binomial))?;
                let slot = coefficients.get_mut(power)?;
                *slot = slot.plus(Rat::ZERO.plus(scaled)?.times(Rat::whole(-1))?)?;
            }
        }

        let divisor = i128::try_from(d).ok()?.checked_add(1)?;
        for coefficient in &mut coefficients {
            *coefficient = coefficient.divide_by_whole(divisor)?;
        }
        table.push(coefficients);
    }
    table.pop()
}

/// `C(n, k)`, for the small `n` this module reaches.
fn binomial(n: usize, k: usize) -> Option<i128> {
    if k > n {
        return Some(0);
    }
    let mut result: i128 = 1;
    for step in 0..k {
        let numerator = i128::try_from(n.checked_sub(step)?).ok()?;
        let denominator = i128::try_from(step.checked_add(1)?).ok()?;
        result = result.checked_mul(numerator)?.checked_div(denominator)?;
    }
    Some(result)
}

/// `sum over k in [0, trip_count) of body[counter := k]`, exactly, or `None`.
///
/// `None` means no *exact* answer is writable - either the body is not a
/// polynomial in the counter, or the closed form has a fractional or negative
/// coefficient that the bound algebra cannot hold. The caller must then
/// over-approximate and weaken its claim; it must never round.
pub fn sum_over_counter(body: &Bound, counter: &VarId, trip_count: &Bound) -> Option<Bound> {
    let poly = read(body, counter)?;

    // Accumulate the answer as a polynomial in the trip count: for each degree
    // `d` in the counter, the body's coefficient times Faulhaber's `S_d`.
    let mut answer: Vec<Coefficient> = Vec::new();
    for (d, coefficient) in poly.0.iter().enumerate() {
        if coefficient.is_empty() {
            continue;
        }
        let closed = faulhaber(d)?;
        if closed.len() > answer.len() {
            answer.resize(closed.len(), Vec::new());
        }
        for (power, scale) in closed.iter().enumerate() {
            if scale.is_zero() {
                continue;
            }
            for term in coefficient {
                answer.get_mut(power)?.push(Term {
                    scale: term.scale.times(*scale)?,
                    symbol: term.symbol.clone(),
                });
            }
        }
    }

    // Collect like symbols so cancellation can happen. This is the step the
    // whole module exists for: in `sum over k < T of (1 + 2k)` the degree-one
    // coefficient is `1 - 1`, and without combining the two it never reaches
    // zero and the answer keeps a term it should not have.
    let mut parts = Vec::new();
    for (power, terms) in answer.iter().enumerate() {
        let mut combined: Vec<Term> = Vec::new();
        for term in terms {
            match combined
                .iter_mut()
                .find(|existing| existing.symbol == term.symbol)
            {
                Some(existing) => existing.scale = existing.scale.plus(term.scale)?,
                None => combined.push(term.clone()),
            }
        }
        for term in combined {
            if term.scale.is_zero() {
                continue;
            }
            // The gate. A surviving denominator or a negative coefficient means
            // the closed form exists but is not a bound.
            let scale = term.scale.to_natural()?;
            let mut factors = vec![Bound::constant(scale), term.symbol];
            for _ in 0..power {
                factors.push(trip_count.clone());
            }
            parts.push(Bound::prod(factors));
        }
    }

    Some(Bound::sum(parts))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use landav_bound::{Bound, Nat, Symbol, Valuation, VarId};

    use super::{MAX_DEGREE, faulhaber, sum_over_counter};
    use crate::rational::Rat;

    /// The degree ceiling must track the lowering's, or a body cost could
    /// arrive that this module silently refuses.
    #[test]
    fn the_degree_ceiling_matches_the_lowerings() {
        assert_eq!(
            MAX_DEGREE,
            landav_its::MAX_DEGREE as usize,
            "the summation's degree ceiling has drifted from the lowering's"
        );
    }

    /// The two closed forms everybody knows, as a check on the recurrence.
    #[test]
    fn faulhaber_reproduces_the_known_closed_forms() {
        // S_0(T) = T
        let s0 = faulhaber(0).expect("S_0 exists");
        assert_eq!(s0, vec![Rat::ZERO, Rat::whole(1)]);

        // S_1(T) = T^2/2 - T/2
        let s1 = faulhaber(1).expect("S_1 exists");
        assert_eq!(
            s1,
            vec![
                Rat::ZERO,
                Rat::new(-1, 2).expect("valid"),
                Rat::new(1, 2).expect("valid"),
            ]
        );

        // S_2(T) = T^3/3 - T^2/2 + T/6
        let s2 = faulhaber(2).expect("S_2 exists");
        assert_eq!(
            s2,
            vec![
                Rat::ZERO,
                Rat::new(1, 6).expect("valid"),
                Rat::new(-1, 2).expect("valid"),
                Rat::new(1, 3).expect("valid"),
            ]
        );
    }

    /// Every degree up to the ceiling is computable, checked numerically
    /// against a brute-force sum rather than against another formula.
    #[test]
    fn faulhaber_agrees_with_brute_force_at_every_degree() {
        for d in 0..=MAX_DEGREE {
            let closed = faulhaber(d).unwrap_or_else(|| panic!("S_{d} must exist"));
            for t in 0_i128..8 {
                let brute: i128 = (0..t)
                    .map(|k| k.pow(u32::try_from(d).expect("small")))
                    .sum();
                let mut evaluated = Rat::ZERO;
                for (power, coefficient) in closed.iter().enumerate() {
                    let term = coefficient
                        .times(Rat::whole(t.pow(u32::try_from(power).expect("small"))))
                        .expect("no overflow");
                    evaluated = evaluated.plus(term).expect("no overflow");
                }
                assert_eq!(
                    evaluated,
                    Rat::whole(brute),
                    "S_{d}({t}) disagreed with brute force"
                );
            }
        }
    }

    struct At(u64);

    impl Valuation for At {
        fn value_of(&self, _var: &VarId) -> Nat {
            Nat::Fin(self.0)
        }
    }

    /// The motivating case. `sum over k < n of (1 + 2k)` is `n^2` - the
    /// denominator is cleared by the `2` and the negative term cancels against
    /// the constant one.
    #[test]
    fn the_triangular_sum_closes_to_a_square() {
        let counter = VarId::new(Symbol::from("i"));
        let body = Bound::sum([
            Bound::one(),
            Bound::prod([Bound::constant(2), Bound::var(Symbol::from("i"))]),
        ]);
        let trip = Bound::var(Symbol::from("n"));
        let summed = sum_over_counter(&body, &counter, &trip).expect("this sum closes");

        for n in [0_u64, 1, 2, 3, 7, 20] {
            assert_eq!(
                summed.eval(&At(n)),
                Nat::Fin(n * n),
                "the closed form disagreed with n^2 at n = {n}"
            );
        }
    }

    /// A body that does not mention the counter multiplies rather than sums,
    /// and the sum must agree with that.
    #[test]
    fn a_counter_free_body_multiplies() {
        let counter = VarId::new(Symbol::from("i"));
        let body = Bound::constant(3);
        let trip = Bound::var(Symbol::from("n"));
        let summed = sum_over_counter(&body, &counter, &trip).expect("a constant body sums");
        for n in [0_u64, 1, 5, 11] {
            assert_eq!(summed.eval(&At(n)), Nat::Fin(3 * n));
        }
    }

    /// `sum over k < n of k` is `n(n-1)/2`, which has a surviving denominator
    /// and so is **not** writable as a bound. Refused rather than rounded:
    /// rounding down is unsound and rounding up silently loses exactness.
    #[test]
    fn a_surviving_denominator_is_refused_rather_than_rounded() {
        let counter = VarId::new(Symbol::from("i"));
        let body = Bound::var(Symbol::from("i"));
        let trip = Bound::var(Symbol::from("n"));
        assert_eq!(sum_over_counter(&body, &counter, &trip), None);
    }

    /// A counter under a maximum is not a polynomial in it, and there is no
    /// distributive law to recover one.
    #[test]
    fn a_counter_under_a_maximum_is_refused() {
        let counter = VarId::new(Symbol::from("i"));
        let body = Bound::max_of([Bound::var(Symbol::from("i")), Bound::constant(4)]);
        let trip = Bound::var(Symbol::from("n"));
        assert_eq!(sum_over_counter(&body, &counter, &trip), None);
    }

    /// Checked against brute force rather than against a second derivation, on
    /// a body with a symbolic coefficient as well as a counter-dependent one.
    #[test]
    fn a_quadratic_body_sums_when_the_coefficients_clear() {
        // 6k^2 + 6k + 1  ->  sum over k < n  =  2n^3 - 3n^2 + n + 3n^2 - 3n + n
        //                                    =  2n^3 - n
        // which has a negative coefficient and must be refused.
        let counter = VarId::new(Symbol::from("i"));
        let i = Bound::var(Symbol::from("i"));
        let body = Bound::sum([
            Bound::prod([Bound::constant(6), i.clone(), i.clone()]),
            Bound::prod([Bound::constant(6), i]),
            Bound::one(),
        ]);
        let trip = Bound::var(Symbol::from("n"));
        // Brute force says the sum is 2n^3 - n, whose middle coefficient is
        // negative, so no bound can hold it.
        assert_eq!(sum_over_counter(&body, &counter, &trip), None);
    }
}
