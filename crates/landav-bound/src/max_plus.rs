//! [`MaxPlus`] - the peak-resource semiring `(Bound u {bottom}, max, +)`.

use crate::{
    bound::Bound, bound_kind::BoundKind, dioid::Dioid, lifted::Lifted, nat::Nat,
    semiring_id::SemiringId,
};

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
        Lifted::Bottom
    }

    /// `Elem(Const(0))` - a step that holds nothing.
    fn one() -> Self::Carrier {
        Lifted::Elem(Bound::zero())
    }

    /// `Bound::max_of([a, b])`, with `Bottom` as the unit. Idempotent, because
    /// [`crate::MaxTerms`] deduplicates at the type level.
    fn plus(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
        match (a, b) {
            (Lifted::Bottom, other) | (other, Lifted::Bottom) => other.clone(),
            // `Bound::max_of`, never `a.max(b)`: the latter does not compile,
            // and under a derived `Ord` it would return `Var("x")` for
            // `max(omega, x)`.
            (Lifted::Elem(left), Lifted::Elem(right)) => {
                Lifted::Elem(Bound::max_of([left.clone(), right.clone()]))
            }
        }
    }

    /// `Bound::sum([a, b])`, with `Bottom` absorbing - L4. "Unreachable then
    /// unbounded" is unreachable.
    fn times(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
        match (a, b) {
            (Lifted::Bottom, _) | (_, Lifted::Bottom) => Lifted::Bottom,
            (Lifted::Elem(left), Lifted::Elem(right)) => {
                Lifted::Elem(Bound::sum([left.clone(), right.clone()]))
            }
        }
    }

    /// `one` for `Bottom` (L9) and for `Elem(Const(0))`; `Elem(omega)`
    /// otherwise.
    ///
    /// Zero is recognised exactly as it is in [`crate::B::star`], and for the
    /// same reason: `Const(0)` is the only [`Bound`] denoting `0` at every
    /// valuation, so the syntactic test *is* the denotational one. Here `one`
    /// is `Const(0)` rather than `Const(1)`, so the tight answer for a
    /// zero-cost body is "the peak is nothing" rather than "the peak is one".
    fn star(a: &Self::Carrier) -> Self::Carrier {
        match a {
            Lifted::Bottom => Self::one(),
            Lifted::Elem(bound) if bound.kind() == &BoundKind::Const(Nat::ZERO) => Self::one(),
            // The known tightness gap, recorded on the type: a loop body that
            // allocates and frees once per iteration has the body's peak, and
            // this reports `omega`, because the algebra has no subtraction.
            Lifted::Elem(_) => Lifted::Elem(Bound::omega()),
        }
    }
}

/// The fixed grids `MaxPlus`'s laws are checked over.
///
/// Same coverage obligations as [`crate::B`], plus one that is specific to
/// this instance: the grid must contain `Elem(omega)` **and** a symbolic
/// element, because `plus(Elem(omega), Elem(Var(x)))` is the pair that
/// distinguishes a correct `max` from a syntactic one, and evaluating it at
/// `x = 3` is what turns that distinction into a test failure.
///
/// The grids are the same as [`crate::B`]'s, deliberately: both instances have
/// the same carrier, so a grid that exercises one exercises the other, and two
/// divergent grids would make a law failure ambiguous between "the algebra is
/// wrong" and "the grids disagree". `MaxPlus` does not need `B`'s
/// exact-regime constraint on the literals - its `times` is `+`, which only
/// grows, so saturation is monotone on both sides of every regrouping - and
/// `peak::saturation_does_not_break_the_equations_here` pins that at exactly
/// the magnitudes which break `B`.
#[cfg(any(test, feature = "laws"))]
impl crate::dioid_laws::DioidLaws for MaxPlus {
    fn grid() -> Vec<Self::Carrier> {
        <crate::b::B as crate::dioid_laws::DioidLaws>::grid()
    }

    fn valuations() -> Vec<crate::total_valuation::TotalValuation> {
        <crate::b::B as crate::dioid_laws::DioidLaws>::valuations()
    }

    fn denote(
        value: &Self::Carrier,
        at: &crate::total_valuation::TotalValuation,
    ) -> Lifted<crate::nat::Nat> {
        match value {
            Lifted::Bottom => Lifted::Bottom,
            Lifted::Elem(bound) => Lifted::Elem(bound.eval(at)),
        }
    }
}

#[cfg(test)]
mod peak {
    use std::collections::BTreeMap;

