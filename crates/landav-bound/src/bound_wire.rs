//! [`BoundWire`] - the versioned, explicit-DAG serialisation form.

use serde::{Deserialize, Serialize};

use crate::wire_node::WireNode;

/// The serialisation form of a [`crate::Bound`].
///
/// [`crate::Bound`] itself implements neither `Serialize` nor `Deserialize`.
/// All traffic goes through this type, for four reasons that a derived impl on
/// the algebra cannot deliver:
///
/// 1. **No recursion during parsing.** A derived `Deserialize` recurses
///    through the child pointers and overflows the stack - an *abort*, which
///    is worse than a panic - before any of this crate's code runs, and the
///    validating `try_from` shims run *after* that recursion. The node table
///    is a flat `Vec`, so parsing is iterative and the depth limit is applied
///    during rebuild.
/// 2. **Sharing survives.** serde's `rc` feature serialises `Arc` by value and
///    does not preserve sharing; this form is an explicit DAG, so it does not
///    need that feature at all.
/// 3. **Re-canonicalisation, not validation.** [`crate::Bound::try_from_wire`]
///    rebuilds bottom-up through the smart constructors, so a document cannot
///    carry a term this crate could not itself construct - no unsorted `Sum`,
///    no undropped `Const(0)`, no unflattened nesting, no arity-1 node.
/// 4. **A pinned wire vocabulary.** Every tag is explicit; see [`WireNode`].
///
/// A golden test must pin the JSON of a representative term, so that a rename
/// fails CI here rather than in the platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BoundWire {
    /// The wire-format version; see [`crate::WIRE_VERSION`].
    #[serde(rename = "v")]
    pub version: u16,
    /// The node table, topologically ordered: every child index is strictly
    /// less than the index of the node referencing it.
    #[serde(rename = "nodes")]
    pub nodes: Vec<WireNode>,
    /// The index of the root node in [`BoundWire::nodes`].
    #[serde(rename = "root")]
    pub root: u32,
}
