//! [`Bound`] - a weakly monotone, total cost expression.

use core::cmp::Ordering;
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use crate::{
    base::Base, bound_error::BoundError, bound_kind::BoundKind, bound_shape::BoundShape,
    bound_wire::BoundWire, canonical::Canonical, canonical_bytes::CanonicalBytes,
    max_terms::MaxTerms, nat::Nat, symbol::Symbol, terms::Terms, trans_kind::TransKind,
    valuation::Valuation, var_id::VarId, var_set::VarSet, wire_node::WireNode,
};

/// The private node behind every [`Bound`].
///
/// Every field but `kind` is **derived** data, computed by the constructors
/// from the children and never accepted as a parameter. All of them are
/// excluded from equality, hashing and the canonical order, so two
/// structurally identical bounds can never compare unequal because of them.
#[derive(Debug)]
struct Node {
    kind: BoundKind,
    depth: u16,
    vars: VarSet,
    /// A content-derived fingerprint of the whole subterm.
    ///
    /// Computed from the children's fingerprints with the same hardcoded
    /// FNV-1a constants [`VarSet`] uses, so it is identical in every process
    /// and on every toolchain and **equal whenever the terms are equal** -
    /// which is what lets `Hash` be O(1) without breaking the `Hash`/`Eq`
    /// contract, and what turns a structural comparison into a single `u64`
    /// test in the overwhelming majority of cases.
    fingerprint: u64,
    /// The number of nodes this subterm has when expanded as a *tree*,
    /// saturating at `u64::MAX`.
    ///
    /// Not the DAG size: `b = (b * b) + 1` adds two distinct nodes per level
    /// and doubles this. Carried so that [`Bound::try_from_wire`] can measure
    /// what a document is about to materialise rather than the document
    /// itself.
    tree_size: u64,
}

/// A weakly monotone, total cost expression over `N u {omega}`.
///
/// # The guarantee
///
/// Let `[[b]] : Valuation -> Nat` be [`Bound::eval`]. For every `b: Bound` and
/// all valuations `v <= v'` pointwise, `[[b]](v) <= [[b]](v')`. This holds for
/// **every value of this type**, not for values that happened to be built
/// carefully:
///
/// * the six constructors are the only inhabitants, and the only route to one
///   is a smart constructor on this type;
/// * all argument-wise monotonicity lives in five [`Nat`] methods, which is
///   the entire surface where a non-monotone step could be introduced;
/// * the two non-monotone operators the algebra could have admitted -
///   `0^x` via a bad base, and wrapping arithmetic - are unrepresentable
///   ([`Base`] is `>= 2`; overflow saturates to `omega`, never wraps).
///
/// Monotonicity is not tightness. `Const(omega)` is monotone and useless. The
/// value of the guarantee is that composition-by-substitution is always
/// *sound*.
///
/// # Representation
///
/// An opaque handle over a shared, immutable node. `Clone` is O(1) and
/// substitution returns untouched subtrees by handle, so a fixpoint round is
/// O(touched) rather than O(size). Match on [`Bound::kind`].
///
/// # No `Ord`, and no `Default`
///
/// `Bound` implements neither [`Ord`] nor [`PartialOrd`]. The canonical total
/// order is [`Canonical::canonical_cmp`]; see [`Canonical`] for why. This is
/// enforced, not merely documented - the following does not compile:
///
/// ```compile_fail
/// use landav_bound::{Bound, Lifted};
/// let a = Lifted::Elem(Bound::omega());
/// let b = Lifted::Elem(Bound::var("x"));
/// // `Lifted<Bound>: !Ord`, because `Bound: !Ord`.
/// let _joined = a.max(b);
/// ```
///
/// A `compile_fail` doctest passes whenever the snippet fails to compile, for
/// *any* reason - so it is only worth as much as its imports. Keep the crate
/// name and both paths above correct: if they ever go stale this assertion
/// silently stops testing anything, which is exactly what it exists to prevent.
/// `Lifted<Nat>` *is* `Ord`; swapping `Bound` for `Nat` above must make this
/// doctest fail, and that is the check that it still bites.
///
/// There is no `Default` either: a default `Bound` would have to be `Const(0)`
/// or `Const(omega)`, and both are meaning-critical values that must never
/// arise by accident.
#[derive(Debug, Clone)]
pub struct Bound(Arc<Node>);

impl Bound {
    // ---- constants (functions, not associated consts: the node is `Arc`ed) ----

    /// `0` - a *proved* cost of nothing.
    ///
    /// This value means exactly one thing. It is **not** the additive identity
    /// of any registered semiring (that is [`crate::Lifted::Bottom`]), it is
    /// not a fixpoint seed, and it is not a placeholder for "not yet
    /// computed". Nothing in this crate ever produces it by default.
    #[must_use]
    pub fn zero() -> Self {
        Self::constant(0)
    }

    /// `1`.
    #[must_use]
    pub fn one() -> Self {
        Self::constant(1)
    }

    /// `omega` - no finite bound was established.
    ///
    /// Prefer routing this through [`crate::Verdict::classify`], which will
    /// not let it escape without blame.
    #[must_use]
    pub fn omega() -> Self {
        Self::magnitude(Nat::OMEGA)
    }

    // ---- the six constructors, all total, none returns `Result` ----

    /// A finite literal.
    #[must_use]
    pub fn constant(n: u64) -> Self {
        Self::magnitude(Nat::Fin(n))
    }

    /// A literal magnitude, possibly `omega`.
    #[must_use]
    pub fn magnitude(n: Nat) -> Self {
        Self::leaf(BoundKind::Const(n))
    }

    /// An input-size variable.
    #[must_use]
    pub fn var(name: impl Into<Symbol>) -> Self {
        Self::leaf(BoundKind::Var(VarId::new(name)))
    }

    /// `t0 + t1 + ...`.
    ///
    /// Flattens nested sums, drops `Const(0)` operands (`0 + x = x` for every
    /// `x` including `omega`), absorbs `omega` (`x + omega = omega` for every
    /// `x`), constant-folds the finite literals into one operand, sorts into
    /// canonical order, and collapses arity 0 to [`Bound::zero`] and arity 1
    /// to the operand. Every step is denotation preserving. Total.
    #[must_use]
    pub fn sum(terms: impl IntoIterator<Item = Self>) -> Self {
        Self::assemble(NaryOp::Sum, terms.into_iter().collect()).unwrap_or_else(|_| Self::omega())
    }