    use super::MaxPlus;
    use crate::{
        base::Base, bound::Bound, dioid::Dioid, dioid_laws::DioidLaws, lifted::Lifted, nat::Nat,
        total_valuation::TotalValuation, valuation::Valuation, var_id::VarId,
    };

    fn elem(bound: Bound) -> Lifted<Bound> {
        Lifted::Elem(bound)
    }

    fn at(x: Nat, y: Nat, default: Nat) -> TotalValuation {
        let mut known = BTreeMap::new();
        known.insert(VarId::new("x"), x);
        known.insert(VarId::new("y"), y);
        TotalValuation::with_default(known, default)
    }

    // ---- the five operations ----

    /// `zero` is `Bottom` and `one` is `Const(0)`, and they are **different**.
    ///
    /// The natural single-implementer choice is `zero = one = Const(0)`,
    /// because `max`'s identity and `+`'s identity are both `0`. It passes any
    /// hand-written per-instance test and ships an unsound peak-memory model:
    /// `zero` stops annihilating `times`.
    #[test]
    fn zero_is_bottom_and_one_is_const_zero() {
        assert_eq!(MaxPlus::zero(), Lifted::Bottom);
        assert_eq!(MaxPlus::one(), elem(Bound::zero()));
        assert_ne!(MaxPlus::zero(), MaxPlus::one());
    }

    /// The unsound model, rejected by L4 rather than by review.
    #[test]
    fn const_zero_would_not_annihilate_times() {
        // What `zero = one = Const(0)` would compute for `times(zero, one)`.
        let pretend_zero = elem(Bound::zero());
        assert_eq!(
            MaxPlus::times(&pretend_zero, &elem(Bound::one())),
            elem(Bound::one()),
            "Const(0) is the unit of `+`, so it cannot also be its annihilator"
        );
        // What the real `zero` computes.
        assert_eq!(
            MaxPlus::times(&MaxPlus::zero(), &elem(Bound::one())),
            Lifted::Bottom
        );
    }

    /// `plus` is `max`, with `Bottom` as the unit - and it is **not**
    /// `a.max(b)`, which does not compile, because `Lifted<Bound>` is not
    /// `Ord`. Under a derived syntactic `Ord` the idiomatic one-liner returns
    /// `Var("x")` for `max(omega, x)`: a peak-memory bound the program exceeds
    /// on every run that takes the unbounded branch.
    #[test]
    fn plus_is_max_and_not_a_syntactic_maximum() {
        let top = elem(Bound::omega());
        let symbolic = elem(Bound::var("x"));
        assert_eq!(MaxPlus::plus(&Lifted::Bottom, &top), top);
        assert_eq!(MaxPlus::plus(&top, &Lifted::Bottom), top);
        assert_eq!(
            MaxPlus::plus(&Lifted::Bottom, &Lifted::Bottom),
            Lifted::Bottom
        );

        let joined = MaxPlus::plus(&top, &symbolic);
        assert_eq!(joined, top, "omega absorbs; the variable does not win");

        // The failure this replaces, made concrete: at `x = 3` the discarded
        // branch is unbounded and the reported peak would be 3.
        let point = at(Nat::Fin(3), Nat::Fin(3), Nat::Fin(3));
        assert_eq!(MaxPlus::denote(&joined, &point), Lifted::Elem(Nat::OMEGA));
        assert_ne!(
            MaxPlus::denote(&joined, &point),
            MaxPlus::denote(&symbolic, &point)
        );
    }

    /// `plus` is idempotent, because [`crate::MaxTerms`] deduplicates at the
    /// type level rather than in a normalisation pass.
    #[test]
    fn plus_is_idempotent_on_every_shape() {
        for element in MaxPlus::grid() {
            assert_eq!(
                MaxPlus::plus(&element, &element),
                element,
                "max is idempotent, including on {element:?}"
            );
        }
        const { assert!(MaxPlus::PLUS_IDEMPOTENT) }
    }

    /// `times` is `+`, with `Bottom` absorbing: "unreachable then unbounded"
    /// is unreachable.
    #[test]
    fn times_is_addition_with_bottom_absorbing() {
        let three = elem(Bound::constant(3));
        assert_eq!(MaxPlus::times(&Lifted::Bottom, &three), Lifted::Bottom);
        assert_eq!(MaxPlus::times(&three, &Lifted::Bottom), Lifted::Bottom);
        assert_eq!(
            MaxPlus::times(&Lifted::Bottom, &elem(Bound::omega())),
            Lifted::Bottom
        );
        assert_eq!(MaxPlus::times(&three, &MaxPlus::one()), three);
        assert_eq!(
            MaxPlus::times(&three, &three),
            elem(Bound::constant(6)),
            "sequential composition accumulates"
        );
    }

