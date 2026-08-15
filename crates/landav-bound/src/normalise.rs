//! Normalisation of [`Bound`] expressions by equality saturation.
//!
//! Normalisation is **syntactic equality up to the algebraic laws**: it makes
//! two differently-written but equal bounds converge on one term, so that a
//! cache key, a golden test and a printed report are all functions of what the
//! bound *is* rather than of how it was assembled.
//!
//! It is emphatically **not** the asymptotic comparator. Deciding whether
//! `O(n*m^2)` dominates `O(n^2*m)` needs Newton-polytope containment; that is
//! F-018 in R2 and nothing here attempts it.
//!
//! # What the smart constructors already did
//!
//! [`Bound`]'s constructors flatten, constant-fold, drop identities, absorb
//! `omega`, sort into [`Canonical`] order and (for `Max`) deduplicate. So a
//! `Bound` is *already* canonical with respect to associativity, commutativity
//! and `max`-idempotence at each node. What remains for the e-graph is the
//! laws that relate two *different* operators - distribution of `*` over `+`
//! - and the regroupings needed to find them.
//!
//! # The determinism contract
//!
//! Everything in [`NormaliserBudget`]'s module documentation is discharged
//! here, and the reasons are restated at each site rather than left in one
//! place:
//!
//! 1. [`Duration::MAX`] as the time limit. `egg`'s `Runner` defaults to five
//!    seconds and checks the clock *before* the node and iteration limits, so
//!    the default makes the extracted term a function of machine load.
//! 2. Only [`NormaliserStop`]'s three count-based reasons are accepted;
//!    anything else is [`BoundError::NonDeterministicNormalisation`].
//! 3. The mirror language carries [`VarId`] and [`Nat`], never `egg::Symbol`.
//!    Even the *pattern* variables are built with `egg::Var::from_u32`, which
//!    is a plain `u32` and touches no interner at all.
//! 4. [`egg::SimpleScheduler`], so rules are applied in the fixed order they
//!    are declared in, with no match-count-driven banning to reason about.
//! 5. An integer-valued, [`Ord`] extraction cost with a total tie-break.
//!
//! # Why the normal form is the *factored* form
//!
//! The cost function minimises tree size, so of the two sides of the
//! distribution law the shorter one wins: `x*y + x*z` normalises to
//! `x * (y + z)` rather than the other way round. That is also the readable
//! direction, which is what acceptance criterion 4 is pinning.

use std::{collections::HashMap, sync::Arc, sync::OnceLock, time::Duration};

use egg::{
    CostFunction, ENodeOrVar, Extractor, Id, Language, Pattern, PatternAst, RecExpr, Rewrite,
    Runner, SimpleScheduler, StopReason, Var,
};

use crate::{
    base::Base, bound::Bound, bound_error::BoundError, bound_kind::BoundKind, canonical::Canonical,
    nat::Nat, normaliser_budget::NormaliserBudget, trans_kind::TransKind, var_id::VarId,
};

// ---------------------------------------------------------------------------
// public surface
// ---------------------------------------------------------------------------

/// Why an equality-saturation run stopped.
///
/// Only the three **count-based** reasons are representable. A wall-clock
/// timeout is not a member of this type by design: it is a function of machine
/// load, so a bound extracted after one is not reproducible, and
/// [`BoundError::NonDeterministicNormalisation`] is the only correct response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormaliserStop {
    /// The rewrite set reached a fixpoint: no rule learned anything new.
    ///
    /// The only reason under which the extracted term is the *fully*
    /// normalised one for the frozen rule set.
    Saturated,
    /// [`NormaliserBudget::iter_limit`] was reached.
    IterationLimit,
    /// [`NormaliserBudget::node_limit`] was reached.
    NodeLimit,
}

impl NormaliserStop {
    /// Every deterministic stop reason, so a fourth is a compile error in the
    /// test suite as well as here.
    pub const ALL: [Self; 3] = [Self::Saturated, Self::IterationLimit, Self::NodeLimit];

    /// A stable name for diagnostics. Written out, not derived from the
    /// identifier, so renaming a variant cannot change a report.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Saturated => "saturated",
            Self::IterationLimit => "iteration-limit",
            Self::NodeLimit => "node-limit",
        }
    }

    /// Maps `egg`'s stop reason onto the deterministic subset.
    ///
    /// # Errors
    ///
    /// [`BoundError::NonDeterministicNormalisation`] for a wall-clock timeout,
    /// for an engine-specific `Other` reason, and for a run that recorded no
    /// reason at all. All three are hard errors: the alternative is publishing
    /// a silently less-normalised bound whose cache key differs from the one
    /// an idle machine would have produced.
    fn classify(reason: Option<&StopReason>) -> Result<Self, BoundError> {
        match reason {
            Some(StopReason::Saturated) => Ok(Self::Saturated),
            Some(StopReason::IterationLimit(_)) => Ok(Self::IterationLimit),
            Some(StopReason::NodeLimit(_)) => Ok(Self::NodeLimit),
            Some(StopReason::TimeLimit(_)) => Err(BoundError::NonDeterministicNormalisation {
                reason: "a wall-clock time limit, which is a function of machine load",
            }),
            Some(StopReason::Other(_)) => Err(BoundError::NonDeterministicNormalisation {
                reason: "an engine-specific stop reason",
            }),
            None => Err(BoundError::NonDeterministicNormalisation {
                reason: "the run recorded no stop reason",
            }),
        }
    }
}

