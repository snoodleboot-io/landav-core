//! [`Substitution`] - simultaneous, single-pass composition of bounds.
//!
//! [`crate::Bound::subst`] replaces one variable. This module is the operation
//! a *caller* actually performs: a whole map of variables replaced at once,
//! and the composition of two such maps.
//!
//! # Why simultaneous, and not a fold of the single-variable form
//!
//! Folding [`crate::Bound::subst`] over a map is **sequential**, and sequential
//! substitution is a different function. `{x := y, y := z}` applied to `x`
//! gives `y` simultaneously and `z` sequentially, so the answer would depend on
//! the iteration order of the map - and the answer callers want is the
//! simultaneous one, because the bindings all describe the *same* moment in the
//! program.
//!
//! Single pass falls out of the same rule: an image is spliced in and never
//! re-scanned, so `x := x + 1` terminates and produces `x + 1`. A re-scanning
//! substitution has no fixpoint there at all.
//!
//! # An unbound variable is not a failure
//!
//! A variable with no binding stays free. Reporting it as an error reads
//! tidier and is worse: the only thing a caller can do with that error is
//! `unwrap_or(omega)`, which is a sound bound with the blame thrown away, and
//! blame is the whole recovery path (F-015). A partially substituted bound is
//! still a bound, and the free variables are still visible through
//! [`crate::Bound::vars`].
//!
//! # Substitution grows terms, so it meets the budgets
//!
//! Replacing a leaf by a term adds that term's depth, and replacing an operand
//! of a `Sum` by another `Sum` **flattens** - which grows the operand list
//! while the depth and the DAG stay put, the failure mode
//! [`crate::MAX_DEPTH`] cannot see. Both budgets are enforced by the smart
//! constructors this module rebuilds through, so they are enforced here on
//! exactly the same terms:
//!
//! * [`Substitution::apply`] and [`Substitution::then`] are **total** and widen
//!   to `omega`, which is sound and monotone;
//! * [`Substitution::apply_checked`] and [`Substitution::then_checked`] report
//!   the budget instead, for callers that would rather attach blame than
//!   silently lose tightness.

use std::collections::{BTreeMap, HashMap};

use crate::{
    bound::Bound, bound_error::BoundError, bound_kind::BoundKind, trans_kind::TransKind,
    var_id::VarId,
};

/// What to do when a smart constructor cannot build the node it was asked for.
///
/// The two answers are both correct and they serve different callers, so the
/// choice is a parameter of one traversal rather than two traversals that can
/// drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnRefusal {
    /// Widen to `omega`. Sound, monotone, and total.
    Widen,
    /// Report the budget that was exceeded.
    Blame,
}

/// A **simultaneous** substitution: a finite map from variables to bounds.
///
/// # The guarantee
///
/// `Bound` is closed under this operation. [`Substitution::apply`] takes a
/// [`Bound`] and returns a [`Bound`], and [`Substitution::then`] takes two
/// substitutions and returns one - not a `Result`, not an `Option`. Every
/// image is itself a `Bound`, so the weak monotonicity every `Bound` carries
/// is preserved: a composition of monotone functions is monotone, which is
/// what makes composition-by-substitution *sound*.
///
/// It is not what makes it *tight*, and this type does not claim tightness.
/// Whether an image over-approximates the variable it replaces is a semantic
/// obligation on the caller about the relationship between the bound and the
/// program, and no Rust type can discharge it.
///
/// # Composition is sound, never exact
///
/// `first.then(&second)` is **not** required to agree, term for term, with
/// applying `first` and then `second`. Composing builds each image in
/// isolation, where nothing in the enclosing term can rescue an overflow;
/// applying twice keeps the whole term in scope through both passes. Since
/// saturating multiplication is not associative on `N u {omega}`, the two can
/// land on different terms, and both over-approximate the same denotation:
///
/// ```text
/// b = 0 * x * z      first = { x := 2^40 * y }      second = { y := 2^40 }
///
/// first.then(&second).apply(b)      x := 2^40 * 2^40 = omega  ->  omega
/// second.apply(&first.apply(b))     0 * y * z, then 0 * z     ->  0
/// ```
///
/// Both are sound; the second is tighter. Nothing here promises which one a
/// caller gets, only that whichever it gets is a bound.
///
/// # Ordering
///
/// The bindings are held in a [`BTreeMap`], so [`Substitution::domain`] and
/// the order images are composed in are functions of the variable names rather
/// than of a hash seed. A substitution that reordered itself between two runs
/// of the same binary would make composition non-deterministic, and the
/// canonical form the F-008 cache keys on rests on determinism.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Substitution {
    /// The bindings, keyed by the variable each one replaces.
    bindings: BTreeMap<VarId, Bound>,
}

