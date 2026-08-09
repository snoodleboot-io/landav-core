//! [`WireNode`] - one entry in the explicit-DAG wire form.

use serde::{Deserialize, Serialize};

/// One node of a [`crate::BoundWire`] DAG.
///
/// Children are `u32` indices into the wire's node table, and the table is
/// topologically ordered (every child index is strictly less than its
/// parent's). That is what preserves sharing on the wire: a bound built as
/// `b0 = x + 1; b1 = b0 * b0; b2 = b1 * b1; ...` is 31 shared nodes in memory
/// and would be `2^30` nodes if the wire form were the derived tree.
///
/// # Wire hygiene, all of it deliberate
///
/// * Every variant and every field is pinned with `#[serde(rename)]`, so
///   renaming a Rust identifier - a readability refactor with no compile error
///   anywhere - cannot break the hosted platform's ingest.
/// * Struct variants with named fields throughout: a newtype variant silently
///   changes JSON shape the moment a second field is added.
/// * `deny_unknown_fields`, so an older reader rejects a newer document
///   loudly rather than dropping a field.
/// * The version lives on [`crate::BoundWire`], not here.
/// * The free-variable summary and the depth are **not** on the wire. They are
///   derived data and are recomputed on rebuild, which is the second,
///   independent defence against a stale summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireNode {
    /// A finite literal.
    #[serde(rename = "const")]
    Const {
        /// The magnitude, or `None` for `omega`.
        #[serde(rename = "fin")]
        fin: Option<u64>,
    },
    /// A variable.
    #[serde(rename = "var")]
    Var {
        /// The frontend-supplied name.
        #[serde(rename = "name")]
        name: String,
    },
    /// `t0 + t1 + ...`.
    #[serde(rename = "sum")]
    Sum {
        /// Indices of the operands, two or more.
        #[serde(rename = "args")]
        args: Vec<u32>,
    },
    /// `max(t0, t1, ...)`.
    #[serde(rename = "max")]
    Max {
        /// Indices of the operands, two or more, pairwise distinct.
        #[serde(rename = "args")]
        args: Vec<u32>,
    },
    /// `t0 * t1 * ...`.
    #[serde(rename = "prod")]
    Prod {
        /// Indices of the operands, two or more.
        #[serde(rename = "args")]
        args: Vec<u32>,
    },
    /// `base ^ arg` or `ceil(log_base(max(1, arg)))`.
    #[serde(rename = "trans")]
    Trans {
        /// Which of the adjoint pair.
        #[serde(rename = "kind")]
        kind: crate::trans_kind::TransKind,
        /// The base, `>= 2`.
        #[serde(rename = "base")]
        base: u32,
        /// Index of the single operand.
        #[serde(rename = "arg")]
        arg: u32,
    },
}
