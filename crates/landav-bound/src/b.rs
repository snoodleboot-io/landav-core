//! [`B`] - the additive cost semiring `(N u {omega} u {bottom}, +, *)`.

use crate::{
    bound::Bound, bound_kind::BoundKind, dioid::Dioid, lifted::Lifted, nat::Nat,
    semiring_id::SemiringId,
};

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
        Lifted::Bottom
    }

    /// `Elem(Const(1))`.
    fn one() -> Self::Carrier {
        Lifted::Elem(Bound::one())
    }

    /// `Bottom` is the unit; otherwise `Bound::sum([a, b])`.
    fn plus(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
        match (a, b) {
            (Lifted::Bottom, other) | (other, Lifted::Bottom) => other.clone(),
            (Lifted::Elem(left), Lifted::Elem(right)) => {
                Lifted::Elem(Bound::sum([left.clone(), right.clone()]))
            }
        }
    }

    /// `Bottom` absorbs (L4); otherwise `Bound::prod([a, b])`.
    ///
    /// L4 holds through `Bottom`, which is why [`crate::Nat::times`] is free
    /// to let `omega` absorb `0`.
    fn times(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier {
        match (a, b) {
            (Lifted::Bottom, _) | (_, Lifted::Bottom) => Lifted::Bottom,
            (Lifted::Elem(left), Lifted::Elem(right)) => {
                Lifted::Elem(Bound::prod([left.clone(), right.clone()]))
            }
        }
    }

    /// `one` for `Bottom` (L9) and for `Elem(Const(0))`; `Elem(omega)`
    /// otherwise.
    ///
    /// # What the `Const(0)` test is a function of
    ///
    /// The test is syntactic - `Bottom`, or an `Elem` whose folded term is
    /// `Const(0)` - and it is **exactly** the test "does this element denote
    /// `0` at every valuation". That equivalence is a theorem about this
    /// algebra rather than a hope:
    ///
    /// * `Const(0)` denotes `0` everywhere, by definition;
    /// * every other *closed* term folds to some other `Const`, and denotes
    ///   that;
    /// * every term containing a variable denotes `omega` at the all-`omega`
    ///   valuation, because every constructor propagates `omega` upwards -
    ///   `Sum`, `Max` and `Prod` through [`Nat`], `Pow` through
    ///   [`Nat::MAX_FINITE_EXPONENT`], and `Log` through
    ///   `ceil_log(omega) = omega`.
    ///
    /// So `star` **is** a function of the carrier's denotation.
    /// `additive::const_zero_is_the_only_bound_denoting_zero_everywhere` pins
    /// the theorem and `additive::star_agrees_on_extensionally_equal_elements`
    /// pins the consequence.
    ///
    /// # What is *not* true, and used to be claimed here
    ///
    /// This comment previously justified the test with *"any closed term
    /// denoting zero folds to `Const(0)`, so `star` is a function of the
    /// denotation rather than of the spelling"*. The premise is **false**.
    /// `prod([prod([2^40, 2^40]), 0])` folds to `Const(omega)` while
    /// `prod([0, 2^40, 2^40])` folds to `Const(0)`: the inner subgroup
    /// saturates before the zero is ever in scope, and [`Nat::times`] lets
    /// `omega` absorb unconditionally from there on. Both recipes are closed
    /// and both have ideal value `0`.
    /// `adversary::a_saturating_subgroup_still_defeats_an_enclosing_zero` pins
    /// the algebra.
    ///
    /// The conclusion survives without the premise, and the determinism hazard
    /// the old comment feared does not arise: the two recipes build two
    /// **different carrier values** with two **different denotations**, `0` and
    /// `omega`, so `star` answering differently is `star` agreeing with the
    /// denotation rather than disagreeing with itself. What is genuinely lost
    /// is *tightness*, and only for a recipe whose literals saturate - `star`
    /// reports `omega` where `one` was available. That is the direction this
    /// crate always errs in, and [`Bound::prod`], not `star`, is where it would
    /// have to be recovered.
    fn star(a: &Self::Carrier) -> Self::Carrier {
        match a {
            Lifted::Bottom => Self::one(),
            // The unique bound denoting `0` at every valuation; see above.
            Lifted::Elem(bound) if bound.kind() == &BoundKind::Const(Nat::ZERO) => Self::one(),
            // Sound for every other element, tight for none of them.
            // Tightness for counted loops comes from `times`, never here.
            Lifted::Elem(_) => Lifted::Elem(Bound::omega()),
        }
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
///
/// # Why the large literal is `2^31` and not something nearer `u64::MAX`
///
/// "Near the saturation edge" means the edge of `times`, not the edge of the
/// carrier. `2^31` squared is `2^62`, still inside `u64`; `2^31` cubed is not,
/// so the grid does exercise saturation. A literal whose *square* saturates
/// would put the grid outside the regime in which this algebra satisfies L2
/// as an equation - `Const(omega)` then absorbs an enclosing zero where the
/// other grouping folds the zero first - and the same holds for L3 once two
/// finite denotations sum past `u64`. Both boundaries are sound in the
/// over-approximating direction and both are pinned by
/// `additive::a_saturating_literal_product_breaks_times_associativity` and
/// `additive::a_saturating_sum_breaks_distributivity_under_a_zero_factor`, so
/// the constant is a recorded decision rather than a lucky number.
#[cfg(any(test, feature = "laws"))]
impl crate::dioid_laws::DioidLaws for B {
    fn grid() -> Vec<Self::Carrier> {
        use crate::base::Base;

        vec![
            // The two meaning-critical zeros, with opposite provenance.
            Lifted::Bottom,
            Lifted::Elem(Bound::zero()),
            // `one`, and L11's non-idempotence witness: 1 + 1 = 2.
            Lifted::Elem(Bound::one()),
            Lifted::Elem(Bound::constant(3)),
            // Large enough that its cube saturates; small enough that its
            // square does not. See this impl's doc comment.
            Lifted::Elem(Bound::constant(1 << 31)),
            // The top.
            Lifted::Elem(Bound::omega()),
            // Symbolic: the fragment constant-only strategies never reach.
            Lifted::Elem(Bound::var("x")),
            Lifted::Elem(Bound::var("y")),
            // One compound term of each shape.
            Lifted::Elem(Bound::sum([Bound::var("x"), Bound::one()])),
            Lifted::Elem(Bound::max_of([Bound::var("x"), Bound::var("y")])),
            Lifted::Elem(Bound::prod([Bound::var("x"), Bound::var("y")])),
            // `zero (*) symbolic`, which `Bound::prod` deliberately does not
            // fold: it is `omega` at `x = omega`. This is where L4's
            // interesting case lives.
            Lifted::Elem(Bound::prod([Bound::zero(), Bound::var("x")])),
            Lifted::Elem(Bound::pow(Base::TWO, Bound::var("x"))),
            Lifted::Elem(Bound::log(Base::TWO, Bound::var("x"))),
        ]
    }

    fn valuations() -> Vec<crate::total_valuation::TotalValuation> {
        use crate::{nat::Nat, total_valuation::TotalValuation, var_id::VarId};
        use std::collections::BTreeMap;

        let point = |x: Nat, y: Nat, absent: Nat| {
            let mut known = BTreeMap::new();
            known.insert(VarId::new("x"), x);
            known.insert(VarId::new("y"), y);
            TotalValuation::with_default(known, absent)
        };
        vec![
            // All zero: where `log2` and `prod` reach their own zero.
            point(Nat::ZERO, Nat::ZERO, Nat::ZERO),
            // All one: where `log2(1) = 0` without the term being `Const(0)`.
            point(Nat::ONE, Nat::ONE, Nat::ONE),
            // Small and asymmetric: the point that tells `x` and `y` apart,
            // which is what catches a `plus` that returns the wrong operand.
            point(Nat::Fin(3), Nat::Fin(5), Nat::ZERO),
            // Large and finite: `2^31 * 2^20` is `2^51`, so products are big
            // and every pairwise sum of denotations still fits in `u64`.
            point(Nat::Fin(1 << 31), Nat::Fin(1 << 20), Nat::Fin(1 << 20)),
            // The top, which is what makes `Const(0)` the unique element
            // denoting zero everywhere - the theorem `star` rests on.
            point(Nat::OMEGA, Nat::OMEGA, Nat::OMEGA),
        ]
    }

    fn denote(
        value: &Self::Carrier,
        at: &crate::total_valuation::TotalValuation,
    ) -> Lifted<crate::nat::Nat> {
        match value {
            // `Bottom` is carried through, never folded into `Elem(0)`: they
            // are the two meaning-critical zeros, and L6 and L8 both turn on
            // telling them apart.
            Lifted::Bottom => Lifted::Bottom,
            Lifted::Elem(bound) => Lifted::Elem(bound.eval(at)),
        }
    }
}

#[cfg(test)]
mod additive {
    use std::collections::BTreeMap;

    use super::B;
    use crate::{
        base::Base, bound::Bound, bound_kind::BoundKind, dioid::Dioid, dioid_laws::DioidLaws,
        lifted::Lifted, nat::Nat, total_valuation::TotalValuation, valuation::Valuation,
        var_id::VarId,
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

    /// `zero` is `Bottom`, **not** `Const(0)`. `Const(0)` is a legitimate,
    /// common cost - "proved to cost nothing" - and using it as the
    /// annihilator makes that indistinguishable from "no execution reaches
    /// here" and from "not yet computed".
    #[test]
    fn zero_is_bottom_and_one_is_const_one() {
        assert_eq!(B::zero(), Lifted::Bottom);
        assert_eq!(B::one(), elem(Bound::one()));
        assert_ne!(B::zero(), elem(Bound::zero()));
        assert_ne!(B::zero(), B::one());
    }

    /// `plus` is `+`, with `Bottom` as the unit on both sides.
    #[test]
    fn plus_is_addition_with_bottom_as_the_unit() {
        let one = elem(Bound::one());
        assert_eq!(B::plus(&Lifted::Bottom, &one), one);
        assert_eq!(B::plus(&one, &Lifted::Bottom), one);
        assert_eq!(B::plus(&Lifted::Bottom, &Lifted::Bottom), Lifted::Bottom);
        assert_eq!(B::plus(&one, &one), elem(Bound::constant(2)));
        assert_eq!(
            B::plus(&elem(Bound::var("x")), &elem(Bound::var("y"))),
            elem(Bound::sum([Bound::var("x"), Bound::var("y")]))
        );
    }

    /// `times` is `*`, with `Bottom` **absorbing** on both sides - that is L4,
    /// and it is why `Nat::times` is free to let `omega` absorb `0`.
    #[test]
    fn times_is_multiplication_with_bottom_absorbing() {
        let three = elem(Bound::constant(3));
        assert_eq!(B::times(&Lifted::Bottom, &three), Lifted::Bottom);
        assert_eq!(B::times(&three, &Lifted::Bottom), Lifted::Bottom);
        assert_eq!(
            B::times(&Lifted::Bottom, &elem(Bound::omega())),
            Lifted::Bottom,
            "unreachable then unbounded is unreachable"
        );
        assert_eq!(B::times(&three, &elem(Bound::one())), three);
        assert_eq!(
            B::times(&three, &elem(Bound::var("x"))),
            elem(Bound::prod([Bound::constant(3), Bound::var("x")]))
        );
    }

    // ---- star, and how it recognises zero ----

    /// `star` returns `one` for the two zeros - `Bottom` and `Const(0)` - and
    /// the top of the lattice for everything else. Sound, total, and not
    /// tight: tightness for counted loops comes from `times`, never here.
    #[test]
    fn star_returns_one_for_both_zeros_and_omega_otherwise() {
        assert_eq!(B::star(&Lifted::Bottom), B::one());
        assert_eq!(B::star(&elem(Bound::zero())), B::one());

        for other in [
            Bound::one(),
            Bound::constant(3),
            Bound::omega(),
            Bound::var("x"),
            Bound::sum([Bound::var("x"), Bound::one()]),
            Bound::prod([Bound::zero(), Bound::var("x")]),
            Bound::log(Base::TWO, Bound::var("x")),
        ] {
            assert_eq!(
                B::star(&elem(other.clone())),
                elem(Bound::omega()),
                "star({other}) must widen to the top"
            );
        }
    }

    /// **The theorem `star`'s zero test rests on.**
    ///
    /// `Const(0)` is the *only* `Bound` that denotes `0` at every valuation.
    /// Every other closed term is some other `Const`; and every term
    /// containing a variable denotes `omega` at the all-`omega` valuation,
    /// because every constructor propagates `omega` upwards - `Sum`, `Max` and
    /// `Prod` through [`Nat`], `Pow` through the `MAX_FINITE_EXPONENT` short
    /// circuit, and `Log` through `ceil_log(omega) = omega`.
    ///
    /// That is what makes the syntactic test *exactly* the denotational one,
    /// and it is checked here on terms that are `0` at some valuations - the
    /// only ones that could have been confused with `Const(0)`.
    #[test]
    fn const_zero_is_the_only_bound_denoting_zero_everywhere() {
        let everywhere = |bound: &Bound| {
            B::valuations()
                .iter()
                .all(|point| bound.eval(point) == Nat::ZERO)
        };
        assert!(everywhere(&Bound::zero()));

        // Each of these denotes `0` at a valuation, and none of them may be
        // mistaken for the everywhere-zero element.
        for near_miss in [
            Bound::prod([Bound::zero(), Bound::var("x")]),
            Bound::prod([Bound::zero(), Bound::var("x"), Bound::var("y")]),
            Bound::var("x"),
            Bound::log(Base::TWO, Bound::var("x")),
            Bound::max_of([Bound::var("x"), Bound::var("y")]),
        ] {
            let zero_somewhere = B::valuations()
                .iter()
                .any(|point| near_miss.eval(point) == Nat::ZERO);
            assert!(
                zero_somewhere,
                "{near_miss} was meant to denote zero somewhere"
            );
            assert!(
                !everywhere(&near_miss),
                "{near_miss} must not denote zero at the all-omega valuation"
            );
            assert_ne!(near_miss.kind(), &BoundKind::Const(Nat::ZERO));
        }
    }

    /// **`star` is a function of the denotation.**
    ///
    /// Two carrier values that agree at every valuation must have `star`s that
    /// agree at every valuation. Checked exhaustively over the grid, and over
    /// explicit pairs that are *structurally distinct but extensionally
    /// equal* - which the grid, being canonical, does not otherwise contain.
    #[test]
    fn star_agrees_on_extensionally_equal_elements() {
        let points = B::valuations();
        let agrees = |left: &Lifted<Bound>, right: &Lifted<Bound>| {
            points
                .iter()
                .all(|point| B::denote(left, point) == B::denote(right, point))
        };

        let grid = B::grid();
        for left in &grid {
            for right in &grid {
                if agrees(left, right) {
                    assert!(
                        agrees(&B::star(left), &B::star(right)),
                        "star disagrees on extensionally equal {left:?} and {right:?}"
                    );
                }
            }
        }

        // Structurally distinct, extensionally equal, and not in the grid.
        let witnesses = [
            (
                elem(Bound::prod([Bound::zero(), Bound::var("x")])),
                elem(Bound::prod([Bound::zero(), Bound::var("y")])),
            ),
            (
                elem(Bound::max_of([
                    Bound::var("x"),
                    Bound::sum([Bound::var("x"), Bound::one()]),
                ])),
                elem(Bound::sum([Bound::var("x"), Bound::one()])),
            ),
        ];
        for (left, right) in &witnesses {
            assert_ne!(left, right, "the witness pair must be distinct terms");
            assert!(
                agrees(left, right),
                "the witness pair must be extensionally equal: {left:?} vs {right:?}"
            );
            assert_eq!(
                B::star(left),
                B::star(right),
                "star must not depend on the spelling"
            );
        }
    }

    /// **`star` is sound pointwise.** At every valuation, `star(a)` denotes at
    /// least the true Kleene closure of what `a` denotes there:
    /// `1` when `a` is `0` or unreachable, `omega` otherwise.
    #[test]
    fn star_over_approximates_the_true_closure_at_every_valuation() {
        for element in B::grid() {
            let starred = B::star(&element);
            for point in B::valuations() {
                let exact = match B::denote(&element, &point) {
                    Lifted::Bottom | Lifted::Elem(Nat::Fin(0)) => Lifted::Elem(Nat::ONE),
                    Lifted::Elem(_) => Lifted::Elem(Nat::OMEGA),
                };
                let reported = B::denote(&starred, &point);
                assert!(
                    reported >= exact,
                    "star({element:?}) denotes {reported:?} where the closure is {exact:?}"
                );
            }
        }
    }

    /// **The premise `star`'s doc comment used to rest on is false, and the
    /// conclusion survives anyway.**
    ///
    /// `prod([prod([2^40, 2^40]), 0])` and `prod([0, 2^40, 2^40])` are both
    /// closed and both have ideal value `0`, and they fold to two different
    /// constants: the inner subgroup saturates before the zero is ever in
    /// scope, and `omega` absorbs unconditionally from there on.
    /// `adversary::a_saturating_subgroup_still_defeats_an_enclosing_zero` pins
    /// the algebra; this pins what it means for `star`.
    ///
    /// It is **not** a determinism hazard. The two recipes build two different
    /// carrier values with two different *denotations*, `0` and `omega`, so
    /// `star` giving two answers is `star` agreeing with the denotation. What
    /// is lost is tightness, in the direction this crate always errs in.
    #[test]
    fn a_saturating_subgroup_costs_tightness_in_star_but_not_determinism() {
        let big = 1u64 << 40;
        let exact = Bound::prod([Bound::zero(), Bound::constant(big), Bound::constant(big)]);
        let saturated = Bound::prod([
            Bound::prod([Bound::constant(big), Bound::constant(big)]),
            Bound::zero(),
        ]);

        assert_eq!(exact.kind(), &BoundKind::Const(Nat::ZERO));
        assert_eq!(saturated.kind(), &BoundKind::Const(Nat::OMEGA));

        // Two spellings of one *ideal* value, but two carrier values with two
        // denotations - so `star` is not being asked the same question twice.
        let point = at(Nat::Fin(3), Nat::Fin(5), Nat::ZERO);
        assert_eq!(exact.eval(&point), Nat::ZERO);
        assert_eq!(saturated.eval(&point), Nat::OMEGA);

        assert_eq!(
            B::star(&elem(exact)),
            B::one(),
            "the exact spelling is tight"
        );
        assert_eq!(
            B::star(&elem(saturated)),
            elem(Bound::omega()),
            "the saturated spelling is sound and loose"
        );
    }

    // ---- the boundaries of the exact regime, which fix the grid's constants
    // ----

    /// **Why the grid's largest literal is `2^31` and not `2^40`.**
    ///
    /// `times` is associative as an *equation* only while no literal product
    /// saturates next to a zero. Here is the smallest failure: the saturated
    /// `Const(omega)` absorbs the zero, where the other grouping folds the
    /// zero first and is exact. Both answers are sound, because `omega` is
    /// above `0`, so this is a tightness boundary rather than an unsoundness;
    /// but it is a genuine failure of the equation, so the grid stays inside
    /// it.
    #[test]
    fn a_saturating_literal_product_breaks_times_associativity() {
        let big = elem(Bound::constant(1u64 << 40));
        let zero_cost = elem(Bound::zero());

        let left = B::times(&B::times(&big, &big), &zero_cost);
        let right = B::times(&big, &B::times(&big, &zero_cost));
        assert_eq!(left, elem(Bound::omega()));
        assert_eq!(right, elem(Bound::zero()));
        assert_ne!(left, right, "the equation fails once a subgroup saturates");

        // And the grid's own literal does not reach it: `2^31` squared is
        // `2^62`, which is still inside `u64`.
        let safe = elem(Bound::constant(1u64 << 31));
        assert_eq!(
            B::times(&B::times(&safe, &safe), &zero_cost),
            B::times(&safe, &B::times(&safe, &zero_cost))
        );
    }

    /// **Why no two finite grid denotations may sum past `u64`.**
    ///
    /// `times` distributes over `plus` as an equation except when the left
    /// factor denotes `0` and the sum saturates: `0 * omega` is `omega` by the
    /// frozen `Nat::times` rule, while `0*b + 0*c` is `0`. Sound in the same
    /// direction, and the same kind of boundary.
    #[test]
    fn a_saturating_sum_breaks_distributivity_under_a_zero_factor() {
        let half = elem(Bound::constant(1u64 << 63));
        let zero_cost = elem(Bound::zero());

        let left = B::times(&zero_cost, &B::plus(&half, &half));
        let right = B::plus(&B::times(&zero_cost, &half), &B::times(&zero_cost, &half));
        assert_eq!(left, elem(Bound::omega()));
        assert_eq!(right, elem(Bound::zero()));
        assert_ne!(left, right);

        // The grid's largest literal is far below the boundary.
        let safe = elem(Bound::constant(1u64 << 31));
        assert_eq!(
            B::times(&zero_cost, &B::plus(&safe, &safe)),
            B::plus(&B::times(&zero_cost, &safe), &B::times(&zero_cost, &safe))
        );
    }

    // ---- the grids ----

    /// The element grid must reach every fragment the law suite needs, or the
    /// laws pass on a subset nobody chose. Each obligation is checked
    /// separately so that a grid losing one of them names which.
    #[test]
    fn the_element_grid_meets_every_documented_obligation() {
        let grid = B::grid();
        let shapes: Vec<_> = grid
            .iter()
            .filter_map(|element| element.as_elem().map(Bound::shape))
            .collect();

        assert!(grid.contains(&Lifted::Bottom), "zero");
        assert!(grid.contains(&elem(Bound::zero())), "the other zero");
        assert!(grid.contains(&B::one()), "one");
        assert!(grid.contains(&elem(Bound::omega())), "the top");
        assert!(
            grid.contains(&elem(Bound::constant(1u64 << 31))),
            "a large finite literal"
        );
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

        // L11's required non-idempotence witness.
        assert!(
            grid.iter()
                .any(|element| &B::plus(element, element) != element),
            "a witness for PLUS_IDEMPOTENT = false"
        );

        // Deterministic: identical on every call, in the same order.
        assert_eq!(B::grid(), grid);
    }

    /// The valuation grid must reach the all-zero, all-one, large-finite and
    /// all-`omega` points; the third is where saturation lives and the fourth
    /// is what makes `Const(0)` the unique everywhere-zero element.
    #[test]
    fn the_valuation_grid_meets_every_documented_obligation() {
        let points = B::valuations();
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
        assert_eq!(B::valuations(), points, "deterministic");
    }

    /// **The grid stays inside the algebra's exact regime.** Both boundaries
    /// pinned above are avoided by construction, not by luck: no pair of
    /// finite grid denotations sums past `u64`, and no pair of grid literals
    /// multiplies past it.
    #[test]
    fn the_grid_stays_inside_the_exact_regime() {
        let grid = B::grid();
        let points = B::valuations();

        for left in &grid {
            for right in &grid {
                for point in &points {
                    let (a, b) = (B::denote(left, point), B::denote(right, point));
                    if let (Lifted::Elem(Nat::Fin(a)), Lifted::Elem(Nat::Fin(b))) = (a, b) {
                        assert!(
                            a.checked_add(b).is_some(),
                            "{a} + {b} saturates, which breaks L3 under a zero factor"
                        );
                    }
                }
            }
        }

        let literals: Vec<u64> = grid
            .iter()
            .filter_map(Lifted::as_elem)
            .filter_map(|bound| match bound.kind() {
                BoundKind::Const(Nat::Fin(value)) => Some(*value),
                _ => None,
            })
            .collect();
        assert!(literals.len() >= 4, "the grid must carry several literals");
        for a in &literals {
            for b in &literals {
                assert!(
                    a.checked_mul(*b).is_some(),
                    "{a} * {b} saturates, which breaks L2 next to a zero"
                );
            }
        }
    }

    /// `denote` is `eval`, lifted. Nothing else: a denotation that folded
    /// `Bottom` into `Elem(0)` would make L6 and L8 pass on an unsound model.
    #[test]
    fn denote_is_eval_with_bottom_carried_through() {
        let point = at(Nat::Fin(3), Nat::Fin(5), Nat::ZERO);
        assert_eq!(B::denote(&Lifted::Bottom, &point), Lifted::Bottom);
        assert_ne!(
            B::denote(&Lifted::Bottom, &point),
            B::denote(&elem(Bound::zero()), &point),
            "Bottom and Const(0) are meaning-critical zeros with opposite provenance"
        );
        for bound in [
            Bound::zero(),
            Bound::constant(7),
            Bound::omega(),
            Bound::var("x"),
            Bound::sum([Bound::var("x"), Bound::var("y")]),
        ] {
            assert_eq!(
                B::denote(&elem(bound.clone()), &point),
                Lifted::Elem(bound.eval(&point))
            );
        }
    }

    /// The whole suite, for this instance. AC4 runs it from the registry too;
    /// this is the direct call, so a failure names `B` rather than "the first
    /// registered resource".
    #[test]
    fn b_satisfies_every_law() {
        let outcome = crate::dioid_laws::check_dioid_laws::<B>();
        assert!(outcome.is_ok(), "B violates a law: {outcome:?}");
    }
}