    /// `max(t0, t1, ...)`.
    ///
    /// Flattens, drops `Const(0)`, absorbs `omega`, constant-folds,
    /// deduplicates, sorts, and collapses arity 0 to [`Bound::zero`] and
    /// arity 1 to the operand. Total.
    #[must_use]
    pub fn max_of(terms: impl IntoIterator<Item = Self>) -> Self {
        Self::assemble(NaryOp::Max, terms.into_iter().collect()).unwrap_or_else(|_| Self::omega())
    }

    /// `t0 * t1 * ...`.
    ///
    /// In this exact order:
    ///
    /// 1. flatten nested products;
    /// 2. **if any operand is `Const(omega)`, the result is `Const(omega)`** -
    ///    exact, because [`Nat::times`] absorbs `omega` unconditionally;
    /// 3. constant-fold the finite literals, saturating overflow to
    ///    `Const(omega)`;
    /// 4. drop the folded constant if it is `1`;
    /// 5. **if the folded constant is `0`, collapse to [`Bound::zero`] only
    ///    when there are no other operands.** `Prod[Const(0), Var(x)]` is
    ///    *not* folded to `0`, because variables range over `N u {omega}` and
    ///    the product is `omega` at `x = omega`. Folding it would make the
    ///    constructor non denotation-preserving in the unsound direction, and
    ///    would make `?a * 0 -> 0` an unsound e-graph congruence;
    /// 6. sort; collapse arity 0 to [`Bound::one`] and arity 1 to the operand.
    ///
    /// Total.
    #[must_use]
    pub fn prod(terms: impl IntoIterator<Item = Self>) -> Self {
        Self::assemble(NaryOp::Prod, terms.into_iter().collect()).unwrap_or_else(|_| Self::omega())
    }

    /// `base ^ exponent`. Constant-folds a `Const` argument via
    /// [`Nat::exp_of`]. Total - [`Base`] has already excluded the bad bases.
    #[must_use]
    pub fn pow(base: Base, exponent: Self) -> Self {
        Self::transcendental(TransKind::Pow, base, exponent).unwrap_or_else(|_| Self::omega())
    }

    /// `ceil(log_base(max(1, argument)))`. Constant-folds a `Const` argument
    /// via [`Nat::ceil_log`]. Total.
    ///
    /// Constant folding here is load bearing beyond tidiness: without it,
    /// `log_2(Const(1))` and `Const(0)` denote the same function but are
    /// distinct terms, and `star` - which tests syntactically for a zero -
    /// would stop being a function of the denotation.
    #[must_use]
    pub fn log(base: Base, argument: Self) -> Self {
        Self::transcendental(TransKind::Log, base, argument).unwrap_or_else(|_| Self::omega())
    }

    // ---- the `_checked` constructors: same rules, blame instead of widening ----

    /// As [`Bound::sum`], but reports the depth limit instead of widening to
    /// `omega`.
    ///
    /// # Errors
    ///
    /// [`BoundError::DepthExceeded`], [`BoundError::NodeBudgetExceeded`], or
    /// [`BoundError::ArityExceeded`] if flattening would produce more
    /// operands than [`crate::MAX_NODES`] allows nodes.
    pub fn sum_checked(terms: impl IntoIterator<Item = Self>) -> Result<Self, BoundError> {
        Self::assemble(NaryOp::Sum, terms.into_iter().collect())
    }

    /// As [`Bound::max_of`], but reports the depth limit.
    ///
    /// # Errors
    ///
    /// [`BoundError::DepthExceeded`], [`BoundError::NodeBudgetExceeded`], or
    /// [`BoundError::ArityExceeded`] if flattening would produce more
    /// operands than [`crate::MAX_NODES`] allows nodes.
    pub fn max_of_checked(terms: impl IntoIterator<Item = Self>) -> Result<Self, BoundError> {
        Self::assemble(NaryOp::Max, terms.into_iter().collect())
    }

    /// As [`Bound::prod`], but reports the depth limit.
    ///
    /// # Errors
    ///
    /// [`BoundError::DepthExceeded`], [`BoundError::NodeBudgetExceeded`], or
    /// [`BoundError::ArityExceeded`] if flattening would produce more
    /// operands than [`crate::MAX_NODES`] allows nodes.
    pub fn prod_checked(terms: impl IntoIterator<Item = Self>) -> Result<Self, BoundError> {
        Self::assemble(NaryOp::Prod, terms.into_iter().collect())
    }

    /// As [`Bound::pow`], but reports the depth limit.
    ///
    /// # Errors
    ///
    /// [`BoundError::DepthExceeded`].
    pub fn pow_checked(base: Base, exponent: Self) -> Result<Self, BoundError> {
        Self::transcendental(TransKind::Pow, base, exponent)
    }

    /// As [`Bound::log`], but reports the depth limit.
    ///
    /// # Errors
    ///
    /// [`BoundError::DepthExceeded`].
    pub fn log_checked(base: Base, argument: Self) -> Result<Self, BoundError> {
        Self::transcendental(TransKind::Log, base, argument)
    }

    // ---- observation ----

    /// The constructor and its payload, for matching.
    #[must_use]
    pub fn kind(&self) -> &BoundKind {
        &self.0.kind
    }

    /// The constructor tag, as a fieldless value.
    #[must_use]
    pub fn shape(&self) -> BoundShape {
        self.0.kind.shape()
    }

    /// The nesting depth, always `<= `[`crate::MAX_DEPTH`]. O(1).
    #[must_use]
    pub fn depth(&self) -> u16 {
        self.0.depth
    }

    /// The conservative free-variable summary. O(1).
    #[must_use]
    pub fn var_set(&self) -> VarSet {
        self.0.vars
    }

    /// `false` guarantees `var` does not occur; `true` means it may. O(1).
    #[must_use]
    pub fn may_contain_var(&self, var: &VarId) -> bool {
        self.0.vars.may_contain(var)
    }