impl core::fmt::Display for NormaliserStop {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A normalised bound, together with everything needed to judge whether the
/// run that produced it was reproducible.
///
/// The stop reason is carried rather than discarded because
/// [`NormaliserStop::Saturated`] is the only one under which the term is
/// *fully* normalised. A caller populating a persisted cache should say so.
#[derive(Debug, Clone)]
pub struct NormalForm {
    bound: Bound,
    stop: NormaliserStop,
    iterations: usize,
    egraph_nodes: usize,
}

impl NormalForm {
    /// The normalised bound.
    #[must_use]
    pub fn bound(&self) -> &Bound {
        &self.bound
    }

    /// The normalised bound, by value.
    #[must_use]
    pub fn into_bound(self) -> Bound {
        self.bound
    }

    /// Why the run stopped. Always one of [`NormaliserStop::ALL`].
    #[must_use]
    pub fn stop(&self) -> NormaliserStop {
        self.stop
    }

    /// How many equality-saturation iterations ran.
    #[must_use]
    pub fn iterations(&self) -> usize {
        self.iterations
    }

    /// How many e-nodes the e-graph held when the run stopped.
    #[must_use]
    pub fn egraph_nodes(&self) -> usize {
        self.egraph_nodes
    }
}

/// Normalises `bound` at [`NormaliserBudget::FROZEN`].
///
/// # Errors
///
/// [`BoundError::NonDeterministicNormalisation`] if the run stopped for a
/// reason that is not reproducible, and [`BoundError::DepthExceeded`],
/// [`BoundError::ArityExceeded`] or [`BoundError::NodeBudgetExceeded`] if the
/// extracted term cannot be rebuilt inside the algebra's limits.
pub fn normalise(bound: &Bound) -> Result<NormalForm, BoundError> {
    normalise_with(bound, NormaliserBudget::FROZEN)
}

/// Normalises `bound` at an explicit budget.
///
/// Soundness does not depend on the budget - the rewrite set never
/// under-approximates at any number of iterations - so a caller that wants a
/// cheaper run may lower it. **The normal form does**, which is why
/// [`normalise`] pins [`NormaliserBudget::FROZEN`] and why the golden and
/// cross-process gates use that entry point rather than this one.
///
/// # Errors
///
/// As [`normalise`].
pub fn normalise_with(bound: &Bound, budget: NormaliserBudget) -> Result<NormalForm, BoundError> {
    let rules = rewrite_rules();
    if rules.is_empty() {
        // Unreachable while `build_rewrite_rules` succeeds, and a hard error
        // rather than a silent identity normalisation if it ever does not:
        // returning the input untouched would be sound but would make every
        // golden and every cache key silently wrong.
        return Err(BoundError::NonDeterministicNormalisation {
            reason: "the frozen rewrite set failed to build",
        });
    }

    let (expr, _) = to_recexpr(bound);
    let mut runner: Runner<BoundNode, ()> = Runner::default()
        // Rules are applied in declaration order, every iteration. The
        // alternative, `BackoffScheduler`, bans rules by match count and is
        // one more thing that would have to be argued to be reproducible.
        .with_scheduler(SimpleScheduler)
        // The whole point of `NormaliserBudget`. `egg` checks the clock
        // *before* the node and iteration limits, so leaving the five-second
        // default in place makes the extracted term a function of machine
        // load - and `cargo-llvm-cov` and a parallel `cargo-mutants` are both
        // load sources this workspace runs on every lane.
        .with_time_limit(Duration::MAX)
        .with_iter_limit(budget.iter_limit())
        .with_node_limit(budget.node_limit());
    let root = runner.egraph.add_expr(&expr);
    let runner = runner.run(rules.iter());

    // Classified *before* extraction, so a non-reproducible run can never
    // reach a caller as a bound at all.
    let stop = NormaliserStop::classify(runner.stop_reason.as_ref())?;

    let root = runner.egraph.find(root);
    let extractor = Extractor::new(&runner.egraph, NormalCost);
    let (_, best) = extractor.find_best(root);
    let normalised = from_recexpr(&best)?;

    Ok(NormalForm {
        bound: normalised,
        stop,
        iterations: runner.iterations.len(),
        egraph_nodes: runner.egraph.total_size(),
    })
}

/// The names of the frozen rewrite set, in application order.
///
/// Exposed so the rule set is a testable artefact rather than a comment:
/// adding, removing or reordering a rule changes the normal form and must
/// therefore fail a test before it changes a cache key.
#[must_use]
pub fn rewrite_rule_names() -> Vec<&'static str> {
    RULE_TABLE.iter().map(|entry| entry.0).collect()
}

// ---------------------------------------------------------------------------
// the mirror language
// ---------------------------------------------------------------------------

/// The base and direction of a `Trans` node, as one payload.
///
/// `Ord` is written out over [`TransKind::canonical_tag`] and [`Base::get`]
/// rather than derived, so that reordering [`TransKind`]'s variants - a pure
/// readability refactor - cannot change an e-class's node order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TransOp {
    kind: TransKind,
    base: Base,
}

impl Ord for TransOp {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.kind
            .canonical_tag()
            .cmp(&other.kind.canonical_tag())
            .then_with(|| self.base.get().cmp(&other.base.get()))
    }
}

