//! [`Canonical`] - the deterministic total order and byte form.

use core::cmp::Ordering;

/// A total, content-derived, deterministic order and byte encoding.
///
/// # Why this exists rather than `Ord`
///
/// [`crate::Bound`] must have a total order: n-ary operands are held sorted so
/// that associativity and commutativity are definitional rather than rewrites,
/// and e-graph extraction needs a tie-break that resolves to a unique winner.
///
/// [`crate::Bound`] must **not** have `Ord`, because `b1 < b2` on a symbolic
/// cost expression reads as "b1 is tighter" - semantic domination, which is
/// F-018 and which this crate does not decide. Writing `a.max(b)` on two
/// bounds compiles, passes clippy, satisfies associativity, commutativity and
/// idempotence, and silently returns `Var(x)` in preference to `Const(omega)`
/// because `Const` is the first variant.
///
/// Both requirements hold at once because the order lives here, under a name
/// that cannot be misread, and is unreachable through `<`, `>`, `.max()`,
/// `.min()`, `.sort()`, `BTreeMap` or any other `Ord`-driven API.
///
/// # Contract
///
/// * `canonical_cmp` is a **total order** and agrees with `Eq`: it returns
///   [`Ordering::Equal`] exactly when the values are equal. No two distinct
///   values tie, so an unstable sort is safe and extraction has a unique
///   winner.
/// * It is **content derived**. It may never depend on an address, an
///   allocation, an interner index, a hash seed, a wall clock, or Rust
///   *declaration* order. A future hash-cons may cache the result of a
///   comparison but may not define it.
/// * It is **never `derive`d** on a type whose variants could be reordered:
///   see [`crate::BoundShape::canonical_tag`].
/// * `write_canonical` produces a version-tagged, length-prefixed,
///   self-delimiting encoding that is independent of serde and of `Hash`.
///   Equal values produce equal bytes and distinct values produce distinct
///   bytes.
/// * Both are pinned by [`crate::NORMAL_FORM_VERSION`].
pub trait Canonical: Eq {
    /// The canonical total order. See the trait contract.
    fn canonical_cmp(&self, other: &Self) -> Ordering;

    /// Appends this value's canonical encoding to `out`.
    ///
    /// Implementations must be iterative or depth-bounded; see
    /// [`crate::MAX_DEPTH`].
    fn write_canonical(&self, out: &mut Vec<u8>);
}