    /// `true` iff `omega` occurs nowhere in this bound.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        let mut work: Vec<&Self> = vec![self];
        while let Some(node) = work.pop() {
            match node.kind() {
                BoundKind::Const(magnitude) => {
                    if !magnitude.is_finite() {
                        return false;
                    }
                }
                BoundKind::Var(_) => {}
                BoundKind::Sum(operands) | BoundKind::Prod(operands) => {
                    work.extend(operands.as_slice());
                }
                BoundKind::Max(operands) => work.extend(operands.as_slice()),
                BoundKind::Trans { arg, .. } => work.push(arg),
            }
        }
        true
    }

    /// Every variable occurring in this bound, **sorted ascending by
    /// [`VarId`] and deduplicated**.
    ///
    /// Sorted, not merely "canonical", and an owned `Vec` rather than an
    /// iterator over a `HashSet`: this result reaches
    /// [`BoundError::UnboundVariable`], which reaches a user-visible message
    /// and a CI log diff, and a `HashSet` iteration order differs between two
    /// runs of the same binary.
    #[must_use]
    pub fn vars(&self) -> Vec<VarId> {
        // A `BTreeSet` rather than a `HashSet`: this result reaches
        // `BoundError::UnboundVariable`, a user-visible message and a CI log
        // diff, so the order must be a function of the content.
        let mut found: BTreeSet<VarId> = BTreeSet::new();
        let mut work: Vec<&Self> = vec![self];
        while let Some(node) = work.pop() {
            match node.kind() {
                BoundKind::Const(_) => {}
                BoundKind::Var(var) => {
                    found.insert(var.clone());
                }
                BoundKind::Sum(operands) | BoundKind::Prod(operands) => {
                    work.extend(operands.as_slice());
                }
                BoundKind::Max(operands) => work.extend(operands.as_slice()),
                BoundKind::Trans { arg, .. } => work.push(arg),
            }
        }
        found.into_iter().collect()
    }

    /// The number of distinct nodes in this bound's DAG - that is, the number
    /// of entries [`Bound::to_wire`] will emit.
    ///
    /// Exposed so a caller can check the serialised size *before* serialising.
    #[must_use]
    pub fn wire_node_count(&self) -> u32 {
        // O(distinct nodes), not O(tree): `canonical_dag` visits each shared
        // node once. This is offered as the cheap pre-check before
        // serialising, so it may not be exponential in the thing it guards.
        u32::try_from(self.canonical_dag().0.len()).unwrap_or(u32::MAX)
    }

    /// Denotation.
    ///
    /// **Infallible.** [`Valuation`] is total by signature and every operator
    /// is total on `N u {omega}`, so there is no failure mode to report.
    /// Implemented with an explicit worklist rather than recursion.
    #[must_use]
    pub fn eval<V: Valuation + ?Sized>(&self, at: &V) -> Nat {
        // An explicit worklist, not recursion: `MAX_DEPTH` bounds the tree but
        // the evaluator must be total regardless of how it is reached.
        //
        // Memoised on node identity, so a shared subterm is folded once. The
        // cache is per call, so the addresses cannot be recycled while it
        // lives: `self` keeps the whole DAG alive for the duration.
        let mut cache: HashMap<usize, Nat> = HashMap::new();
        let mut work: Vec<(&Self, bool)> = vec![(self, false)];
        let mut values: Vec<Nat> = Vec::new();
        while let Some((node, reduce)) = work.pop() {
            if reduce {
                let arity = arity_of(node.kind());
                let start = values.len().saturating_sub(arity);
                let args = values.split_off(start);
                let folded = reduce_node(node.kind(), &args, at);
                cache.insert(node.addr(), folded);
                values.push(folded);
                continue;
            }
            if let Some(known) = cache.get(&node.addr()) {
                values.push(*known);
                continue;
            }
            match node.kind() {
                BoundKind::Const(magnitude) => values.push(*magnitude),
                BoundKind::Var(var) => values.push(at.value_of(var)),
                BoundKind::Sum(operands) | BoundKind::Prod(operands) => {
                    work.push((node, true));
                    // Pushed in reverse so that they pop - and therefore fold -
                    // in canonical order.
                    for operand in operands.as_slice().iter().rev() {
                        work.push((operand, false));
                    }
                }
                BoundKind::Max(operands) => {
                    work.push((node, true));
                    for operand in operands.as_slice().iter().rev() {
                        work.push((operand, false));
                    }
                }
                BoundKind::Trans { arg, .. } => {
                    work.push((node, true));
                    work.push((arg, false));
                }
            }
        }
        // `omega` is the sound answer if the worklist somehow produced nothing.
        values.pop().unwrap_or(Nat::OMEGA)
    }

    /// Replace every occurrence of `var` by `replacement`.
    ///
    /// **Total**, and closed: monotone in, monotone out, because a composition
    /// of monotone functions is monotone. This is the seam LAN-57 builds on.
    ///
    /// Two contract points that are not optional:
    ///
    /// * it **rebuilds through the smart constructors**. Substitution changes
    ///   a child's sort key, can create nesting
    ///   (`x := a + b` inside a `Sum`), and can trigger `omega` absorption. A
    ///   `subst` that re-sorts without flattening emits non-canonical terms,
    ///   which is a second cache key for one program;
    /// * it returns untouched subtrees **by handle**, gated on
    ///   [`Bound::may_contain_var`].
    ///
    /// It does **not** establish that `replacement` over-approximates `var`.
    /// That is a semantic obligation on the caller about the relationship
    /// between the bound and the program, and no Rust type can discharge it.
    #[must_use]
    pub fn subst(&self, var: &VarId, replacement: &Self) -> Self {
        // O(1) skip of an untouched subtree. `VarSet` never produces a false
        // negative, so this cannot leave a stale free variable behind.
        if !self.may_contain_var(var) {
            return self.clone();
        }
        match self.kind() {
            BoundKind::Const(_) => self.clone(),
            BoundKind::Var(here) => {
                if here == var {
                    replacement.clone()
                } else {
                    self.clone()
                }
            }
            // Rebuilt through the smart constructors: substitution changes a
            // child's sort key, can create nesting, and can trigger absorption.
            BoundKind::Sum(operands) => Self::sum(
                operands
                    .as_slice()
                    .iter()
                    .map(|operand| operand.subst(var, replacement)),
            ),
            BoundKind::Max(operands) => Self::max_of(
                operands
                    .as_slice()
                    .iter()
                    .map(|operand| operand.subst(var, replacement)),
            ),
            BoundKind::Prod(operands) => Self::prod(
                operands
                    .as_slice()
                    .iter()
                    .map(|operand| operand.subst(var, replacement)),
            ),
            BoundKind::Trans { kind, base, arg } => {
                let rewritten = arg.subst(var, replacement);
                match kind {
                    TransKind::Pow => Self::pow(*base, rewritten),
                    TransKind::Log => Self::log(*base, rewritten),
                }
            }
        }
    }

    /// The canonical byte encoding. See [`CanonicalBytes`].
    #[must_use]
    pub fn canonical_bytes(&self) -> CanonicalBytes {
        let mut out = Vec::new();
        out.extend_from_slice(&crate::NORMAL_FORM_VERSION.to_be_bytes());
        self.write_canonical(&mut out);
        CanonicalBytes::from_vec(out)
    }

    /// The address of this handle's node. Only ever used to *memoise* a
    /// traversal within a single call, never to define an answer.
    fn addr(&self) -> usize {
        Arc::as_ptr(&self.0).addr()
    }

    /// The distinct subterms of this bound in post-order, with the index of
    /// each.
    ///
    /// Deduplicated **structurally**, not by `Arc` identity: two independently
    /// built copies of one term must produce the same table, or the canonical
    /// byte form would depend on how the term was assembled rather than on
    /// what it is. Children are visited in canonical operand order, so the
    /// table is a function of the term alone.
    ///
    /// O(distinct nodes). Every observer that would otherwise walk the tree is
    /// built on this.
    fn canonical_dag(&self) -> (Vec<Self>, HashMap<Self, u64>) {
        let mut index: HashMap<Self, u64> = HashMap::new();
        let mut order: Vec<Self> = Vec::new();
        let mut work: Vec<(Self, bool)> = vec![(self.clone(), false)];
        while let Some((node, emit)) = work.pop() {
            if index.contains_key(&node) {
                continue;
            }
            if !emit {
                work.push((node.clone(), true));
                // Pushed in reverse so children pop in canonical order.
                for child in children_of(&node).into_iter().rev() {
                    work.push((child, false));
                }
                continue;
            }
            let position = u64::try_from(order.len()).unwrap_or(u64::MAX);
            order.push(node.clone());
            index.insert(node, position);
        }
        (order, index)
    }

    /// The explicit-DAG wire form.
    ///
    /// # Errors
    ///
    /// [`BoundError::NodeBudgetExceeded`] if the DAG exceeds
    /// [`crate::MAX_NODES`].
    pub fn to_wire(&self) -> Result<BoundWire, BoundError> {
        let (order, index) = self.canonical_dag();
        if u64::try_from(order.len()).unwrap_or(u64::MAX) > u64::from(crate::MAX_NODES) {
            return Err(node_budget_exceeded());
        }
        let mut nodes: Vec<WireNode> = Vec::with_capacity(order.len());
        for node in &order {
            nodes.push(match node.kind() {
                BoundKind::Const(Nat::Fin(value)) => WireNode::Const { fin: Some(*value) },
                BoundKind::Const(Nat::Omega) => WireNode::Const { fin: None },
                BoundKind::Var(var) => WireNode::Var {
                    name: var.symbol().as_str().to_owned(),
                },
                BoundKind::Sum(operands) => WireNode::Sum {
                    args: wire_args(&index, operands.as_slice())?,
                },
                BoundKind::Prod(operands) => WireNode::Prod {
                    args: wire_args(&index, operands.as_slice())?,
                },
                BoundKind::Max(operands) => WireNode::Max {
                    args: wire_args(&index, operands.as_slice())?,
                },
                BoundKind::Trans { kind, base, arg } => WireNode::Trans {
                    kind: *kind,
                    base: base.get(),
                    arg: wire_index(&index, arg)?,
                },
            });
        }
        Ok(BoundWire {
            version: crate::WIRE_VERSION,
            nodes,
            root: wire_index(&index, self)?,
        })
    }

    /// Rebuilds a bound from its wire form, **through the smart
    /// constructors**, so a hand-edited or platform-supplied document cannot
    /// introduce a term this crate could not itself have built.
    ///
    /// Validation is strictly weaker than re-canonicalisation, and only
    /// re-canonicalisation preserves the one-program-one-key property the
    /// F-008 cache rests on.
    ///
    /// # This is stricter than the constructors, deliberately
    ///
    /// A document is measured against the **tree** it would materialise, not
    /// against the node table that carries it: fifty in-budget wire nodes can
    /// describe a term of `2^24` tree nodes, and while every observer on
    /// [`Bound`] is memoised over the shared nodes, [`core::fmt::Display`]
    /// still renders the tree.
    ///
    /// The smart constructors carry no such limit, so a term built in process
    /// can be one this method would refuse on the way back in. That asymmetry
    /// is intended - ingest is the untrusted direction and in-process
    /// construction is the caller doing it to themselves - but it means
    /// [`Bound::to_wire`] can emit a document that this method rejects, and a
    /// caller relying on a total round trip should check
    /// [`Bound::wire_node_count`] and the term's own size first. If
    /// `Display` is ever made DAG-aware, this guard becomes unnecessary and
    /// the round trip can be made total again.
    ///
    /// # Errors
    ///
    /// [`BoundError::WireVersionUnsupported`], [`BoundError::WireMalformed`],
    /// [`BoundError::DepthExceeded`], [`BoundError::NodeBudgetExceeded`],
    /// [`BoundError::TreeSizeExceeded`], [`BoundError::ArityExceeded`],
    /// [`BoundError::BaseTooSmall`].
    pub fn try_from_wire(wire: &BoundWire) -> Result<Self, BoundError> {
        if wire.version != crate::WIRE_VERSION {
            return Err(BoundError::WireVersionUnsupported {
                got: wire.version,
                supported: crate::WIRE_VERSION,
            });
        }
        let budget = usize::try_from(crate::MAX_NODES).unwrap_or(usize::MAX);
        if wire.nodes.len() > budget {
            return Err(node_budget_exceeded());
        }
        // Rebuilt bottom-up **through the smart constructors**, so a
        // hand-edited document cannot carry a term this crate could not build.
        let mut built: Vec<Self> = Vec::with_capacity(wire.nodes.len());
        for (position, node) in wire.nodes.iter().enumerate() {
            let rebuilt = match node {
                WireNode::Const { fin } => match fin {
                    Some(value) => Self::constant(*value),
                    None => Self::omega(),
                },
                WireNode::Var { name } => Self::var(name.as_str()),
                WireNode::Sum { args } => {
                    Self::sum_checked(wire_children(&built, args, position)?)?
                }
                WireNode::Max { args } => {
                    Self::max_of_checked(wire_children(&built, args, position)?)?
                }
                WireNode::Prod { args } => {
                    Self::prod_checked(wire_children(&built, args, position)?)?
                }
                WireNode::Trans { kind, base, arg } => {
                    let base = Base::new(*base)?;
                    let child = wire_child(&built, *arg, position)?;
                    match kind {
                        TransKind::Pow => Self::pow_checked(base, child)?,
                        TransKind::Log => Self::log_checked(base, child)?,
                    }
                }
            };
            // The document is inside every budget; the *term* it materialises
            // may not be. Fifty in-budget nodes can describe a tree of `2^24`,
            // and `Display` still renders the tree.
            if rebuilt.0.tree_size > u64::from(crate::MAX_NODES) {
                return Err(BoundError::TreeSizeExceeded {
                    got: rebuilt.0.tree_size,
                    limit: crate::MAX_NODES,
                });
            }
            built.push(rebuilt);
        }
        let root = usize::try_from(wire.root).map_err(|_| BoundError::WireMalformed {
            detail: "root index out of range",
        })?;
        built.get(root).cloned().ok_or(BoundError::WireMalformed {
            detail: "root index out of range",
        })
    }
}

