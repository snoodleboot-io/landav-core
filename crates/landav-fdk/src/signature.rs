//! [`Signature`] - one row of a signature pack.

use serde::Deserialize;

use crate::{cost_class::CostClass, result_length::ResultLength};

/// What a pack declares about one callee.
///
/// # Every field is a claim somebody has to be willing to defend
///
/// A signature is not a hint and it is not a heuristic: a frontend that
/// resolves a call against one of these stops emitting a hole for it, and the
/// bound the user reads is only as sound as the row. So the row carries the
/// argument as well as the answer - see [`Self::why`], which is required and
/// which is the field that makes a later contributor argue rather than append.
///
/// # The four questions, and why none of them can be folded into another
///
/// * [`Self::cost`] - does the callee's cost grow with anything the analysis
///   can see?
/// * [`Self::rebinds_locals`] - can it change what a local name of the
///   *caller's* frame denotes?
/// * [`Self::mutates_arguments`] - can it change an object reachable from its
///   arguments?
/// * [`Self::result_length`] - how many values does its result hold?
///
/// The second is the one that buys coverage. A region in this analysis forgets
/// every value the caller supplied, so a type test above a loop costs that loop
/// its entire trip count; a callee that cannot rebind a local gives the loop
/// back. The third is what stops that going too far: `isinstance` may run an
/// `__instancecheck__`, which is user code and may call `items.append(...)`, so
/// a length read on entry is not the length the loop below walks. Collapsing
/// the two into one flag would either lose the coverage or publish a bound the
/// program exceeds.
///
/// The fourth is a claim about the callee's **result** rather than about the
/// work it did, and it is the one that must never be read as discharging the
/// first: `sorted` yields exactly `len(argument)` values and costs `n log n`.
/// See [`ResultLength`], and see the two accessors on
/// [`crate::SignaturePack`] - one per claim, with a different admissibility
/// gate on each, so the separation is stated in the API rather than in a
/// comment.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    /// The callee's name, as the source writes it.
    ///
    /// A bare name (`isinstance`) or a bare attribute (`append`); a frontend
    /// matches whichever of the two it can see at the call site. Matching by
    /// name is the caveat this whole mechanism carries - see
    /// [`crate::SignaturePack`], where the shadowing rule lives.
    pub callee: String,
    /// What the callee costs.
    pub cost: CostClass,
    /// Whether calling it can rebind a local of the caller's frame.
    pub rebinds_locals: bool,
    /// Whether calling it can mutate an object reachable from its arguments.
    pub mutates_arguments: bool,
    /// How many values its result holds, in terms of argument 0.
    ///
    /// Omitted rows declare [`ResultLength::Unknown`], which confers nothing:
    /// the pack is an allowlist for this claim exactly as it is for the cost.
    #[serde(default)]
    pub result_length: ResultLength,
    /// Why this row says what it says.
    ///
    /// Required, and required on the refusing rows as much as the admitting
    /// ones. A table of names and booleans is a table anybody can add a line to;
    /// a table where every line carries its argument is one where adding a line
    /// means writing the argument, and deleting a line means answering one.
    pub why: String,
}

impl Signature {
    /// Whether a frontend may resolve a call to this callee.
    ///
    /// A row is resolvable when its cost class is and when it cannot rebind a
    /// local. Both halves are needed and neither implies the other: a bounded
    /// cost that rebinds the caller's names would still erase every value the
    /// analysis knew, and a callee that rebinds nothing but blocks on a lock
    /// has no bound to declare.
    #[must_use]
    pub const fn is_resolvable(&self) -> bool {
        self.cost.is_resolvable() && !self.rebinds_locals
    }
}
