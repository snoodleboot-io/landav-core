//! [`Lifted`] - `T` with a bottom element adjoined.

use crate::canonical::Canonical;

/// `T` with a bottom element adjoined.
///
/// # Why *both* semirings use this carrier
///
/// `Bottom` is the cost of a path **no execution reaches**, and it is also the
/// only sanctioned fixpoint seed. It is the additive identity and the
/// multiplicative annihilator of every semiring in this crate.
///
/// `MaxPlus` needs it because `(max, +)` over `N u {omega}` has no
/// annihilator: `max`'s identity and `+`'s identity are both `0`, so `zero`
/// fails to annihilate `times` and the structure is not a lawful semiring at
/// all.
///
/// `B` needs it for a different and more dangerous reason. `B`'s annihilator
/// would otherwise be `Const(0)`, which is *also* a legitimate, common,
/// meaningful cost - so one value would mean three things at once:
/// "infeasible path", "fixpoint seed, not yet computed", and "proved to cost
/// nothing". Under that conflation, round zero of a fixpoint over a loop with
/// an unanalysed body computes `zero * omega`, the annihilation law forces
/// that to `0`, the result is `omega`-free, and the derivation is published as
/// *proved to cost exactly nothing* with the blame closure never invoked.
///
/// Giving `B` the same lifted carrier dissolves that at the root, and it has
/// two further consequences:
///
/// * `0 * omega = 0` **stops being forced** by the annihilation law, so
///   [`crate::Nat::times`] can let `omega` absorb unconditionally;
/// * the two-carrier plumbing tax disappears - both registered semirings have
///   the same carrier, so a helper written for one is reusable for the other.
///
/// `Bottom` and `Const(0)` are both meaning-critical zeros with **opposite**
/// provenance. Neither may ever be a placeholder.
///
/// # Ordering
///
/// `PartialOrd`/`Ord` are derived, which means they exist for
/// `Lifted<Nat>` - where the order is the semantic magnitude order with
/// unreachable at the bottom, and the law suite needs it - and do **not**
/// exist for `Lifted<Bound>`, because [`crate::Bound`] has no `Ord`. That
/// asymmetry is the point: it is what makes an idiomatic
/// `fn plus(a, b) { a.max(b) }` for `MaxPlus` a compile error rather than a
/// peak-memory bound that silently discards the unbounded branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lifted<T> {
    /// Unreachable: no execution reaches here. The additive identity and the
    /// multiplicative annihilator.
    Bottom,
    /// A reachable cost.
    Elem(T),
}

impl<T> Lifted<T> {
    /// `true` iff this is [`Lifted::Bottom`].
    #[must_use]
    pub const fn is_bottom(&self) -> bool {
        todo!()
    }

    /// The element, if reachable.
    #[must_use]
    pub const fn as_elem(&self) -> Option<&T> {
        todo!()
    }
}

impl<T: Canonical> Canonical for Lifted<T> {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        todo!()
    }

    fn write_canonical(&self, out: &mut Vec<u8>) {
        todo!()
    }
}

#[cfg(test)]
mod frozen_ord_asymmetry {
    use super::Lifted;
    use crate::nat::Nat;

    const fn requires_ord<T: Ord>() {}

    /// `Lifted<Nat>` **is** `Ord`: the law suite compares denotations with it.
    ///
    /// The dual fact - that `Lifted<Bound>` is **not** `Ord`, so
    /// `MaxPlus::plus` cannot be written `a.max(b)` - is pinned by the
    /// `compile_fail` doctest on [`crate::Bound`]. Between them the asymmetry
    /// is a compiled fact in both directions rather than a comment.
    const _: () = requires_ord::<Lifted<Nat>>();
}