impl PartialOrd for TransOp {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The e-graph mirror of [`BoundKind`], binarised.
///
/// # No `egg::Symbol`, anywhere
///
/// The payloads are [`Nat`], [`VarId`] and [`TransOp`], all of whose `Ord` and
/// `Hash` are content derived. `egg::Symbol` is `symbol_table::GlobalSymbol`,
/// whose `Ord` is an index into a **process-global** interner; since
/// `EGraph::rebuild` sorts each e-class's nodes with `sort_unstable` and
/// `Extractor::make_pass` takes the *first* minimum, an interner-ordered
/// payload would break every residual cost tie by directory-walk order.
///
/// # Why binary and not n-ary
///
/// `egg`'s pattern language has no variadic matching, so an n-ary `Sum` would
/// need one rule per arity. The n-ary shape is restored on the way out by
/// [`Bound::sum_checked`] and friends, which flatten and re-sort.
///
/// # Why the fold is to the **left**
///
/// `Nat::times` saturates and `omega` absorbs unconditionally, so `*` is
/// **not associative** on `N u {omega}`: `(2 * u64::MAX) * 0` is `omega` while
/// `2 * (u64::MAX * 0)` is `0`. `Bound::eval` folds an n-ary `Prod` from the
/// left over its canonically ordered operands, and `Const` sorts first, so a
/// left fold puts any zero literal innermost and reproduces `Bound::eval`
/// exactly. A right fold would not, and the mirror would then disagree with
/// the denotation it is supposed to preserve before a single rule had fired.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum BoundNode {
    /// A literal magnitude, possibly `omega`.
    Const(Nat),
    /// An input-size variable.
    Var(VarId),
    /// `a + b`.
    Add([Id; 2]),
    /// `max(a, b)`.
    Max([Id; 2]),
    /// `a * b`.
    Mul([Id; 2]),
    /// `base ^ a`, or `ceil(log_base(max(1, a)))`.
    Trans(TransOp, [Id; 1]),
}

/// The node's canonical tag.
///
/// Deliberately the same numbering as [`crate::BoundShape::canonical_tag`], and
/// written out for the same reason: it orders e-class nodes and prefixes the
/// extraction tie-break key, so Rust declaration order may not decide it.
const fn node_tag(node: &BoundNode) -> u8 {
    match node {
        BoundNode::Const(_) => 0,
        BoundNode::Var(_) => 1,
        BoundNode::Add(_) => 2,
        BoundNode::Max(_) => 3,
        BoundNode::Mul(_) => 4,
        BoundNode::Trans(_, _) => 5,
    }
}

/// The node's payload, in the same encoding [`Canonical::write_canonical`]
/// uses, so the tie-break key agrees with the canonical byte form.
fn write_payload(node: &BoundNode, out: &mut Vec<u8>) {
    match node {
        BoundNode::Const(magnitude) => magnitude.write_canonical(out),
        BoundNode::Var(var) => var.write_canonical(out),
        BoundNode::Add(_) | BoundNode::Max(_) | BoundNode::Mul(_) => {}
        BoundNode::Trans(op, _) => {
            op.kind.write_canonical(out);
            op.base.write_canonical(out);
        }
    }
}

impl Ord for BoundNode {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;

        let tags = node_tag(self).cmp(&node_tag(other));
        if tags != Ordering::Equal {
            return tags;
        }
        let payload = match (self, other) {
            (Self::Const(left), Self::Const(right)) => left.cmp(right),
            (Self::Var(left), Self::Var(right)) => left.cmp(right),
            (Self::Trans(left, _), Self::Trans(right, _)) => left.cmp(right),
            // Unreachable: the tags already agree, so the shapes do, and the
            // remaining shapes have no payload.
            _ => Ordering::Equal,
        };
        payload.then_with(|| self.children().cmp(other.children()))
    }
}

impl PartialOrd for BoundNode {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The operator and arity of a node, with the children erased.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum NodeDiscriminant {
    /// A literal magnitude.
    Const(Nat),
    /// An input-size variable.
    Var(VarId),
    /// `+`.
    Add,
    /// `max`.
    Max,
    /// `*`.
    Mul,
    /// `pow` or `log` at a given base.
    Trans(TransOp),
}

impl Language for BoundNode {
    type Discriminant = NodeDiscriminant;

    fn discriminant(&self) -> Self::Discriminant {
        match self {
            Self::Const(magnitude) => NodeDiscriminant::Const(*magnitude),
            Self::Var(var) => NodeDiscriminant::Var(var.clone()),
            Self::Add(_) => NodeDiscriminant::Add,
            Self::Max(_) => NodeDiscriminant::Max,
            Self::Mul(_) => NodeDiscriminant::Mul,
            Self::Trans(op, _) => NodeDiscriminant::Trans(*op),
        }
    }

    fn matches(&self, other: &Self) -> bool {
        // Operator and arity only, never the children - that is the trait's
        // contract, and the arity is a function of the discriminant here.
        self.discriminant() == other.discriminant()
    }

    fn children(&self) -> &[Id] {
        match self {
            Self::Const(_) | Self::Var(_) => &[],
            Self::Add(kids) | Self::Max(kids) | Self::Mul(kids) => kids,
            Self::Trans(_, kids) => kids,
        }
    }

    fn children_mut(&mut self) -> &mut [Id] {
        match self {
            Self::Const(_) | Self::Var(_) => &mut [],
            Self::Add(kids) | Self::Max(kids) | Self::Mul(kids) => kids,
            Self::Trans(_, kids) => kids,
        }
    }
}

// ---------------------------------------------------------------------------
// the extraction cost function
// ---------------------------------------------------------------------------

/// The extraction cost: tree size, then an exact content key.
///
/// # Integer valued, and `Ord`
///
/// `egg` compares costs with `partial_cmp(..).unwrap()` (`extract.rs:195`), a
/// panic inside a dependency that this workspace's `unwrap_used` lint cannot
/// see. An `f64` cost reaches it through a `NaN`; an [`Ord`] cost cannot,
/// because its `partial_cmp` is `Some(self.cmp(other))` by construction.
///
/// # No saturation-induced ties
///
/// `egg`'s own `AstSize` accumulates into a `usize` with `saturating_add`, so
/// two enormous distinct terms both reach `usize::MAX` and tie. The
/// accumulator here is a `u128` *and* the tie-break key carries the
/// distinction, so a tie on `size` is still resolved.
///
/// # A unique winner
///
/// `key` is a bottom-up, length-prefixed, self-delimiting encoding of the term
/// that would be extracted: the node's tag, its payload, then each child's
/// already-chosen key. Two costs are equal exactly when the terms they would
/// extract are identical, so the extracted term is unique even where two
/// e-nodes of one class tie. Nothing falls through to the language's `Ord`,
/// and therefore nothing falls through to `min_by`'s first-minimum rule.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TermCost {
    /// The number of nodes in the term, as a tree. Saturating, but see above.
    size: u128,
    /// The exact content key. `Arc` because `egg` clones costs constantly.
    key: Arc<[u8]>,
}