/// Structural equality, ignoring the derived `depth` and `vars` fields.
///
/// Hand written rather than derived precisely so that those cache fields can
/// never make two structurally identical bounds compare unequal - which would
/// break `MaxTerms` deduplication and make the canonical child order
/// cache-dependent. Short-circuits on `Arc::ptr_eq`; recursion is bounded by
/// [`crate::MAX_DEPTH`].
impl PartialEq for Bound {
    fn eq(&self, other: &Self) -> bool {
        // The derived fields are deliberately not compared. Sharing and the
        // fingerprint only ever *shortcut* the answer; the structure defines
        // it, and the memo table keeps a shared subterm from being compared
        // once per path that reaches it.
        if Arc::ptr_eq(&self.0, &other.0) {
            return true;
        }
        if self.0.fingerprint != other.0.fingerprint {
            return false;
        }
        let mut decided: HashMap<(usize, usize), bool> = HashMap::new();
        structurally_eq(self, other, &mut decided)
    }
}

impl Eq for Bound {}

/// Hashes the constructor and payload only, matching [`PartialEq`].
impl core::hash::Hash for Bound {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        // O(1), and it matches `PartialEq`: the fingerprint is derived from
        // the children's fingerprints, so equal terms have equal
        // fingerprints. Hashing the `BoundKind` directly would walk the tree,
        // which is exponential on a shared DAG.
        state.write_u64(self.0.fingerprint);
    }
}

