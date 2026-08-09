//! [`BoundError`] - caller misuse and malformed input.

use crate::{bound_shape::BoundShape, var_id::VarId};

/// Malformed *input to the algebra*, or a violation of a frozen contract.
///
/// Deliberately **separate from blame**. `BoundError` is caller misuse and
/// ends in a message; blame is "the analysed program did not tell us" and ends
/// in a partial bound. Collapsing them puts "we could not size `n`" on the `?`
/// path, and the `?` path does not end in a bound.
///
/// `#[non_exhaustive]` - unlike [`crate::BoundKind`] - because adding an error
/// variant should not break consumers, whereas adding a seventh constructor
/// should.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BoundError {
    /// An n-ary node was given fewer than two operands.
    #[error("`{op}` needs at least two operands, got {got}")]
    Underfull {
        /// Which operator.
        op: BoundShape,
        /// How many operands were supplied.
        got: usize,
    },

    /// A base below 2 was supplied.
    #[error("base must be at least 2, got {got}")]
    BaseTooSmall {
        /// The rejected base.
        got: u32,
    },

    /// A variable had no value and the caller asked for a total valuation.
    #[error("variable `{var}` has no value in this valuation")]
    UnboundVariable {
        /// The least absent variable, in [`VarId`] order.
        var: VarId,
    },

    /// A bound exceeded [`crate::MAX_DEPTH`].
    #[error("bound nesting exceeded the {limit} level limit")]
    DepthExceeded {
        /// The limit that was exceeded.
        limit: u16,
    },

    /// A bound's DAG exceeded [`crate::MAX_NODES`].
    #[error("bound exceeded the {limit} node budget")]
    NodeBudgetExceeded {
        /// The limit that was exceeded.
        limit: u32,
    },

    /// An n-ary node was given more operands than [`crate::MAX_NODES`] allows
    /// nodes.
    ///
    /// Separate from [`BoundError::DepthExceeded`] and
    /// [`BoundError::NodeBudgetExceeded`] because neither of them can see it:
    /// `b = op([b, b])` flattens the child into the parent, so the operand
    /// vector *doubles* while the depth stays at 2 and the DAG stays at two
    /// distinct nodes. Forty such calls ask for a `Vec` of `2^40` handles, and
    /// a `Vec` that cannot grow calls `handle_alloc_error`, which **aborts** -
    /// a failure mode `#![forbid(unsafe_code)]`, `unwrap_used` and `panic`
    /// cannot see.
    #[error("`{op}` was given {got} operands, exceeding the {limit} operand budget")]
    ArityExceeded {
        /// Which operator.
        op: BoundShape,
        /// How many operands were supplied, after flattening.
        got: u64,
        /// The limit that was exceeded.
        limit: u32,
    },

    /// A term's expansion as a *tree* exceeds [`crate::MAX_NODES`].
    ///
    /// Distinct from [`BoundError::NodeBudgetExceeded`], which counts the
    /// *distinct* nodes of the DAG. A wire document of fifty nodes, inside
    /// every other budget, can rebuild a term whose tree is `2^24` nodes;
    /// [`crate::Bound::canonical_bytes`] and the other observers are
    /// memoised over the shared nodes, but [`std::fmt::Display`] still
    /// renders the tree, so untrusted input is measured against the tree it
    /// is about to materialise rather than against the document that carries
    /// it.
    #[error("bound expands to {got} tree nodes, exceeding the {limit} node budget")]
    TreeSizeExceeded {
        /// The tree size, saturating at `u64::MAX`.
        got: u64,
        /// The limit that was exceeded.
        limit: u32,
    },

    /// `--resource` was given a value that is not registered.
    #[error("unknown resource `{got}`; registered resources are: {}", known.join(", "))]
    UnknownResource {
        /// The rejected value.
        got: String,
        /// Every registered resource, generated from the registry.
        known: Vec<&'static str>,
    },

    /// A derivation tried to publish an `omega`-bearing bound with an empty
    /// blame ledger.
    ///
    /// This is the mechanical form of "failure must carry blame". It is a
    /// **tool error**, not a clean report: an unbounded result that names
    /// nothing unaccounted for is a bug in the caller, and reporting it as
    /// `Proved` or as an unblamed `Partial` would be the exact failure the
    /// blame machinery exists to prevent.
    #[error("an unbounded result was published with no blame recorded")]
    UnblamedOmega,

    /// A wire document declared a version this build cannot read.
    #[error("wire version {got} is not supported (this build reads {supported})")]
    WireVersionUnsupported {
        /// The document's version.
        got: u16,
        /// The version this build reads.
        supported: u16,
    },

    /// A wire document was structurally invalid - a child index out of range,
    /// a forward reference, an unreachable node, or a root out of range.
    #[error("malformed wire document: {detail}")]
    WireMalformed {
        /// What was wrong.
        detail: &'static str,
    },

    /// The normaliser stopped for a reason that is not reproducible.
    ///
    /// Raised when an e-graph run stops on anything other than saturation, the
    /// iteration limit or the node limit - in particular on a wall-clock
    /// timeout, which makes the normal form a function of machine load. See
    /// [`crate::NormaliserBudget`].
    #[error("normalisation stopped non-deterministically: {reason}")]
    NonDeterministicNormalisation {
        /// The stop reason reported by the engine.
        reason: &'static str,
    },
}

impl core::fmt::Display for BoundShape {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