impl Ord for TermCost {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.size
            .cmp(&other.size)
            .then_with(|| self.key.cmp(&other.key))
    }
}

impl PartialOrd for TermCost {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        // Total by construction. This is the method `egg` unwraps.
        Some(self.cmp(other))
    }
}

/// The [`egg::CostFunction`] behind extraction. See [`TermCost`].
#[derive(Debug, Clone, Copy)]
struct NormalCost;

impl CostFunction<BoundNode> for NormalCost {
    type Cost = TermCost;

    fn cost<C>(&mut self, enode: &BoundNode, mut costs: C) -> Self::Cost
    where
        C: FnMut(Id) -> Self::Cost,
    {
        let mut key: Vec<u8> = Vec::new();
        key.push(node_tag(enode));
        write_payload(enode, &mut key);
        let mut size: u128 = 1;
        for child in enode.children() {
            let child_cost = costs(*child);
            size = size.saturating_add(child_cost.size);
            // Length prefixed, so the concatenation is self-delimiting and two
            // different child splits cannot produce the same byte run.
            let len = u64::try_from(child_cost.key.len()).unwrap_or(u64::MAX);
            key.extend_from_slice(&len.to_be_bytes());
            key.extend_from_slice(&child_cost.key);
        }
        TermCost {
            size,
            key: Arc::from(key.as_slice()),
        }
    }
}

// ---------------------------------------------------------------------------
// the frozen rewrite set
// ---------------------------------------------------------------------------

/// A pattern term, as a tree, before it is flattened into a [`PatternAst`].
///
/// Written as data rather than as a string so that no parse can fail: the
/// `rewrite!` macro and `Pattern::from_str` both end in an `unwrap`, and
/// `Var::from_str` is the one route by which an `egg::Symbol` would enter even
/// the pattern side of this module. [`Var::from_u32`] is a plain `u32` and
/// touches no interner.
enum Pat {
    /// A pattern variable, by index.
    Hole(u32),
    /// A literal finite magnitude.
    Lit(u64),
    /// `a + b`.
    Add(Box<Pat>, Box<Pat>),
    /// `max(a, b)`.
    Max(Box<Pat>, Box<Pat>),
    /// `a * b`.
    Mul(Box<Pat>, Box<Pat>),
}

/// `?a`.
fn a() -> Box<Pat> {
    Box::new(Pat::Hole(0))
}

/// `?b`.
fn b() -> Box<Pat> {
    Box::new(Pat::Hole(1))
}

/// `?c`.
fn c() -> Box<Pat> {
    Box::new(Pat::Hole(2))
}

/// A literal.
fn lit(value: u64) -> Box<Pat> {
    Box::new(Pat::Lit(value))
}

/// `a + b`.
fn add(left: Box<Pat>, right: Box<Pat>) -> Box<Pat> {
    Box::new(Pat::Add(left, right))
}

/// `max(a, b)`.
fn max(left: Box<Pat>, right: Box<Pat>) -> Box<Pat> {
    Box::new(Pat::Max(left, right))
}

/// `a * b`.
fn mul(left: Box<Pat>, right: Box<Pat>) -> Box<Pat> {
    Box::new(Pat::Mul(left, right))
}

/// Flattens a pattern tree into a [`PatternAst`], children first.
///
/// Infallible: every leaf is added before its parent, so `RecExpr::add`'s
/// debug assertion holds, and no string is parsed.
fn compile(pat: &Pat, ast: &mut PatternAst<BoundNode>) -> Id {
    match pat {
        Pat::Hole(index) => ast.add(ENodeOrVar::Var(Var::from_u32(*index))),
        Pat::Lit(value) => ast.add(ENodeOrVar::ENode(BoundNode::Const(Nat::Fin(*value)))),
        Pat::Add(left, right) => {
            let kids = [compile(left, ast), compile(right, ast)];
            ast.add(ENodeOrVar::ENode(BoundNode::Add(kids)))
        }
        Pat::Max(left, right) => {
            let kids = [compile(left, ast), compile(right, ast)];
            ast.add(ENodeOrVar::ENode(BoundNode::Max(kids)))
        }
        Pat::Mul(left, right) => {
            let kids = [compile(left, ast), compile(right, ast)];
            ast.add(ENodeOrVar::ENode(BoundNode::Mul(kids)))
        }
    }
}