    // ---- star ----

    /// `star` returns `one` for both zeros and the top otherwise.
    ///
    /// The known tightness gap: for a loop whose body allocates and frees once
    /// per iteration the true peak is the body cost, and this reports `omega`.
    /// Sound, extremely loose, and a consequence of the algebra having no
    /// subtraction.
    #[test]
    fn star_returns_one_for_both_zeros_and_omega_otherwise() {
        assert_eq!(MaxPlus::star(&Lifted::Bottom), MaxPlus::one());
        assert_eq!(MaxPlus::star(&elem(Bound::zero())), MaxPlus::one());
        for other in [
            Bound::one(),
            Bound::constant(3),
            Bound::omega(),
            Bound::var("x"),
            Bound::max_of([Bound::var("x"), Bound::var("y")]),
            Bound::log(Base::TWO, Bound::var("x")),
        ] {
            assert_eq!(
                MaxPlus::star(&elem(other.clone())),
                elem(Bound::omega()),
                "star({other}) must widen to the top"
            );
        }
    }

    /// `star` is a function of the denotation here too: two extensionally
    /// equal carrier values must have extensionally equal `star`s.
    #[test]
    fn star_agrees_on_extensionally_equal_elements() {
        let points = MaxPlus::valuations();
        let agrees = |left: &Lifted<Bound>, right: &Lifted<Bound>| {
            points
                .iter()
                .all(|point| MaxPlus::denote(left, point) == MaxPlus::denote(right, point))
        };

        let grid = MaxPlus::grid();
        for left in &grid {
            for right in &grid {
                if agrees(left, right) {
                    assert!(
                        agrees(&MaxPlus::star(left), &MaxPlus::star(right)),
                        "star disagrees on extensionally equal {left:?} and {right:?}"
                    );
                }
            }
        }

        let left = elem(Bound::max_of([
            Bound::var("x"),
            Bound::sum([Bound::var("x"), Bound::one()]),
        ]));
        let right = elem(Bound::sum([Bound::var("x"), Bound::one()]));
        assert_ne!(left, right);
        assert!(agrees(&left, &right));
        assert_eq!(MaxPlus::star(&left), MaxPlus::star(&right));
    }

    /// `star` over-approximates the true closure `max(0, a, a+a, ...)` at
    /// every valuation.
    #[test]
    fn star_over_approximates_the_true_closure_at_every_valuation() {
        for element in MaxPlus::grid() {
            let starred = MaxPlus::star(&element);
            for point in MaxPlus::valuations() {
                let exact = match MaxPlus::denote(&element, &point) {
                    Lifted::Bottom | Lifted::Elem(Nat::Fin(0)) => Lifted::Elem(Nat::ZERO),
                    Lifted::Elem(_) => Lifted::Elem(Nat::OMEGA),
                };
                let reported = MaxPlus::denote(&starred, &point);
                assert!(
                    reported >= exact,
                    "star({element:?}) denotes {reported:?} where the closure is {exact:?}"
                );
            }
        }
    }

    // ---- the grids ----

    /// The same obligations as [`crate::B`], plus one specific to this
    /// instance: the grid must contain `Elem(omega)` **and** a symbolic
    /// element, because that is the pair which distinguishes a correct `max`
    /// from a syntactic one.
    #[test]
    fn the_element_grid_meets_every_documented_obligation() {
        let grid = MaxPlus::grid();
        let shapes: Vec<_> = grid
            .iter()
            .filter_map(|element| element.as_elem().map(Bound::shape))
            .collect();

        assert!(grid.contains(&Lifted::Bottom), "zero");
        assert!(grid.contains(&MaxPlus::one()), "one, which is Const(0)");
        assert!(grid.contains(&elem(Bound::omega())), "the top");
        assert!(grid.contains(&elem(Bound::var("x"))), "a symbolic element");
        assert!(
            grid.iter()
                .filter_map(Lifted::as_elem)
                .filter(|bound| !bound.vars().is_empty())
                .count()
                >= 2,
            "at least two symbolic elements"
        );
        for required in [
            crate::bound_shape::BoundShape::Const,
            crate::bound_shape::BoundShape::Var,
            crate::bound_shape::BoundShape::Sum,
            crate::bound_shape::BoundShape::Max,
            crate::bound_shape::BoundShape::Prod,
            crate::bound_shape::BoundShape::Trans,
        ] {
            assert!(shapes.contains(&required), "a compound {required:?} term");
        }
        assert_eq!(MaxPlus::grid(), grid, "deterministic");
    }