impl Substitution {
    /// The empty substitution, which is the identity function on bounds.
    ///
    /// Unlike [`Bound`], this type does have a meaningful neutral element, and
    /// it is neutral for both operations: `s.then(&identity)` and
    /// `identity.then(&s)` are both `s`.
    #[must_use]
    pub fn identity() -> Self {
        Self {
            bindings: BTreeMap::new(),
        }
    }

    /// The one-binding substitution, which agrees exactly with
    /// [`crate::Bound::subst`].
    #[must_use]
    pub fn of(var: VarId, replacement: Bound) -> Self {
        let mut bindings = BTreeMap::new();
        bindings.insert(var, replacement);
        Self { bindings }
    }

    /// Collects bindings into one simultaneous substitution.
    ///
    /// A repeated variable takes its **last** binding, matching every other
    /// map-building API in the standard library. The bindings do not see each
    /// other: this is one map applied in one pass, not a sequence.
    #[must_use]
    pub fn from_bindings(bindings: impl IntoIterator<Item = (VarId, Bound)>) -> Self {
        Self {
            bindings: bindings.into_iter().collect(),
        }
    }

    /// The image of `var`, or `None` if it is not bound - in which case it
    /// stays free.
    #[must_use]
    pub fn get(&self, var: &VarId) -> Option<&Bound> {
        self.bindings.get(var)
    }

    /// Every bound variable, **sorted ascending** by [`VarId`].
    ///
    /// Sorted rather than merely deterministic: this reaches diagnostics, and
    /// a set whose iteration order differs between two runs of one binary
    /// produces a CI log diff that means nothing.
    #[must_use]
    pub fn domain(&self) -> Vec<VarId> {
        self.bindings.keys().cloned().collect()
    }

    /// How many variables are bound.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// `true` iff this is the identity substitution.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// `false` **guarantees** that applying this substitution to `bound`
    /// returns `bound`; `true` means it may rewrite something. O(domain).
    ///
    /// Built on [`crate::Bound::may_contain_var`], whose summary is allowed to
    /// over-approximate but never to produce a false negative - a false
    /// negative here would skip a subtree that needed rewriting and leave a
    /// stale free variable in a term reported as composed.
    #[must_use]
    pub fn may_rewrite(&self, bound: &Bound) -> bool {
        self.bindings.keys().any(|var| bound.may_contain_var(var))
    }

    /// Replaces every bound variable in `bound`, simultaneously and in one
    /// pass.
    ///
    /// **Total**, and closed: a `Bound` in, a `Bound` out. A term that would
    /// exceed [`crate::MAX_DEPTH`] or the operand budget widens to `omega`,
    /// which is sound and monotone. Use [`Substitution::apply_checked`] to
    /// have the budget reported instead.
    ///
    /// Untouched subtrees come back **by handle**, gated on
    /// [`Substitution::may_rewrite`], so a fixpoint round costs O(touched)
    /// rather than O(size).
    #[must_use]
    pub fn apply(&self, bound: &Bound) -> Bound {
        match self.rewrite(bound, OnRefusal::Widen) {
            Ok(rewritten) => rewritten,
            // Unreachable: in `Widen` mode every constructor call goes through
            // a total constructor, which has no error channel. `omega` is the
            // sound answer regardless, and it is cheaper than an invariant
            // nobody can check.
            Err(_) => Bound::omega(),
        }
    }

    /// As [`Substitution::apply`], but reports the budget instead of widening.
    ///
    /// An unbound variable is **not** an error here either. The only failures
    /// are the ones the smart constructors raise.
    ///
    /// # Errors
    ///
    /// [`BoundError::DepthExceeded`] if an image makes the term deeper than
    /// [`crate::MAX_DEPTH`], or [`BoundError::ArityExceeded`] /
    /// [`BoundError::NodeBudgetExceeded`] if flattening an image into its
    /// parent produces more operands than [`crate::MAX_NODES`] allows nodes.
    pub fn apply_checked(&self, bound: &Bound) -> Result<Bound, BoundError> {
        self.rewrite(bound, OnRefusal::Blame)
    }