/// One entry of the frozen rewrite set: a name, a left-hand side and a
/// right-hand side.
type RuleEntry = (&'static str, fn() -> Box<Pat>, fn() -> Box<Pat>);

/// The frozen rewrite set, in the order it is applied.
///
/// **Every rule preserves the ideal value**, so extraction is choosing between
/// terms that over-approximate the same true cost, and the cost function is not
/// a soundness surface.
///
/// It is tempting to state that more strongly — that every rule is an *exact
/// identity* — and that stronger claim stood here until the Gate 2 algebra
/// adversary disproved it in both directions:
///
/// ```text
/// widening:   a*b + a*c  ->  a*(b + c)     at a=0, b=c=2^63:  Fin(0) -> Omega
/// narrowing:  0*(x+z)*(z+z) -> 0*z*(x+z)   at x=0, z=2^63:    Omega -> Fin(0)
/// ```
///
/// Both directions occur because saturating arithmetic is neither associative
/// nor distributive once a value leaves `u64`, and `Bound::eval` cannot tell
/// "no finite bound" from "a finite magnitude left the carrier" — both are
/// `omega`. Rewriting therefore moves *between over-approximations*, and can
/// land on a tighter one or a looser one.
///
/// The conclusion survives on the weaker premise, which is the one that
/// actually matters: no rule takes a term below its ideal value, so no
/// extraction can report a bound the program exceeds. Verified over a grid that
/// deliberately leaves the exact regime — see
/// `tests/gate2_algebra_adversary.rs`.
///
/// The two rules the algebra would *not* survive are called out here because
/// their absence is load bearing:
///
/// * **`?a * 0 -> 0` is unsound.** Variables range over `N u {omega}` and
///   `omega` absorbs unconditionally, so the product is `omega` at
///   `?a = omega`. `Bound::prod` refuses the same fold for the same reason.
/// * **`log_2(x) -> log_4(x)` is unsound** (the base is anti-monotone); only
///   the loosening direction would be admissible, and this module does not
///   loosen.
///
/// The associativity rules for `*` deserve a word. Saturating multiplication
/// is **not** associative on `N u {omega}` - `(2 * u64::MAX) * 0` is `omega`
/// and `2 * (u64::MAX * 0)` is `0` - so re-association can change what
/// `Bound::eval` reports. It cannot change it *downwards past the true
/// denotation*: a zero factor makes the ideal product zero unless an `omega`
/// is present, and an `omega` propagates through every grouping, so every
/// grouping lies between the ideal value and `omega`. Re-association is
/// therefore sound (never an under-approximation) and exact wherever nothing
/// saturates, which is exactly what `tests/properties/normal_form.rs`
/// asserts.
static RULE_TABLE: &[RuleEntry] = &[
    // `+` is associative and commutative on `N u {omega}`: saturation lands on
    // `omega`, which absorbs, so no grouping can disagree.
    ("add-comm", || add(a(), b()), || add(b(), a())),
    (
        "add-assoc",
        || add(add(a(), b()), c()),
        || add(a(), add(b(), c())),
    ),
    (
        "add-assoc-rev",
        || add(a(), add(b(), c())),
        || add(add(a(), b()), c()),
    ),
    // `0` is the additive identity for every `x`, including `omega`.
    ("add-zero", || add(a(), lit(0)), a),
    ("mul-comm", || mul(a(), b()), || mul(b(), a())),
    (
        "mul-assoc",
        || mul(mul(a(), b()), c()),
        || mul(a(), mul(b(), c())),
    ),
    (
        "mul-assoc-rev",
        || mul(a(), mul(b(), c())),
        || mul(mul(a(), b()), c()),
    ),
    // `1` is the multiplicative identity for every `x`, including `omega`.
    // `?a * 0 -> 0` is **not** here; see this table's documentation.
    ("mul-one", || mul(a(), lit(1)), a),
    ("max-comm", || max(a(), b()), || max(b(), a())),
    (
        "max-assoc",
        || max(max(a(), b()), c()),
        || max(a(), max(b(), c())),
    ),
    (
        "max-assoc-rev",
        || max(a(), max(b(), c())),
        || max(max(a(), b()), c()),
    ),
    // Idempotence of `max` - LAN-58 AC2. `MaxTerms` deduplicates the *syntactic*
    // duplicates the constructors can see; this catches the ones only the
    // e-graph can, where two operands become equal after other rules fire.
    ("max-idem", || max(a(), a()), a),
    // Every magnitude is at least zero, so zero is the identity of `max`.
    ("max-zero", || max(a(), lit(0)), a),
    // Distribution of `*` over `+` - LAN-58 AC2 - in both directions, so that
    // the e-graph proves the equality and the cost function, not the rule
    // direction, chooses which side is the normal form.
    (
        "mul-distributes-over-add",
        || mul(a(), add(b(), c())),
        || add(mul(a(), b()), mul(a(), c())),
    ),
    (
        "add-factors-through-mul",
        || add(mul(a(), b()), mul(a(), c())),
        || mul(a(), add(b(), c())),
    ),
];

/// The compiled rewrite set, built once.
static RULES: OnceLock<Vec<Rewrite<BoundNode, ()>>> = OnceLock::new();

/// The compiled rewrite set.
///
/// Empty only if `Rewrite::new` refused an entry - which it does exactly when
/// a right-hand side mentions a variable the left-hand side does not bind, and
/// which [`RULE_TABLE`] does not do. `normalise_with` turns an empty set into
/// a hard error rather than into a silent identity normalisation, and
/// `rules_all_compile` fails first.
fn rewrite_rules() -> &'static [Rewrite<BoundNode, ()>] {
    RULES.get_or_init(|| {
        let mut built = Vec::with_capacity(RULE_TABLE.len());
        for (name, lhs, rhs) in RULE_TABLE {
            let mut searcher = PatternAst::default();
            compile(&lhs(), &mut searcher);
            let mut applier = PatternAst::default();
            compile(&rhs(), &mut applier);
            match Rewrite::new(*name, Pattern::new(searcher), Pattern::new(applier)) {
                Ok(rule) => built.push(rule),
                // One bad entry invalidates the whole frozen set: a partial
                // rule set is a different normal form, and a different normal
                // form must never be produced silently.
                Err(_) => return Vec::new(),
            }
        }
        built
    })
}

// ---------------------------------------------------------------------------
// Bound <-> RecExpr
// ---------------------------------------------------------------------------