impl Canonical for Bound {
    fn canonical_cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Sharing may *shortcut* the comparison; it never defines it.
        if Arc::ptr_eq(&self.0, &other.0) {
            return Ordering::Equal;
        }
        let mut decided: HashMap<(usize, usize), Ordering> = HashMap::new();
        canonical_cmp_memo(self, other, &mut decided)
    }

    /// The **DAG** encoding: a length-prefixed table of the distinct subterms
    /// in post-order, each referring to its children by table index, followed
    /// by the index of the root.
    ///
    /// A tree encoding would be exponential in a shared term - `b = (b*b)+1`
    /// is 42 distinct nodes at twenty levels and 39 MB of tree - and
    /// `CanonicalBytes` is the cache-key material, so it may not be. The table
    /// is deduplicated structurally and ordered by a traversal of the term, so
    /// it stays content derived: two independently built copies of one term
    /// produce identical bytes whatever their sharing.
    fn write_canonical(&self, out: &mut Vec<u8>) {
        let (order, index) = self.canonical_dag();
        let count = u64::try_from(order.len()).unwrap_or(u64::MAX);
        out.extend_from_slice(&count.to_be_bytes());
        for node in &order {
            write_node_record(node, &index, out);
        }
        let root = index.get(self).copied().unwrap_or_default();
        out.extend_from_slice(&root.to_be_bytes());
    }
}

/// Renders in **canonical** operand order, e.g. `x1 * (2 + log2(x1))`.
///
/// There is deliberately no second "presentation" order. A presentation order
/// distinct from the canonical order can drift from it, and every golden test
/// for LAN-58 would then be pinning the wrong artefact. LAN-57's acceptance
/// criterion should be restated against this rendering rather than against
/// KoAT's source-order string `x1 * (log2(x1) + 2)`.
impl core::fmt::Display for Bound {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.kind() {
            BoundKind::Const(Nat::Fin(value)) => write!(f, "{value}"),
            BoundKind::Const(Nat::Omega) => f.write_str("omega"),
            BoundKind::Var(var) => write!(f, "{var}"),
            BoundKind::Sum(operands) => write_joined(f, operands.as_slice(), " + "),
            BoundKind::Prod(operands) => write_joined(f, operands.as_slice(), " * "),
            BoundKind::Max(operands) => {
                f.write_str("max")?;
                write_joined(f, operands.as_slice(), ", ")
            }
            BoundKind::Trans { kind, base, arg } => match kind {
                TransKind::Pow => write!(f, "{}^({arg})", base.get()),
                TransKind::Log => write!(f, "log{}({arg})", base.get()),
            },
        }
    }
}

/// Which n-ary node an operand list is being assembled for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NaryOp {
    /// `t0 + t1 + ...`.
    Sum,
    /// `max(t0, t1, ...)`.
    Max,
    /// `t0 * t1 * ...`.
    Prod,
}

impl NaryOp {
    /// The constructor tag this operator assembles.
    const fn shape(self) -> BoundShape {
        match self {
            Self::Sum => BoundShape::Sum,
            Self::Max => BoundShape::Max,
            Self::Prod => BoundShape::Prod,
        }
    }
}

impl Bound {
    /// A leaf node: depth 1, and a free-variable summary derived from the
    /// payload rather than accepted as a parameter.
    fn leaf(kind: BoundKind) -> Self {
        let vars = match &kind {
            BoundKind::Var(var) => VarSet::singleton(var),
            _ => VarSet::EMPTY,
        };
        let fingerprint = fingerprint_of(&kind);
        Self(Arc::new(Node {
            kind,
            depth: 1,
            vars,
            fingerprint,
            tree_size: 1,
        }))
    }