    /// The all-zero, all-one, large-finite and all-`omega` points, and a point
    /// that tells the two variables apart.
    #[test]
    fn the_valuation_grid_meets_every_documented_obligation() {
        let points = MaxPlus::valuations();
        let x = VarId::new("x");
        let y = VarId::new("y");

        assert!(
            points
                .iter()
                .any(|point| point.value_of(&x) == Nat::ZERO && point.value_of(&y) == Nat::ZERO),
            "an all-zero point"
        );
        assert!(
            points
                .iter()
                .any(|point| point.value_of(&x) == Nat::ONE && point.value_of(&y) == Nat::ONE),
            "an all-one point"
        );
        assert!(
            points
                .iter()
                .any(|point| { matches!(point.value_of(&x), Nat::Fin(value) if value >= 1 << 20) }),
            "a large finite point"
        );
        assert!(
            points
                .iter()
                .any(|point| point.value_of(&x) == Nat::OMEGA && point.value_of(&y) == Nat::OMEGA),
            "an all-omega point"
        );
        assert!(
            points
                .iter()
                .any(|point| point.value_of(&x) != point.value_of(&y)),
            "a point that tells the two variables apart"
        );
        assert_eq!(MaxPlus::valuations(), points, "deterministic");
    }

    /// `denote` is `eval`, lifted.
    #[test]
    fn denote_is_eval_with_bottom_carried_through() {
        let point = at(Nat::Fin(3), Nat::Fin(5), Nat::ZERO);
        assert_eq!(MaxPlus::denote(&Lifted::Bottom, &point), Lifted::Bottom);
        assert_ne!(
            MaxPlus::denote(&Lifted::Bottom, &point),
            MaxPlus::denote(&MaxPlus::one(), &point),
            "an unreachable path is not a step that holds nothing"
        );
        for bound in [
            Bound::zero(),
            Bound::omega(),
            Bound::var("x"),
            Bound::max_of([Bound::var("x"), Bound::var("y")]),
        ] {
            assert_eq!(
                MaxPlus::denote(&elem(bound.clone()), &point),
                Lifted::Elem(bound.eval(&point))
            );
        }
    }

    /// `MaxPlus` has neither of [`crate::B`]'s exact-regime boundaries: its
    /// `times` is `+`, which only grows, so saturation is monotone on both
    /// sides of every regrouping. Pinned at the magnitudes that break `B`.
    #[test]
    fn saturation_does_not_break_the_equations_here() {
        let big = elem(Bound::constant(1u64 << 40));
        let unit = MaxPlus::one();

        assert_eq!(
            MaxPlus::times(&MaxPlus::times(&big, &big), &unit),
            MaxPlus::times(&big, &MaxPlus::times(&big, &unit))
        );
        let half = elem(Bound::constant(1u64 << 63));
        assert_eq!(
            MaxPlus::times(&unit, &MaxPlus::plus(&half, &half)),
            MaxPlus::plus(&MaxPlus::times(&unit, &half), &MaxPlus::times(&unit, &half))
        );
    }

    /// The whole suite, for this instance.
    #[test]
    fn max_plus_satisfies_every_law() {
        let outcome = crate::dioid_laws::check_dioid_laws::<MaxPlus>();
        assert!(outcome.is_ok(), "MaxPlus violates a law: {outcome:?}");
    }

    /// `MaxPlus` and [`crate::B`] are genuinely different algebras over the
    /// same carrier - which is the point of LAN-59: peak memory is a different
    /// semiring over the same engine, not a second engine.
    #[test]
    fn the_two_instances_are_different_algebras_over_one_carrier() {
        let three = elem(Bound::constant(3));
        let five = elem(Bound::constant(5));
        assert_eq!(MaxPlus::plus(&three, &five), elem(Bound::constant(5)));
        assert_eq!(crate::b::B::plus(&three, &five), elem(Bound::constant(8)));
        assert_eq!(MaxPlus::times(&three, &five), elem(Bound::constant(8)));
        assert_eq!(crate::b::B::times(&three, &five), elem(Bound::constant(15)));
        assert_ne!(MaxPlus::one(), crate::b::B::one());
        assert_eq!(
            MaxPlus::zero(),
            crate::b::B::zero(),
            "one bottom, both algebras"
        );
    }
}