/// Lowers a [`Bound`] into the mirror language, sharing nodes.
///
/// Memoised on the `Bound` itself - whose `Hash` is an O(1) content
/// fingerprint - so a shared DAG is lowered once per distinct subterm rather
/// than once per path. Recursion is bounded by [`crate::MAX_DEPTH`], which
/// every inhabitant of `Bound` satisfies by construction.
fn to_recexpr(bound: &Bound) -> (RecExpr<BoundNode>, Id) {
    let mut expr = RecExpr::default();
    let mut memo: HashMap<Bound, Id> = HashMap::new();
    let root = lower(bound, &mut expr, &mut memo);
    (expr, root)
}

/// Lowers one node, left-folding the n-ary shapes. See [`BoundNode`] for why
/// the fold is to the left.
fn lower(bound: &Bound, expr: &mut RecExpr<BoundNode>, memo: &mut HashMap<Bound, Id>) -> Id {
    if let Some(known) = memo.get(bound) {
        return *known;
    }
    let id = match bound.kind() {
        BoundKind::Const(magnitude) => expr.add(BoundNode::Const(*magnitude)),
        BoundKind::Var(var) => expr.add(BoundNode::Var(var.clone())),
        BoundKind::Sum(operands) => fold_left(operands.as_slice(), expr, memo, BoundNode::Add),
        BoundKind::Max(operands) => fold_left(operands.as_slice(), expr, memo, BoundNode::Max),
        BoundKind::Prod(operands) => fold_left(operands.as_slice(), expr, memo, BoundNode::Mul),
        BoundKind::Trans { kind, base, arg } => {
            let child = lower(arg, expr, memo);
            expr.add(BoundNode::Trans(
                TransOp {
                    kind: *kind,
                    base: *base,
                },
                [child],
            ))
        }
    };
    memo.insert(bound.clone(), id);
    id
}

/// Left-folds an operand list into binary nodes.
///
/// The operand list always has at least two entries (`Terms` and `MaxTerms`
/// both guarantee it), so the `omega` fallback below is unreachable; it exists
/// so that this function has no panicking path at all.
fn fold_left(
    operands: &[Bound],
    expr: &mut RecExpr<BoundNode>,
    memo: &mut HashMap<Bound, Id>,
    node: fn([Id; 2]) -> BoundNode,
) -> Id {
    let mut ids: Vec<Id> = Vec::with_capacity(operands.len());
    for operand in operands {
        ids.push(lower(operand, expr, memo));
    }
    let mut walk = ids.into_iter();
    let Some(first) = walk.next() else {
        return expr.add(BoundNode::Const(Nat::OMEGA));
    };
    let mut accumulator = first;
    for next in walk {
        accumulator = expr.add(node([accumulator, next]));
    }
    accumulator
}

/// Rebuilds a [`Bound`] from an extracted expression, **through the smart
/// constructors**.
///
/// This is what restores flatness, the canonical operand order, constant
/// folding, `omega` absorption and `Max` deduplication: the e-graph works in a
/// binary, unordered world and the algebra does not. The `_checked`
/// constructors are used rather than the total ones so that a term that
/// outgrew [`crate::MAX_DEPTH`] is reported rather than silently widened to
/// `omega`.
///
/// # Errors
///
/// [`BoundError::DepthExceeded`], [`BoundError::ArityExceeded`],
/// [`BoundError::NodeBudgetExceeded`], or
/// [`BoundError::NonDeterministicNormalisation`] if the extracted expression
/// is not a well-formed DAG - which `egg`'s `build_recexpr` guarantees, so it
/// is a hard error rather than a fallback.
fn from_recexpr(expr: &RecExpr<BoundNode>) -> Result<Bound, BoundError> {
    let nodes = expr.as_ref();
    let mut built: Vec<Bound> = Vec::with_capacity(nodes.len());
    for node in nodes {
        let rebuilt = match node {
            BoundNode::Const(magnitude) => Bound::magnitude(*magnitude),
            BoundNode::Var(var) => Bound::var(var.symbol().clone()),
            BoundNode::Add(kids) => Bound::sum_checked(pair(&built, kids)?)?,
            BoundNode::Max(kids) => Bound::max_of_checked(pair(&built, kids)?)?,
            BoundNode::Mul(kids) => Bound::prod_checked(pair(&built, kids)?)?,
            BoundNode::Trans(op, kids) => {
                let child = child_of(&built, kids[0])?;
                match op.kind {
                    TransKind::Pow => Bound::pow_checked(op.base, child)?,
                    TransKind::Log => Bound::log_checked(op.base, child)?,
                }
            }
        };
        built.push(rebuilt);
    }
    built.last().cloned().ok_or(malformed_extraction())
}

/// The hard error for an extracted expression that is not a well-formed DAG.
fn malformed_extraction() -> BoundError {
    BoundError::NonDeterministicNormalisation {
        reason: "the extracted expression was not a well-formed DAG",
    }
}

/// One already-rebuilt child.
fn child_of(built: &[Bound], id: Id) -> Result<Bound, BoundError> {
    built
        .get(usize::from(id))
        .cloned()
        .ok_or_else(malformed_extraction)
}

/// Both already-rebuilt children of a binary node.
fn pair(built: &[Bound], kids: &[Id; 2]) -> Result<[Bound; 2], BoundError> {
    Ok([child_of(built, kids[0])?, child_of(built, kids[1])?])
}

#[cfg(test)]
mod tests {
    use super::{
        BoundNode, Nat, NormalCost, NormaliserStop, RULE_TABLE, TermCost, TransOp, node_tag,
        rewrite_rule_names, rewrite_rules,
    };
    use crate::{base::Base, trans_kind::TransKind, var_id::VarId};
    use egg::{CostFunction, Id, Language, StopReason};

