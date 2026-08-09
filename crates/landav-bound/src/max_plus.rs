//! [`MaxPlus`] - the peak-resource semiring `(Bound u {bottom}, max, +)`.

use crate::{bound::Bound, dioid::Dioid, lifted::Lifted, semiring_id::SemiringId};

/// The peak-resource semiring: `plus` is `max`, `times` is `+`.
///
/// Instantiated by `--resource peak-mem`. Branch join is `max`, sequential
/// composition is `+`, and unbounded repeated allocation is `omega`.
///
/// # `zero` is `Bottom`, and this is not optional
///
/// `(max, +)` over `N u {omega}` is **not a lawful semiring**: `max`'s
/// identity and `+`'s identity are both `0`, so `zero` fails to annihilate
/// `times`. The natural single-implementer choice - `zero = one = Const(0)` -
/// passes any hand-written per-instance test and ships an unsound peak-memory
/// model. The generic law suite rejects it on L4:
/// `times(zero, Elem(1)) = Elem(0 + 1) = Elem(1) != zero`.
///
/// # Known tightness gap
///
/// [`MaxPlus::star`] returns `Elem(omega)` for every non-`Bottom` argument,
/// because `times` is `+` and repeated composition accumulates. For a loop
/// whose body allocates and frees once per iteration the true peak is the body
/// cost, and the reported peak is `omega`. Sound, extremely loose, and a
/// consequence of the algebra having no subtraction: net-effect resources are
/// inexpressible in M0. Recorded so it is a ticket rather than a bug report.
///
/// # `plus` must not be written as `a.max(b)`
///
/// It does not compile, because `Lifted<Bound>` is not `Ord`, because
/// [`Bound`] is not `Ord`. That is deliberate: with a derived syntactic `Ord`
/// the idiomatic one-liner returns `Elem(Var("x"))` for
/// `max(Elem(omega), Elem(Var("x")))` - a peak-memory bound the program
/// exceeds on every run that takes the unbounded branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaxPlus {}

impl Dioid for MaxPlus {
    type Carrier = Lifted<Bound>;

    const SEMIRING: SemiringId = SemiringId::new("peak");
    const PLUS_IDEMPOTENT: bool = true;

    /// [`Lifted::Bottom`] - an unreachable path.
    fn zero() -> Self::Carrier {
        todo!()
    }

    /// `Elem(Const(0))` - a step that holds nothing.
    fn one() -> Self::Carrier {
        todo!()
    }

    /// `Bound::max_of([a, b])`, with `Bottom` as the unit. Idempotent, because
    /// [`crate::MaxTerms`] deduplicates at the type level.
    fn plus(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
        todo!()
    }

    /// `Bound::sum([a, b])`, with `Bottom` absorbing - L4. "Unreachable then
    /// unbounded" is unreachable.
    fn times(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
        todo!()
    }

    /// `one` for `Bottom` (L9) and for `Elem(Const(0))`; `Elem(omega)`
    /// otherwise.
    fn star(a: &Self::Carrier) -> Self::Carrier {
        todo!()
    }
}

/// The fixed grids `MaxPlus`'s laws are checked over.
///
/// Same coverage obligations as [`crate::B`], plus one that is specific to
/// this instance: the grid must contain `Elem(omega)` **and** a symbolic
/// element, because `plus(Elem(omega), Elem(Var(x)))` is the pair that
/// distinguishes a correct `max` from a syntactic one, and evaluating it at
/// `x = 3` is what turns that distinction into a test failure.
#[cfg(any(test, feature = "laws"))]
impl crate::dioid_laws::DioidLaws for MaxPlus {
    fn grid() -> Vec<Self::Carrier> {
        todo!()
    }

    fn valuations() -> Vec<crate::total_valuation::TotalValuation> {
        todo!()
    }

    fn denote(
        value: &Self::Carrier,
        at: &crate::total_valuation::TotalValuation,
    ) -> Lifted<crate::nat::Nat> {
        todo!()
    }
}