    /// The substitution that applies `self` and then `next`, in one pass.
    ///
    /// Each of `self`'s images has `next` applied to it, and any variable
    /// `next` binds that `self` does not is carried over unchanged. A variable
    /// bound by both takes `self`'s composed image: `self` acts first, so
    /// `next`'s binding for that variable is never reached.
    ///
    /// **Total**, and closed: two substitutions in, one substitution out. See
    /// the type's documentation for why this is sound but not equal to
    /// applying the two in sequence.
    #[must_use]
    pub fn then(&self, next: &Self) -> Self {
        let mut bindings = BTreeMap::new();
        for (var, image) in &self.bindings {
            bindings.insert(var.clone(), next.apply(image));
        }
        Self {
            bindings: carry_over(bindings, next),
        }
    }

    /// As [`Substitution::then`], but reports the budget instead of widening.
    ///
    /// Composition builds each image by substituting into it, so it meets the
    /// same budgets [`Substitution::apply`] does and needs the same blame
    /// channel: without one, a caller composing two substitutions loses the
    /// reason its bound became `omega`.
    ///
    /// # Errors
    ///
    /// Whatever [`Substitution::apply_checked`] raises for the first of
    /// `self`'s images that cannot be rebuilt.
    pub fn then_checked(&self, next: &Self) -> Result<Self, BoundError> {
        let mut bindings = BTreeMap::new();
        for (var, image) in &self.bindings {
            bindings.insert(var.clone(), next.apply_checked(image)?);
        }
        Ok(Self {
            bindings: carry_over(bindings, next),
        })
    }

    /// The shared traversal behind both `apply` forms.
    ///
    /// An explicit worklist rather than recursion, and memoised over
    /// **structurally equal** subterms. Both are load bearing:
    ///
    /// * a term may be [`crate::MAX_DEPTH`] levels deep, and a recursive
    ///   rewrite of one would overflow the stack - an abort that
    ///   `#![forbid(unsafe_code)]`, `unwrap_used` and `panic` cannot see;
    /// * a `Bound` is a DAG, and a shared subterm reached by `2^k` paths must
    ///   be rewritten once rather than `2^k` times. Keying the memo on the
    ///   term rather than on the handle also folds two independently built
    ///   copies of one subterm together, which is strictly stronger and costs
    ///   nothing: `Hash` on a `Bound` is O(1) and equality short-circuits on
    ///   the fingerprint.
    fn rewrite(&self, root: &Bound, on_refusal: OnRefusal) -> Result<Bound, BoundError> {
        // O(1) skip of a whole term. The free-variable summary never produces
        // a false negative, so this cannot leave a stale free variable behind.
        if !self.may_rewrite(root) {
            return Ok(root.clone());
        }
        let mut memo: HashMap<Bound, Bound> = HashMap::new();
        let mut work: Vec<(Bound, bool)> = vec![(root.clone(), false)];
        let mut rewritten: Vec<Bound> = Vec::new();
        while let Some((node, reduce)) = work.pop() {
            if reduce {
                let arity = arity_of(&node);
                let start = rewritten.len().saturating_sub(arity);
                let operands = rewritten.split_off(start);
                let rebuilt = rebuild(&node, operands, on_refusal)?;
                memo.insert(node, rebuilt.clone());
                rewritten.push(rebuilt);
                continue;
            }
            if let Some(known) = memo.get(&node) {
                rewritten.push(known.clone());
                continue;
            }
            // An untouched subtree is returned by handle, not rebuilt.
            if !self.may_rewrite(&node) {
                rewritten.push(node);
                continue;
            }
            match node.kind() {
                BoundKind::Const(_) => rewritten.push(node.clone()),
                BoundKind::Var(var) => {
                    // Simultaneous and single pass: the image is pushed as a
                    // *result*, never back onto the worklist, so it is never
                    // re-scanned and `x := x + 1` terminates. An unbound
                    // variable stays free.
                    rewritten.push(match self.bindings.get(var) {
                        Some(replacement) => replacement.clone(),
                        None => node.clone(),
                    });
                }
                BoundKind::Sum(_)
                | BoundKind::Max(_)
                | BoundKind::Prod(_)
                | BoundKind::Trans { .. } => {
                    work.push((node.clone(), true));
                    // Pushed in reverse so that they pop - and therefore
                    // rebuild - in canonical operand order.
                    for operand in operands_of(&node).into_iter().rev() {
                        work.push((operand, false));
                    }
                }
            }
        }
        match rewritten.pop() {
            Some(result) => Ok(result),
            // Unreachable: the loop pushes exactly one result per node popped
            // and the root is the last one reduced. `omega` is the sound
            // answer if the worklist somehow produced nothing.
            None => Ok(Bound::omega()),
        }
    }
}