    /// The node tags prefix the extraction tie-break key, so reordering the
    /// variants of [`BoundNode`] would silently change every normal form.
    #[test]
    fn node_tags_are_pinned() {
        assert_eq!(node_tag(&BoundNode::Const(Nat::ZERO)), 0);
        assert_eq!(node_tag(&BoundNode::Var(VarId::new("x"))), 1);
        assert_eq!(node_tag(&BoundNode::Add([Id::from(0), Id::from(0)])), 2);
        assert_eq!(node_tag(&BoundNode::Max([Id::from(0), Id::from(0)])), 3);
        assert_eq!(node_tag(&BoundNode::Mul([Id::from(0), Id::from(0)])), 4);
        assert_eq!(
            node_tag(&BoundNode::Trans(
                TransOp {
                    kind: TransKind::Pow,
                    base: Base::TWO
                },
                [Id::from(0)]
            )),
            5
        );
    }

    /// `Ord` must agree with `Eq`: `cmp` may return `Equal` only for nodes
    /// that really are equal.
    ///
    /// `EGraph::rebuild` sorts each e-class's nodes with `sort_unstable` and
    /// then `dedup`s them, and extraction takes the *first* minimum of that
    /// order. An order that calls two distinct nodes equal is therefore an
    /// order in which the e-class layout - and so the extracted term - depends
    /// on the sequence the nodes happened to be inserted in.
    #[test]
    fn the_language_order_agrees_with_equality() {
        let kids = [Id::from(0), Id::from(1)];
        let nodes = [
            BoundNode::Const(Nat::ZERO),
            BoundNode::Const(Nat::ONE),
            BoundNode::Const(Nat::OMEGA),
            BoundNode::Var(VarId::new("x0")),
            BoundNode::Var(VarId::new("x1")),
            BoundNode::Add(kids),
            BoundNode::Add([Id::from(0), Id::from(2)]),
            BoundNode::Max(kids),
            BoundNode::Mul(kids),
            BoundNode::Trans(
                TransOp {
                    kind: TransKind::Pow,
                    base: Base::TWO,
                },
                [Id::from(0)],
            ),
            BoundNode::Trans(
                TransOp {
                    kind: TransKind::Log,
                    base: Base::TWO,
                },
                [Id::from(0)],
            ),
            BoundNode::Trans(
                TransOp {
                    kind: TransKind::Log,
                    base: Base::TEN,
                },
                [Id::from(0)],
            ),
        ];
        for (i, left) in nodes.iter().enumerate() {
            for (j, right) in nodes.iter().enumerate() {
                let ordering = left.cmp(right);
                assert_eq!(
                    ordering == core::cmp::Ordering::Equal,
                    i == j,
                    "node {i} and node {j} compare Equal but are not the same node"
                );
                assert_eq!(
                    ordering,
                    right.cmp(left).reverse(),
                    "the order is not antisymmetric at {i} and {j}"
                );
                assert_eq!(left.partial_cmp(right), Some(ordering));
            }
        }
    }

    /// The `Trans` payload orders by [`TransKind::canonical_tag`] and the
    /// base, both written out, so reordering `TransKind`'s variants cannot
    /// move an e-class's node order.
    #[test]
    fn the_trans_payload_orders_by_content() {
        let pow_two = TransOp {
            kind: TransKind::Pow,
            base: Base::TWO,
        };
        let log_two = TransOp {
            kind: TransKind::Log,
            base: Base::TWO,
        };
        let log_ten = TransOp {
            kind: TransKind::Log,
            base: Base::TEN,
        };
        assert_eq!(pow_two.cmp(&log_two), core::cmp::Ordering::Less);
        assert_eq!(log_two.cmp(&log_ten), core::cmp::Ordering::Less);
        assert_eq!(pow_two.cmp(&pow_two), core::cmp::Ordering::Equal);
        // `partial_cmp` is what a generic `<` would reach; it must be total.
        for left in [pow_two, log_two, log_ten] {
            for right in [pow_two, log_two, log_ten] {
                assert_eq!(left.partial_cmp(&right), Some(left.cmp(&right)));
            }
        }
    }

    /// Every entry of the frozen table must compile, or the whole set is
    /// discarded and `normalise_with` refuses to run at all.
    #[test]
    fn every_frozen_rule_compiles() {
        assert_eq!(
            rewrite_rules().len(),
            RULE_TABLE.len(),
            "a frozen rewrite rule failed to compile; the whole set is then discarded"
        );
        assert_eq!(rewrite_rule_names().len(), RULE_TABLE.len());
    }

    /// The cost of a node, given a fixed cost for each child.
    fn cost_of(node: &BoundNode, child: &TermCost) -> TermCost {
        NormalCost.cost(node, |_| child.clone())
    }

    /// A leaf cost, for use as a child.
    fn leaf(node: &BoundNode) -> TermCost {
        cost_of(
            node,
            &TermCost {
                size: 0,
                key: std::sync::Arc::from([].as_slice()),
            },
        )
    }

    /// `TermCost::partial_cmp` is the method `egg` unwraps at
    /// `extract.rs:195`. It must never be `None` - that is the entire reason
    /// this cost is an integer `Ord` and not an `f64`.
    #[test]
    fn the_cost_order_is_total() {
        let one = leaf(&BoundNode::Const(Nat::Fin(1)));
        let two = leaf(&BoundNode::Const(Nat::Fin(2)));
        let unbounded = leaf(&BoundNode::Const(Nat::OMEGA));
        for a in [&one, &two, &unbounded] {
            for b in [&one, &two, &unbounded] {
                assert_eq!(
                    a.partial_cmp(b),
                    Some(a.cmp(b)),
                    "partial_cmp must never be None; egg unwraps it"
                );
            }
        }
        assert_eq!(one.partial_cmp(&one), Some(core::cmp::Ordering::Equal));
    }

