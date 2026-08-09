//! [`Dioid`] - the cost semiring the propagation engine is generic over.

use core::{fmt::Debug, hash::Hash};

use crate::{canonical::Canonical, semiring_id::SemiringId};

/// A closed, naturally ordered cost semiring with an antisymmetric canonical
/// order: a **dioid in the sense of Gondran-Minoux, plus a Kleene closure**.
///
/// That qualification matters. Under the more common definition
/// *dioid = idempotent semiring*, `(N u {omega}, +, *)` is not a dioid at all,
/// because `x + x = 2x`. Gondran-Minoux instead define a dioid as a semiring
/// whose canonical preorder `a <= b iff exists c. a (+) c = b` is
/// **antisymmetric**; under that definition `(N u {omega}, +, *)` is the
/// canonical *cancellative* dioid and idempotent dioids are a different
/// subclass. Both registered instances are dioids in that sense.
///
/// Gondran-Minoux's signature has no `star`; requiring one makes this a
/// *closed* dioid. Hence the name and this paragraph.
///
/// Peak live memory is a **different semiring over the same engine**, not a
/// second engine. Additive resources instantiate `(+, *)`; peak memory
/// instantiates `(max, +)`.
///
/// # The five operations
///
/// | op | meaning |
/// |---|---|
/// | [`Dioid::zero`]  | additive identity - the cost of a path no execution reaches, and the only sanctioned fixpoint seed |
/// | [`Dioid::one`]   | multiplicative identity - the cost of a no-op step |
/// | [`Dioid::plus`]  | join of alternatives (branch) |
/// | [`Dioid::times`] | sequential composition |
/// | [`Dioid::star`]  | unbounded iteration (Kleene closure) |
///
/// # The laws
///
/// L1-L11 below are the **single authoritative numbering**; earlier drafts
/// numbered them inconsistently across documents, which would have shipped one
/// law implemented twice and another not at all.
/// `check_dioid_laws::<Self>()` tests all of them, and the registry macro
/// emits that call for every registered instance, so a future instance that
/// violates a law fails at **test** time rather than at analysis time.
///
/// Throughout, `a <= b` denotes the **canonical preorder**
/// `exists c. plus(a, c) == b`, and `==` denotes **extensional** equality -
/// see "What equality means" below.
///
/// * **L1** `plus` is associative and commutative, with identity `zero`.
/// * **L2** `times` is associative, with identity `one`.
/// * **L3** `times` distributes over `plus` on both sides.
/// * **L4** *(annihilation)* `times(zero, a) == zero == times(a, zero)`.
/// * **L5** *(star unfolding, an **equation**)*
///   `star(a) == plus(one, times(a, star(a)))` and
///   `star(a) == plus(one, times(star(a), a))`.
///
///   This is an equation, not the inequation `star(a) >= ...`. The inequation
///   is **vacuous**: `omega` is the top of the canonical order, so an
///   implementation that returns `omega` unconditionally satisfies it for
///   every input, and a mutant that drops the zero case survives with no law
///   to blame it on. The equation was checked against both shipped instances
///   and holds identically.
/// * **L6** *(zero-sum-freeness)* `plus(a, b) == zero` implies `a == zero` and
///   `b == zero`.
/// * **L7** *(antisymmetry of the canonical order)* for all `a`, `c`, `d`: let
///   `b = plus(a, c)`; if `plus(b, d) == a` then `a == b`.
///
///   **This is the law that defines the trait**, and it does not follow from
///   L6. Zero-sum-freeness does *not* imply antisymmetry: the quotient
///   `S = N / (n ~ n+2 for n >= 1)` is a legitimate commutative semiring,
///   is zero-sum-free, and has `e <= f` and `f <= e` with `e != f`. Without
///   L7 a future "parity of allocations" or "mod-k counting" carrier passes
///   every other law and is not a dioid.
/// * **L8** *(non-degeneracy)* `zero() != one()`.
///
///   The one-element semiring (`Carrier = ()`, `zero = one = ()`) satisfies
///   every other law, including L6 vacuously, and reports every program as
///   costing nothing. One line of test closes it.
/// * **L9** *(star at zero)* `star(zero) == one`. Kills the
///   returns-`omega`-unconditionally mutant that L5 alone lets through.
/// * **L10** *(star monotonicity)* `a <= b` implies `star(a) <= star(b)`.
///
///   Follows from nothing else. `times`-monotonicity is a theorem of L3 and
///   `plus`-monotonicity a theorem of L1, but an instance with
///   `star(one) == omega` and `star(two) == one` violates no other law and
///   makes loop composition unsound.
/// * **L11** *(idempotence, both directions)* `plus(a, a) == a` for every `a`
///   **iff** [`Dioid::PLUS_IDEMPOTENT`]. When the flag is `false` the suite
///   additionally requires a **witness** `a` with `plus(a, a) != a`, so the
///   flag cannot be wrong in either direction.
///
/// # Why a `const`, not an `IdempotentDioid` marker trait
///
/// Both can be set wrong. Only the marker can be *forgotten*: a missing
/// associated const is a compile error, whereas a missing `impl
/// IdempotentDioid for X {}` compiles silently and the idempotence test is
/// simply never emitted. And a marker declares idempotence without checking
/// it - `impl IdempotentDioid for B {}` compiles today while `1 + 1 != 1`,
/// with nothing running. The const plus L11's two-directional check makes the
/// property *mechanically tested* rather than declared.
///
/// # What equality means in the law suite
///
/// **Extensional, never `PartialEq`.** Structural equality fails L3 on the
/// first symbolic input: `x * (1 + 1)` is `Prod[Const(2), Var(x)]` while
/// `x*1 + x*1` is `Sum[Var(x), Var(x)]`. These are different values of the
/// same type, and no future normaliser rescues it - making L3 hold
/// structurally requires an expanded-polynomial normal form, which is
/// incompatible with rendering `x1 * (2 + log2(x1))` in factored form.
///
/// The suite therefore compares **denotations over a fixed valuation grid**;
/// see `DioidLaws`, in the `dioid_laws` module, which is gated behind
/// `cfg(any(test, feature = "laws"))` and so is absent from a default doc
/// build. Build with `--features laws` to see it.
///
/// # Not object safe, deliberately
///
/// No `&self`, an associated const and an associated type: instances are
/// uninhabited witness types, so there is no instance state to construct
/// wrongly. A `dyn Dioid<Carrier = Bound>` registry would hard-block any
/// future analysis whose carrier is not [`crate::Bound`].
pub trait Dioid {
    /// The carrier.
    ///
    /// An associated type rather than [`crate::Bound`], because a future
    /// instance's carrier need not be a `Bound`. Both instances registered in
    /// M0 use `Lifted<Bound>`, so the "two carriers" plumbing tax the panel
    /// feared does not arise today.
    ///
    /// Note the bound is [`Canonical`], **not `Ord`**: requiring `Ord` here
    /// is what forced `Bound: Ord` and made
    /// `fn plus(a, b) { a.max(b).clone() }` compile into something that
    /// returns `Var(x)` in preference to `Const(omega)`.
    type Carrier: Clone + Debug + Eq + Hash + Canonical;

    /// The algebra's identity. Present on the trait so a semiring cannot be
    /// defined without one. **Not a cache key** - see [`crate::ResourceId`].
    const SEMIRING: SemiringId;

    /// Whether `plus` is idempotent. Checked in both directions by L11.
    const PLUS_IDEMPOTENT: bool;

    /// Additive identity: the cost of a path no execution reaches, and the
    /// only sanctioned fixpoint seed.
    fn zero() -> Self::Carrier;

    /// Multiplicative identity: the cost of a no-op step.
    fn one() -> Self::Carrier;

    /// Join of alternatives.
    fn plus(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier;

    /// Sequential composition.
    fn times(a: &Self::Carrier, b: &Self::Carrier) -> Self::Carrier;

    /// Unbounded iteration: `one (+) a (+) a(*)a (+) ...`.
    ///
    /// Must be **total and sound**, not tight. Deciding "is this symbolic
    /// bound zero" is undecidable in general, and over-approximating to the
    /// top of the lattice is always sound. Tightness for counted loops comes
    /// from `times(loop_bound, body)`, never from `star`.
    fn star(a: &Self::Carrier) -> Self::Carrier;
}