    /// The shared body of the three n-ary smart constructors.
    ///
    /// Flatten, fold the literals, drop the identity, absorb `omega`, sort
    /// (and deduplicate, for `Max`), then collapse arity 0 and 1. Every step
    /// is denotation preserving.
    fn assemble(op: NaryOp, terms: Vec<Self>) -> Result<Self, BoundError> {
        // Budgeted **before** the flattened vector is allocated. Flattening a
        // same-operator child doubles the operand list while the depth and the
        // DAG stay constant, so neither `MAX_DEPTH` nor the node budget sees
        // it, and the failure mode of a `Vec` that cannot grow is an abort.
        let mut operand_count: u64 = 0;
        for term in &terms {
            operand_count = operand_count.saturating_add(match (op, term.kind()) {
                (NaryOp::Sum, BoundKind::Sum(inner)) | (NaryOp::Prod, BoundKind::Prod(inner)) => {
                    u64::try_from(inner.len()).unwrap_or(u64::MAX)
                }
                (NaryOp::Max, BoundKind::Max(inner)) => {
                    u64::try_from(inner.len()).unwrap_or(u64::MAX)
                }
                _ => 1,
            });
        }
        if operand_count > u64::from(crate::MAX_NODES) {
            return Err(BoundError::ArityExceeded {
                op: op.shape(),
                got: operand_count,
                limit: crate::MAX_NODES,
            });
        }

        let mut flat: Vec<Self> = Vec::with_capacity(terms.len());
        for term in terms {
            // One level suffices: every nested node is already flat.
            let nested = match (op, term.kind()) {
                (NaryOp::Sum, BoundKind::Sum(inner)) | (NaryOp::Prod, BoundKind::Prod(inner)) => {
                    Some(inner.as_slice().to_vec())
                }
                (NaryOp::Max, BoundKind::Max(inner)) => Some(inner.as_slice().to_vec()),
                _ => None,
            };
            match nested {
                Some(operands) => flat.extend(operands),
                None => flat.push(term),
            }
        }

        let mut literals: Vec<Nat> = Vec::new();
        let mut operands: Vec<Self> = Vec::new();
        for term in flat {
            if let BoundKind::Const(magnitude) = term.kind() {
                literals.push(*magnitude);
            } else {
                operands.push(term);
            }
        }

        match op {
            // `0 + x = x` and `max(0, x) = x` for every `x` in `N u {omega}`,
            // and `omega` absorbs, so a folded `omega` is the whole answer.
            NaryOp::Sum | NaryOp::Max => {
                let folded = literals.iter().fold(Nat::ZERO, |accumulator, value| {
                    if op == NaryOp::Sum {
                        accumulator.plus(*value)
                    } else {
                        accumulator.join(*value)
                    }
                });
                if folded == Nat::OMEGA {
                    return Ok(Self::omega());
                }
                if folded != Nat::ZERO {
                    operands.push(Self::magnitude(folded));
                }
            }
            NaryOp::Prod => {
                // `omega` absorbs unconditionally, including `0 * omega`, so
                // it is tested first and it wins even against a zero.
                if literals.iter().any(|value| !value.is_finite()) {
                    return Ok(Self::omega());
                }
                if literals.contains(&Nat::ZERO) {
                    // A zero literal decides the literal part **exactly**.
                    // The product of the literals really is zero; only
                    // saturating the other literals *first* could have
                    // invented an `omega`, and inventing one here published
                    // `Err(UnblamedOmega)` for a program proved to cost
                    // nothing. The remaining literals carry no information
                    // once a zero is among them, so they are dropped - which
                    // is also what step 3 of this constructor's contract
                    // describes.
                    if operands.is_empty() {
                        return Ok(Self::zero());
                    }
                    // `Prod[Const(0), Var(x)]` is **not** folded to `0`:
                    // variables range over `N u {omega}` and the product is
                    // `omega` at `x = omega`. That is why the zero survives as
                    // an operand rather than collapsing the node.
                    operands.push(Self::zero());
                } else {
                    // Every literal is at least one here, so the running
                    // product only grows and whether it leaves the carrier
                    // does not depend on the order. Overflow saturates to
                    // `omega`, never truncating.
                    let folded = fold_product(&literals);
                    if folded == Nat::OMEGA {
                        return Ok(Self::omega());
                    }
                    if folded != Nat::ONE {
                        operands.push(Self::magnitude(folded));
                    }
                }
            }
        }

        operands.sort_by(Canonical::canonical_cmp);
        if op == NaryOp::Max {
            // `max` is idempotent, so duplicates are removed in the type.
            operands.dedup();
        }
        if operands.is_empty() {
            return Ok(match op {
                NaryOp::Prod => Self::one(),
                NaryOp::Sum | NaryOp::Max => Self::zero(),
            });
        }
        if operands.len() == 1 {
            return Ok(operands.swap_remove(0));
        }

        let depth = nary_depth(&operands)?;
        let vars = union_vars(&operands);
        let tree_size = tree_size_of(&operands);
        let kind = match op {
            NaryOp::Sum => BoundKind::Sum(Terms::from_canonical(operands)),
            NaryOp::Prod => BoundKind::Prod(Terms::from_canonical(operands)),
            NaryOp::Max => BoundKind::Max(MaxTerms::from_canonical(operands)),
        };
        let fingerprint = fingerprint_of(&kind);
        Ok(Self(Arc::new(Node {
            kind,
            depth,
            vars,
            fingerprint,
            tree_size,
        })))
    }

    /// The shared body of `pow` and `log`. Constant-folds a `Const` argument,
    /// which is what keeps `star`'s syntactic zero test a function of the
    /// denotation.
    fn transcendental(which: TransKind, base: Base, arg: Self) -> Result<Self, BoundError> {
        if let BoundKind::Const(magnitude) = arg.kind() {
            let folded = match which {
                TransKind::Pow => magnitude.exp_of(base),
                TransKind::Log => magnitude.ceil_log(base),
            };
            return Ok(Self::magnitude(folded));
        }
        let depth = arg.depth().saturating_add(1);
        if depth > crate::MAX_DEPTH {
            return Err(BoundError::DepthExceeded {
                limit: crate::MAX_DEPTH,
            });
        }
        let vars = arg.var_set();
        let tree_size = arg.0.tree_size.saturating_add(1);
        let kind = BoundKind::Trans {
            kind: which,
            base,
            arg,
        };
        let fingerprint = fingerprint_of(&kind);
        Ok(Self(Arc::new(Node {
            kind,
            depth,
            vars,
            fingerprint,
            tree_size,
        })))
    }
}

/// The product of a multiset of magnitudes, computed order independently.
///
/// The three cases, in the order they are tested, are exactly the ideal
/// product saturated once at the end:
///
/// 1. `omega` absorbs **unconditionally**, including against zero - the
///    frozen `Nat::times` rule - so it is tested first;
/// 2. a zero factor then makes the product exactly zero. It may not be
///    pre-empted by an overflow of the *other* factors: that product is a
///    magnitude the carrier cannot hold, but multiplying it by zero still
///    gives zero, and reporting `omega` there is looseness with a hard
///    consequence - `Verdict::classify` refuses an unblamed `omega`, so a
///    cost proved to be nothing became a tool error;
/// 3. everything left is at least one, so the running product only grows and
///    whether it leaves the carrier does not depend on the order the factors
///    arrive in. Overflow saturates to `omega`, never truncating to
///    `u64::MAX`.
///
/// Order independent in all three cases, which is what `Prod`'s canonical
/// operand order requires of it.
fn fold_product(factors: &[Nat]) -> Nat {
    if factors.iter().any(|value| !value.is_finite()) {
        return Nat::OMEGA;
    }
    if factors.contains(&Nat::ZERO) {
        return Nat::ZERO;
    }
    factors
        .iter()
        .fold(Nat::ONE, |accumulator, value| accumulator.times(*value))
}