    /// Distinct leaves must not tie, or extraction falls through to the
    /// language's `Ord` and `min_by`'s first-minimum rule.
    #[test]
    fn distinct_leaves_have_distinct_costs() {
        let x = leaf(&BoundNode::Var(VarId::new("x0")));
        let y = leaf(&BoundNode::Var(VarId::new("x1")));
        assert_ne!(x, y);
        assert_eq!(x.size, y.size, "the tie is on size; the key must break it");
        assert_ne!(x.key, y.key);
    }

    /// Two nodes of the same shape, the same size and the same children must
    /// still be told apart when their *payloads* differ, and two nodes of
    /// different shapes must be told apart even when everything else agrees.
    ///
    /// This is the "unique winner" half of acceptance criterion 6: nothing may
    /// be left for `min_by` to decide.
    #[test]
    fn the_tie_break_separates_everything_the_size_does_not() {
        let child = leaf(&BoundNode::Var(VarId::new("x0")));
        let kids = [Id::from(0), Id::from(1)];
        let costs = [
            cost_of(&BoundNode::Add(kids), &child),
            cost_of(&BoundNode::Max(kids), &child),
            cost_of(&BoundNode::Mul(kids), &child),
            cost_of(
                &BoundNode::Trans(
                    TransOp {
                        kind: TransKind::Pow,
                        base: Base::TWO,
                    },
                    [Id::from(0)],
                ),
                &child,
            ),
            cost_of(
                &BoundNode::Trans(
                    TransOp {
                        kind: TransKind::Log,
                        base: Base::TWO,
                    },
                    [Id::from(0)],
                ),
                &child,
            ),
            cost_of(
                &BoundNode::Trans(
                    TransOp {
                        kind: TransKind::Log,
                        base: Base::TEN,
                    },
                    [Id::from(0)],
                ),
                &child,
            ),
        ];
        for (i, left) in costs.iter().enumerate() {
            for (j, right) in costs.iter().enumerate() {
                assert_eq!(
                    i == j,
                    left == right,
                    "cost {i} and cost {j} must differ exactly when the nodes do"
                );
            }
        }
    }

    /// `egg` requires the cost of a node to exceed every child's cost, and a
    /// `u128` accumulator is what stops two enormous distinct terms from both
    /// reaching the top and tying there.
    #[test]
    fn the_cost_is_monotone_in_its_children() {
        let child = TermCost {
            size: 41,
            key: std::sync::Arc::from([7u8].as_slice()),
        };
        let parent = cost_of(&BoundNode::Add([Id::from(0), Id::from(1)]), &child);
        assert!(parent.size > child.size);
        assert_eq!(parent.size, 83, "one for the node, and both children");

        let huge = TermCost {
            size: u128::MAX,
            key: std::sync::Arc::from([1u8].as_slice()),
        };
        let saturated = cost_of(&BoundNode::Add([Id::from(0), Id::from(1)]), &huge);
        assert_eq!(saturated.size, u128::MAX, "the accumulator saturates");
        let other = TermCost {
            size: u128::MAX,
            key: std::sync::Arc::from([2u8].as_slice()),
        };
        assert_ne!(
            saturated,
            cost_of(&BoundNode::Add([Id::from(0), Id::from(1)]), &other),
            "two saturated sizes must still be separated by the tie-break key"
        );
    }

    /// A wall-clock stop is the whole reason [`crate::NormaliserBudget`]
    /// exists: it makes the extracted term a function of machine load, so it
    /// may never be reported as a normalised bound.
    #[test]
    fn a_wall_clock_stop_is_refused() {
        assert!(NormaliserStop::classify(Some(&StopReason::TimeLimit(5.0))).is_err());
        assert!(NormaliserStop::classify(Some(&StopReason::Other("x".to_owned()))).is_err());
        assert!(NormaliserStop::classify(None).is_err());
    }

    /// The three count-based reasons must survive the round trip, or a
    /// perfectly reproducible run would be reported as a tool error.
    #[test]
    fn every_count_based_stop_is_accepted() {
        assert_eq!(
            NormaliserStop::classify(Some(&StopReason::Saturated)),
            Ok(NormaliserStop::Saturated)
        );
        assert_eq!(
            NormaliserStop::classify(Some(&StopReason::IterationLimit(60))),
            Ok(NormaliserStop::IterationLimit)
        );
        assert_eq!(
            NormaliserStop::classify(Some(&StopReason::NodeLimit(100_000))),
            Ok(NormaliserStop::NodeLimit)
        );
    }

    /// The names reach diagnostics, so they are written out rather than
    /// derived from the Rust identifiers.
    #[test]
    fn the_stop_names_are_pinned() {
        assert_eq!(NormaliserStop::Saturated.as_str(), "saturated");
        assert_eq!(NormaliserStop::IterationLimit.as_str(), "iteration-limit");
        assert_eq!(NormaliserStop::NodeLimit.as_str(), "node-limit");
    }

    /// `Language::children` must agree with the arity each shape declares.
    #[test]
    fn children_agree_with_arity() {
        assert_eq!(BoundNode::Const(Nat::ZERO).children().len(), 0);
        assert_eq!(BoundNode::Var(VarId::new("x")).children().len(), 0);
        assert_eq!(
            BoundNode::Add([Id::from(0), Id::from(1)]).children().len(),
            2
        );
        assert_eq!(
            BoundNode::Trans(
                TransOp {
                    kind: TransKind::Log,
                    base: Base::TWO
                },
                [Id::from(0)]
            )
            .children()
            .len(),
            1
        );
    }
}
