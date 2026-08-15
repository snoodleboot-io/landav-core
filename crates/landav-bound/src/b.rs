//! [`B`] - the additive cost semiring `(N u {omega} u {bottom}, +, *)`.

use crate::{bound::Bound, dioid::Dioid, lifted::Lifted, semiring_id::SemiringId};

/// The additive cost semiring: `plus` is `+`, `times` is `*`.
///
/// Instantiated by `--resource ops`, `--resource alloc` and
/// `--resource queries`: three resources, one algebra, distinct only in what
/// the frontend counts. That sharing is exactly why [`Dioid::SEMIRING`] must
/// not be a cache key.
///
/// Uninhabited on purpose - a type-level witness, never a value.
///
/// # Carrier
///
/// [`Lifted<Bound>`], not `Bound`. `zero` is [`Lifted::Bottom`], not
/// `Const(0)`. See [`Lifted`] for the failure this prevents; the short version
/// is that `Const(0)` is a legitimate cost, so using it as the annihilator and
/// the fixpoint seed makes "proved to cost nothing" indistinguishable from
/// "we have not computed this yet".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum B {}

impl Dioid for B {
    type Carrier = Lifted<Bound>;

    const SEMIRING: SemiringId = SemiringId::new("additive");
    const PLUS_IDEMPOTENT: bool = false;

    /// [`Lifted::Bottom`].
    fn zero() -> Self::Carrier {
        todo!()
    }

    /// `Elem(Const(1))`.
    fn one() -> Self::Carrier {
        todo!()
    }

    /// `Bottom` is the unit; otherwise `Bound::sum([a, b])`.
    fn plus(_a: &Self::Carrier, _b: &Self::Carrier) -> Self::Carrier {
        todo!()
    }

    /// `Bottom` absorbs (L4); otherwise `Bound::prod([a, b])`.
    ///
    /// L4 holds through `Bottom`, which is why [`crate::Nat::times`] is free
    /// to let `omega` absorb `0`.
    fn times(_a: &Self::Carrier, _b: &Self::Carrier) -> Self::Carrier {
        todo!()
    }

    /// `one` for `Bottom` (L9) and for `Elem(Const(0))`; `Elem(omega)`
    /// otherwise.
    ///
    /// The `Elem(Const(0))` case is a *syntactic* test, and it is well defined
    /// only because the smart constructors constant-fold: any closed term
    /// denoting zero folds to `Const(0)`, so `star` is a function of the
    /// denotation rather than of the spelling. Without that folding,
    /// `star(log2(Const(1)))` and `star(Const(0))` would give two different
    /// sound answers for one denotation - a determinism hazard no law catches.
    fn star(_a: &Self::Carrier) -> Self::Carrier {
        todo!()
    }
}

/// The fixed grids `B`'s laws are checked over.
///
/// The element grid must reach, at minimum: `Bottom` (`zero`),
/// `Elem(Const(0))` (the *other* meaning-critical zero), `Elem(Const(1))`
/// (`one`), `Elem(Const(k))` for a large finite `k` near the saturation edge,
/// `Elem(omega)` (the top), `Elem(Var(x))` and `Elem(Var(y))` (symbolic - the
/// fragment constant-only strategies never reach), and at least one compound
/// term of each of `Sum`, `Max`, `Prod` and `Trans`.
///
/// `Elem(Const(1))` is also the required non-idempotence witness for L11:
/// `plus(Elem(1), Elem(1)) == Elem(2) != Elem(1)`.
#[cfg(any(test, feature = "laws"))]
impl crate::dioid_laws::DioidLaws for B {
    fn grid() -> Vec<Self::Carrier> {
        todo!()
    }

    fn valuations() -> Vec<crate::total_valuation::TotalValuation> {
        todo!()
    }

    fn denote(
        _value: &Self::Carrier,
        _at: &crate::total_valuation::TotalValuation,
    ) -> Lifted<crate::nat::Nat> {
        todo!()
    }
}