/// The depth of an n-ary node, refused rather than truncated at the limit.
fn nary_depth(operands: &[Bound]) -> Result<u16, BoundError> {
    let mut deepest: u16 = 0;
    for operand in operands {
        deepest = deepest.max(operand.depth());
    }
    let depth = deepest.saturating_add(1);
    if depth > crate::MAX_DEPTH {
        return Err(BoundError::DepthExceeded {
            limit: crate::MAX_DEPTH,
        });
    }
    Ok(depth)
}

/// The union of the operands' free-variable summaries. Never a parameter.
fn union_vars(operands: &[Bound]) -> VarSet {
    operands.iter().fold(VarSet::EMPTY, |accumulator, operand| {
        accumulator.union(operand.var_set())
    })
}

/// How many operand values a node consumes from the evaluation stack.
fn arity_of(kind: &BoundKind) -> usize {
    match kind {
        BoundKind::Const(_) | BoundKind::Var(_) => 0,
        BoundKind::Sum(operands) | BoundKind::Prod(operands) => operands.len(),
        BoundKind::Max(operands) => operands.len(),
        BoundKind::Trans { .. } => 1,
    }
}

/// Folds one node's operand values. Total on `N u {omega}` in every arm.
fn reduce_node<V: Valuation + ?Sized>(kind: &BoundKind, args: &[Nat], at: &V) -> Nat {
    match kind {
        BoundKind::Const(magnitude) => *magnitude,
        BoundKind::Var(var) => at.value_of(var),
        BoundKind::Sum(_) => args
            .iter()
            .fold(Nat::ZERO, |accumulator, value| accumulator.plus(*value)),
        BoundKind::Max(_) => args
            .iter()
            .fold(Nat::ZERO, |accumulator, value| accumulator.join(*value)),
        BoundKind::Prod(_) => fold_product(args),
        BoundKind::Trans { kind, base, .. } => {
            // `omega` is the sound answer for a missing operand.
            let argument = args.first().copied().unwrap_or(Nat::OMEGA);
            match kind {
                TransKind::Pow => argument.exp_of(*base),
                TransKind::Log => argument.ceil_log(*base),
            }
        }
    }
}

