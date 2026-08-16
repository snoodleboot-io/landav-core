//! [`Cost`] - what one step of a [`crate::Transition`] is worth.

use crate::{its_var::ItsVar, polynomial::Polynomial};

/// The resource a single traversal of a transition consumes.
///
/// A bound on a program is a bound on the *sum of the costs* of the transitions
/// its executions take. With every cost equal to one that sum is a step count,
/// which is what this crate emitted before costs existed and what it still
/// emits by default.
///
/// # Why this is a polynomial and not a number
///
/// A cost may depend on the state. A loop body that copies an `n`-element slice
/// costs `n` per iteration, not one, and running it `n` times is quadratic
/// rather than linear. Collapsing that to a constant would not be a loose bound,
/// it would be the wrong *shape* - and shape is the whole output of this
/// analysis. KoAT accepts a polynomial in the rule's variables here, so nothing
/// is lost by carrying one.
///
/// # Non-negativity
///
/// Costs are non-negative. A negative cost would let an execution *refund*
/// resource, so a longer run could carry a smaller total and the bound would no
/// longer be monotone in the number of steps taken - which is the property every
/// upper bound in this crate rests on.
///
/// The constructors enforce this as far as it is decidable. [`Cost::constant`]
/// takes a `u32`, so a constant cost cannot be negative by construction.
/// [`Cost::stateful`] cannot: whether a polynomial is non-negative over the
/// states a guard admits is not answerable here, and the honest position is that
/// the caller owes it. See that constructor's own note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cost(Polynomial);

impl Cost {
    /// One unit - the default, and what a bare `->` means to KoAT.
    ///
    /// Use this when the transition models one step of control and the question
    /// being asked is "how many steps". That is every transition the Python
    /// lowering currently emits.
    #[must_use]
    pub fn step() -> Self {
        Self(Polynomial::constant(1))
    }

    /// No cost at all.
    ///
    /// For transitions that exist to shape control flow rather than to model
    /// work - the edge into a loop header, say. Charging them one unit does not
    /// make a bound unsound, but it does make it wrong by a constant that grows
    /// with the number of such edges, and that constant is visible in the
    /// output the user reads.
    #[must_use]
    pub fn free() -> Self {
        Self(Polynomial::zero())
    }

    /// A fixed cost.
    ///
    /// `u32` rather than `i64`: it makes the non-negativity invariant hold by
    /// construction, and it widens to `i64` without truncation, so the
    /// conversion here cannot be the lossy `as` cast the workspace lints deny.
    #[must_use]
    pub fn constant(units: u32) -> Self {
        Self(Polynomial::constant(i64::from(units)))
    }

    /// A cost that depends on the state.
    ///
    /// # The caller's obligation
    ///
    /// This constructor cannot check what it would need to check. Deciding
    /// whether a polynomial is non-negative across exactly the states a guard
    /// admits is a satisfiability question over the guard, and this crate does
    /// not have a solver - deliberately, because everything else it does is
    /// syntactic.
    ///
    /// So the obligation is the caller's: the polynomial must be non-negative
    /// on every state its transition's guard admits. In practice that is
    /// discharged by construction rather than by proof - a cost derived from a
    /// loop's trip count is non-negative because the guard is what makes the
    /// loop run at all.
    ///
    /// A caller that cannot discharge it should use [`Cost::constant`] with an
    /// over-estimate instead. A loose bound is a worse answer; an unsound one is
    /// not an answer.
    #[must_use]
    pub const fn stateful(polynomial: Polynomial) -> Self {
        Self(polynomial)
    }

    /// The cost as a polynomial.
    #[must_use]
    pub const fn polynomial(&self) -> &Polynomial {
        &self.0
    }

    /// Whether this is exactly one unit.
    ///
    /// The renderer uses this to emit a bare `->`, which KoAT reads as cost one.
    /// That keeps the common case readable and leaves output for step-counting
    /// systems byte-identical to what this crate emitted before costs existed.
    #[must_use]
    pub fn is_step(&self) -> bool {
        self.0.as_constant() == Some(1)
    }

    /// Whether this costs nothing.
    #[must_use]
    pub fn is_free(&self) -> bool {
        self.0.is_zero()
    }

    /// The variables the cost reads.
    ///
    /// Every one of these must be in the system's variable tuple, or the
    /// emitted rule mentions a name the solver never bound.
    #[must_use]
    pub fn vars(&self) -> std::collections::BTreeSet<ItsVar> {
        self.0.vars()
    }
}

impl Default for Cost {
    /// [`Cost::step`] - one unit.
    fn default() -> Self {
        Self::step()
    }
}

impl core::fmt::Display for Cost {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use landav_bound::Symbol;

    use super::Cost;
    use crate::{its_var::ItsVar, polynomial::Polynomial};

    #[test]
    fn a_step_is_one_unit() {
        assert!(Cost::step().is_step());
        assert!(!Cost::step().is_free());
        assert_eq!(Cost::step().polynomial().as_constant(), Some(1));
    }

    #[test]
    fn free_is_zero_and_is_not_a_step() {
        assert!(Cost::free().is_free());
        assert!(!Cost::free().is_step());
        assert_eq!(Cost::free().polynomial().as_constant(), Some(0));
    }

    #[test]
    fn the_default_cost_is_one_step() {
        assert_eq!(Cost::default(), Cost::step());
    }

    /// `u32` is the whole non-negativity argument for a constant cost: no value
    /// of the parameter produces a negative one, so the invariant holds by
    /// construction rather than by a check that a later edit could drop.
    #[test]
    fn a_constant_cost_is_never_negative() {
        for units in [0_u32, 1, 2, 7, u32::from(u16::MAX), u32::MAX] {
            // Asserted against the exact value rather than against `>= 0`: it
            // covers non-negativity and faithfulness in one, and needs no
            // unwrapping to get at the number.
            assert_eq!(
                Cost::constant(units).polynomial().as_constant(),
                Some(i64::from(units)),
                "Cost::constant({units}) did not round-trip"
            );
        }
    }

    /// The widening is `i64::from`, not `as`. `u32::MAX` is the input that would
    /// expose a truncating cast - the workspace denies those because one of them
    /// once turned an enormous bound into a small one.
    #[test]
    fn the_largest_constant_cost_survives_widening() {
        assert_eq!(
            Cost::constant(u32::MAX).polynomial().as_constant(),
            Some(i64::from(u32::MAX))
        );
    }

    #[test]
    fn a_stateful_cost_reports_the_variables_it_reads() {
        let n = ItsVar::new(Symbol::from("n"));
        let cost = Cost::stateful(Polynomial::var(n.clone()));
        assert!(cost.vars().contains(&n));
        assert!(!cost.is_step());
        assert!(!cost.is_free());
    }

    /// A cost of one written the long way is still a step. `is_step` asks what
    /// the polynomial *is*, not how it was built, because the renderer's choice
    /// between `->` and `-{1}>` must not depend on the constructor a caller
    /// happened to reach for.
    #[test]
    fn one_is_a_step_however_it_was_constructed() {
        assert!(Cost::constant(1).is_step());
        assert!(Cost::stateful(Polynomial::constant(1)).is_step());
        assert!(Cost::constant(0).is_free());
        assert!(Cost::stateful(Polynomial::zero()).is_free());
    }
}