/// Adds `next`'s own bindings to an already-composed map, without disturbing
/// the ones that are there.
///
/// A variable bound by both substitutions keeps the composed image: the first
/// substitution acts first, so the second's binding for that variable is
/// unreachable.
fn carry_over(mut composed: BTreeMap<VarId, Bound>, next: &Substitution) -> BTreeMap<VarId, Bound> {
    for (var, image) in &next.bindings {
        composed.entry(var.clone()).or_insert_with(|| image.clone());
    }
    composed
}

/// How many operand results a node consumes from the rewrite stack.
fn arity_of(node: &Bound) -> usize {
    match node.kind() {
        BoundKind::Const(_) | BoundKind::Var(_) => 0,
        BoundKind::Sum(terms) | BoundKind::Prod(terms) => terms.len(),
        BoundKind::Max(terms) => terms.len(),
        BoundKind::Trans { .. } => 1,
    }
}

/// The operands of a node, cloned by handle. `Clone` on a [`Bound`] is an
/// `Arc` bump, so this shares rather than copies.
fn operands_of(node: &Bound) -> Vec<Bound> {
    match node.kind() {
        BoundKind::Const(_) | BoundKind::Var(_) => Vec::new(),
        BoundKind::Sum(terms) | BoundKind::Prod(terms) => terms.as_slice().to_vec(),
        BoundKind::Max(terms) => terms.as_slice().to_vec(),
        BoundKind::Trans { arg, .. } => vec![arg.clone()],
    }
}

/// Rebuilds one node from its rewritten operands, **through the smart
/// constructors**.
///
/// Not optional, and not merely tidy. Substitution changes a child's sort key,
/// can create nesting (`x := a + b` inside a `Sum`), and can trigger `omega`
/// absorption or constant folding. A rebuild that re-sorted without flattening
/// would emit non-canonical terms, which is a second cache key for one
/// program - before normalisation has started.
fn rebuild(node: &Bound, operands: Vec<Bound>, on_refusal: OnRefusal) -> Result<Bound, BoundError> {
    match node.kind() {
        // Leaves are resolved where they are popped, not here.
        BoundKind::Const(_) | BoundKind::Var(_) => Ok(node.clone()),
        BoundKind::Sum(_) => match on_refusal {
            OnRefusal::Widen => Ok(Bound::sum(operands)),
            OnRefusal::Blame => Bound::sum_checked(operands),
        },
        BoundKind::Max(_) => match on_refusal {
            OnRefusal::Widen => Ok(Bound::max_of(operands)),
            OnRefusal::Blame => Bound::max_of_checked(operands),
        },
        BoundKind::Prod(_) => match on_refusal {
            OnRefusal::Widen => Ok(Bound::prod(operands)),
            OnRefusal::Blame => Bound::prod_checked(operands),
        },
        BoundKind::Trans { kind, base, .. } => {
            // `omega` is the sound argument for an operand that is not there.
            let argument = operands.into_iter().next().unwrap_or_else(Bound::omega);
            match (kind, on_refusal) {
                (TransKind::Pow, OnRefusal::Widen) => Ok(Bound::pow(*base, argument)),
                (TransKind::Pow, OnRefusal::Blame) => Bound::pow_checked(*base, argument),
                (TransKind::Log, OnRefusal::Widen) => Ok(Bound::log(*base, argument)),
                (TransKind::Log, OnRefusal::Blame) => Bound::log_checked(*base, argument),
            }
        }
    }
}