/// The canonical order over two operand lists: element-wise, then by length.
pub(crate) fn compare_operands(left: &[Bound], right: &[Bound]) -> Ordering {
    for (a, b) in left.iter().zip(right.iter()) {
        let ordering = a.canonical_cmp(b);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

/// The canonical encoding of an operand list: length prefixed, then each
/// operand, so the encoding is self-delimiting.
pub(crate) fn write_operands(operands: &[Bound], out: &mut Vec<u8>) {
    let count = u64::try_from(operands.len()).unwrap_or(u64::MAX);
    out.extend_from_slice(&count.to_be_bytes());
    for operand in operands {
        operand.write_canonical(out);
    }
}

/// The node-budget refusal, spelled once.
fn node_budget_exceeded() -> BoundError {
    BoundError::NodeBudgetExceeded {
        limit: crate::MAX_NODES,
    }
}

/// The wire index of an already-emitted node.
fn wire_index(index: &HashMap<Bound, u64>, node: &Bound) -> Result<u32, BoundError> {
    let position = index.get(node).copied().ok_or(BoundError::WireMalformed {
        detail: "a child was not emitted before its parent",
    })?;
    u32::try_from(position).map_err(|_| node_budget_exceeded())
}

/// The wire indices of an operand list.
fn wire_args(index: &HashMap<Bound, u64>, operands: &[Bound]) -> Result<Vec<u32>, BoundError> {
    operands
        .iter()
        .map(|operand| wire_index(index, operand))
        .collect()
}

/// The children of a node, cloned by handle. `Clone` on a [`Bound`] is an
/// `Arc` bump, so this shares rather than copies.
fn children_of(node: &Bound) -> Vec<Bound> {
    match node.kind() {
        BoundKind::Const(_) | BoundKind::Var(_) => Vec::new(),
        BoundKind::Sum(operands) | BoundKind::Prod(operands) => operands.as_slice().to_vec(),
        BoundKind::Max(operands) => operands.as_slice().to_vec(),
        BoundKind::Trans { arg, .. } => vec![arg.clone()],
    }
}

/// The tree size of an n-ary node: itself plus its operands' trees.
fn tree_size_of(operands: &[Bound]) -> u64 {
    operands.iter().fold(1u64, |total, operand| {
        total.saturating_add(operand.0.tree_size)
    })
}

/// Folds one `u64` into an FNV-1a accumulator, byte by byte.
fn mix(hash: &mut u64, value: u64) {
    for byte in value.to_be_bytes() {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(VarSet::FNV_PRIME);
    }
}

/// A content-derived fingerprint of a node, from its children's fingerprints.
///
/// Uses the hardcoded FNV-1a constants rather than `DefaultHasher`, for the
/// same reason [`VarSet`] does: `RandomState` is seeded per process and the
/// algorithm is not guaranteed across releases, and a fingerprint that differs
/// between two runs would make `Hash` disagree with `PartialEq` for one term
/// observed twice.
fn fingerprint_of(kind: &BoundKind) -> u64 {
    let mut hash = VarSet::FNV_OFFSET_BASIS;
    mix(&mut hash, u64::from(kind.shape().canonical_tag()));
    match kind {
        BoundKind::Const(Nat::Fin(value)) => {
            mix(&mut hash, 1);
            mix(&mut hash, *value);
        }
        BoundKind::Const(Nat::Omega) => mix(&mut hash, 2),
        BoundKind::Var(var) => {
            for byte in var.symbol().as_str().as_bytes() {
                mix(&mut hash, u64::from(*byte));
            }
        }
        BoundKind::Sum(operands) | BoundKind::Prod(operands) => {
            for operand in operands.as_slice() {
                mix(&mut hash, operand.0.fingerprint);
            }
        }
        BoundKind::Max(operands) => {
            for operand in operands.as_slice() {
                mix(&mut hash, operand.0.fingerprint);
            }
        }
        BoundKind::Trans { kind, base, arg } => {
            mix(&mut hash, u64::from(kind.canonical_tag()));
            mix(&mut hash, u64::from(base.get()));
            mix(&mut hash, arg.0.fingerprint);
        }
    }
    hash
}

/// Structural equality with a memo table over node-address pairs.
///
/// The table only ever *records* an answer the structure already determined,
/// and it lives for one call, during which the roots keep every node alive -
/// so no address can be recycled underneath it.
fn structurally_eq(
    left: &Bound,
    right: &Bound,
    decided: &mut HashMap<(usize, usize), bool>,
) -> bool {
    if Arc::ptr_eq(&left.0, &right.0) {
        return true;
    }
    if left.0.fingerprint != right.0.fingerprint {
        return false;
    }
    let key = (left.addr(), right.addr());
    if let Some(known) = decided.get(&key) {
        return *known;
    }
    let answer = match (left.kind(), right.kind()) {
        (BoundKind::Const(a), BoundKind::Const(b)) => a == b,
        (BoundKind::Var(a), BoundKind::Var(b)) => a == b,
        (BoundKind::Sum(a), BoundKind::Sum(b)) | (BoundKind::Prod(a), BoundKind::Prod(b)) => {
            operands_eq(a.as_slice(), b.as_slice(), decided)
        }
        (BoundKind::Max(a), BoundKind::Max(b)) => operands_eq(a.as_slice(), b.as_slice(), decided),
        (
            BoundKind::Trans {
                kind: left_kind,
                base: left_base,
                arg: left_arg,
            },
            BoundKind::Trans {
                kind: right_kind,
                base: right_base,
                arg: right_arg,
            },
        ) => {
            left_kind == right_kind
                && left_base == right_base
                && structurally_eq(left_arg, right_arg, decided)
        }
        _ => false,
    };
    decided.insert(key, answer);
    answer
}

/// Element-wise structural equality of two operand lists.
fn operands_eq(
    left: &[Bound],
    right: &[Bound],
    decided: &mut HashMap<(usize, usize), bool>,
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(a, b)| structurally_eq(a, b, decided))
}

/// [`Canonical::canonical_cmp`] with a memo table over node-address pairs.
fn canonical_cmp_memo(
    left: &Bound,
    right: &Bound,
    decided: &mut HashMap<(usize, usize), Ordering>,
) -> Ordering {
    if Arc::ptr_eq(&left.0, &right.0) {
        return Ordering::Equal;
    }
    let key = (left.addr(), right.addr());
    if let Some(known) = decided.get(&key) {
        return *known;
    }
    let tags = left
        .shape()
        .canonical_tag()
        .cmp(&right.shape().canonical_tag());
    let answer = if tags == Ordering::Equal {
        match (left.kind(), right.kind()) {
            (BoundKind::Const(a), BoundKind::Const(b)) => a.canonical_cmp(b),
            (BoundKind::Var(a), BoundKind::Var(b)) => a.canonical_cmp(b),
            (BoundKind::Sum(a), BoundKind::Sum(b)) | (BoundKind::Prod(a), BoundKind::Prod(b)) => {
                operands_cmp(a.as_slice(), b.as_slice(), decided)
            }
            (BoundKind::Max(a), BoundKind::Max(b)) => {
                operands_cmp(a.as_slice(), b.as_slice(), decided)
            }
            (
                BoundKind::Trans {
                    kind: left_kind,
                    base: left_base,
                    arg: left_arg,
                },
                BoundKind::Trans {
                    kind: right_kind,
                    base: right_base,
                    arg: right_arg,
                },
            ) => left_kind
                .canonical_cmp(right_kind)
                .then_with(|| left_base.canonical_cmp(right_base))
                .then_with(|| canonical_cmp_memo(left_arg, right_arg, decided)),
            // Unreachable: the tags above already agree, so the shapes do.
            _ => Ordering::Equal,
        }
    } else {
        tags
    };
    decided.insert(key, answer);
    answer
}

/// The canonical order over two operand lists: element-wise, then by length.
fn operands_cmp(
    left: &[Bound],
    right: &[Bound],
    decided: &mut HashMap<(usize, usize), Ordering>,
) -> Ordering {
    for (a, b) in left.iter().zip(right.iter()) {
        let ordering = canonical_cmp_memo(a, b, decided);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

/// One entry of the canonical DAG table: the node's own payload, with every
/// child written as a table index rather than inline.
fn write_node_record(node: &Bound, index: &HashMap<Bound, u64>, out: &mut Vec<u8>) {
    out.push(node.shape().canonical_tag());
    match node.kind() {
        BoundKind::Const(magnitude) => magnitude.write_canonical(out),
        BoundKind::Var(var) => var.write_canonical(out),
        BoundKind::Sum(operands) | BoundKind::Prod(operands) => {
            write_child_indices(operands.as_slice(), index, out);
        }
        BoundKind::Max(operands) => write_child_indices(operands.as_slice(), index, out),
        BoundKind::Trans { kind, base, arg } => {
            kind.write_canonical(out);
            base.write_canonical(out);
            write_child_indices(core::slice::from_ref(arg), index, out);
        }
    }
}

/// A length-prefixed run of child table indices.
fn write_child_indices(operands: &[Bound], index: &HashMap<Bound, u64>, out: &mut Vec<u8>) {
    let count = u64::try_from(operands.len()).unwrap_or(u64::MAX);
    out.extend_from_slice(&count.to_be_bytes());
    for operand in operands {
        let position = index.get(operand).copied().unwrap_or_default();
        out.extend_from_slice(&position.to_be_bytes());
    }
}

/// One already-rebuilt child, refusing a forward or out-of-range reference.
fn wire_child(built: &[Bound], index: u32, parent: usize) -> Result<Bound, BoundError> {
    let at = usize::try_from(index).map_err(|_| BoundError::WireMalformed {
        detail: "child index out of range",
    })?;
    if at >= parent {
        return Err(BoundError::WireMalformed {
            detail: "child index is not strictly less than its parent",
        });
    }
    built.get(at).cloned().ok_or(BoundError::WireMalformed {
        detail: "child index out of range",
    })
}

/// The already-rebuilt children of an n-ary wire node.
fn wire_children(built: &[Bound], args: &[u32], parent: usize) -> Result<Vec<Bound>, BoundError> {
    if args.len() < 2 {
        return Err(BoundError::WireMalformed {
            detail: "an n-ary node needs at least two operands",
        });
    }
    args.iter()
        .map(|arg| wire_child(built, *arg, parent))
        .collect()
}

/// Renders an operand list in canonical order, parenthesised.
fn write_joined(
    f: &mut core::fmt::Formatter<'_>,
    operands: &[Bound],
    separator: &str,
) -> core::fmt::Result {
    f.write_str("(")?;
    for (position, operand) in operands.iter().enumerate() {
        if position > 0 {
            f.write_str(separator)?;
        }
        write!(f, "{operand}")?;
    }
    f.write_str(")")
}
