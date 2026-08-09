//! [`VarSet`] - the conservative free-variable summary on every node.

use crate::var_id::VarId;

/// A 64-bit conservative summary of the free variables of a subterm.
///
/// Used by [`crate::Bound::subst`] to skip whole subtrees in O(1), which is
/// the difference between O(size) and O(touched) per fixpoint round.
///
/// # The two false-negative hazards, and how both are closed
///
/// A false positive costs work. A **false negative is a soundness bug**:
/// substitution would skip a subtree that needed rewriting, leaving a stale
/// free variable in a term the pipeline then reports as a closed-form bound.
///
/// 1. **Unstable hash.** The bit index must be identical at insert time and at
///    query time, in every process and on every toolchain. This type therefore
///    uses **FNV-1a with hardcoded constants**, never
///    `DefaultHasher`/`RandomState` (per-process seed, and an algorithm std
///    explicitly does not guarantee across releases).
/// 2. **Stale cache after a rebuild.** A `VarSet` is *never* accepted as a
///    constructor parameter. It is computed by the node constructor as the
///    union of its children's sets, and there is no public path that builds a
///    node without going through that constructor - the matchable
///    [`crate::BoundKind`] cannot be lifted back into a [`crate::Bound`].
///
/// The two defences are independent: the set is additionally **not carried on
/// the wire** (see [`crate::BoundWire`]), so a deserialised bound recomputes
/// it from scratch even if a future change broke defence 1.
///
/// It is also excluded from `PartialEq`, `Hash` and
/// [`crate::Canonical::canonical_cmp`]: it is derived data, and letting it
/// into the canonical key would make the child order of every `Sum` a function
/// of the hasher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VarSet(u64);

impl VarSet {
    /// No variables.
    pub const EMPTY: Self = Self(0);

    /// The FNV-1a 64-bit offset basis. Hardcoded, and part of the frozen
    /// contract: changing it changes nothing observable, but only because the
    /// set never crosses a process boundary. Keep it fixed anyway.
    pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    /// The FNV-1a 64-bit prime.
    pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    /// The set containing exactly `var`.
    #[must_use]
    pub fn singleton(var: &VarId) -> Self {
        // FNV-1a with the hardcoded constants above: identical at insert time
        // and at query time, in every process and on every toolchain.
        let mut hash = Self::FNV_OFFSET_BASIS;
        for byte in var.symbol().as_str().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(Self::FNV_PRIME);
        }
        Self(1u64 << (hash % 64))
    }

    /// The union of two sets. Monotone: a union never clears a bit, which is
    /// what makes the filter a pure superset and false negatives impossible
    /// given a stable hash.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `false` guarantees `var` does **not** occur. `true` means it may.
    #[must_use]
    pub fn may_contain(self, var: &VarId) -> bool {
        self.0 & Self::singleton(var).0 != 0
    }

    /// `true` iff no variable can occur.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}
