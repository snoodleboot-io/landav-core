//! [`CostClass`] - how a signature row says what its callee costs.

use serde::Deserialize;

/// The shape of a callee's cost, as a signature row declares it.
///
/// # Why a class and not a number
///
/// The question a signature has to answer is not "how many microseconds" but
/// "does this grow with anything the analysis can see". A row saying
/// `cost = "constant"` is making a claim that can be argued with; a row saying
/// `cost = 7` invites a discussion about the 7, which is the discussion that
/// does not matter. The constant factor is a calibration profile's business
/// (`E-003`), and the shape is this table's.
///
/// # Two classes are admissible, and the others are here to be refused
///
/// [`Self::Constant`] and [`Self::AmortisedConstant`] are the classes a
/// frontend may resolve. The rest exist so that a row for `join` or `sorted`
/// can be *written down and rejected* - carrying its `why` and its
/// measurement - rather than being absent and looking like an oversight. A
/// pack is a place to record a decision, and half the decisions are refusals.
///
/// `LAN-11`'s size analysis is what makes the other classes resolvable. Until
/// it lands, a callee whose cost is a function of an argument's size stays a
/// hole, and a row declaring one is a documented refusal rather than a bound
/// the program exceeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum CostClass {
    /// Bounded by the program's own structure, not by any input.
    ///
    /// `isinstance(x, T)` walks a class hierarchy fixed at import time. The
    /// walk is not free, and it is not a function of `n`.
    Constant,
    /// Bounded only when the calls are **summed**, not one at a time.
    ///
    /// `list.append` is the case. A single call may reallocate and copy, so its
    /// worst case is linear and a `Constant` row for it would be a bound the
    /// program exceeds.
    ///
    /// # Why summing makes it sound anyway
    ///
    /// The quantity this toolchain reports is a **sum of source steps along a
    /// path**, never the maximum duration of one statement. The potential
    /// method of amortised analysis says precisely that summing an amortised
    /// per-operation cost over *any* sequence of operations bounds that
    /// sequence's total cost. So an amortised constant is a valid summand in
    /// the quantity actually being computed: a loop appending `n` times
    /// reports `n` steps, and really does cost O(n).
    ///
    /// # What a reader must not take from it
    ///
    /// That a single call is cheap. It is not, and no run of this analysis
    /// claims it is. Because that misreading is available and costs nothing to
    /// make, a bound resting on this class **publishes a premise** naming the
    /// callee - which is the whole reason this is a class of its own rather
    /// than a second spelling of [`Self::Constant`].
    ///
    /// # It does not license forgetting what the call mutates
    ///
    /// Orthogonal, and worth saying because the two were once stacked into one
    /// refusal. `append` changes its receiver, so a length read before it is
    /// not the length a loop after it walks. That is carried by
    /// `mutates_arguments` on the row, which makes the engine forget every
    /// volatile name at the node; this class says nothing about it either way.
    AmortisedConstant,
    /// Linear in the size of an argument.
    LinearInArgument,
    /// `n log n` in the size of an argument.
    Loglinear,
    /// Not bounded by anything this analysis can see: I/O, a lock, a network.
    Unbounded,
}

impl CostClass {
    /// Whether a frontend may resolve a call to a callee in this class.
    ///
    /// The one place the admissibility rule is written. A row that is not
    /// admissible still belongs in the table: see the type's own note.
    #[must_use]
    pub const fn is_resolvable(self) -> bool {
        matches!(self, Self::Constant | Self::AmortisedConstant)
    }

    /// Whether a bound closing over this class must publish a premise saying
    /// so.
    ///
    /// True only for [`Self::AmortisedConstant`]. A `Constant` row claims a
    /// cost that holds call by call, which is what every other number in a
    /// report means, so it adds no premise of its own - a *supplied* row still
    /// publishes one, but for the different reason that somebody outside the
    /// tool wrote it. Amortisation is a claim about sequences, and a reader
    /// cannot infer it from the number.
    #[must_use]
    pub const fn rests_on_amortisation(self) -> bool {
        matches!(self, Self::AmortisedConstant)
    }

    /// How many source steps of the analysed program a call in this class costs
    /// beyond the statement holding it.
    ///
    /// Zero for [`Self::Constant`] and [`Self::AmortisedConstant`], and the
    /// zero is a claim rather than a shrug. The unit this toolchain counts is a *source statement of the
    /// program being analysed*; the work `isinstance` does is C in another
    /// frame and is not statements of that program, exactly as the work a
    /// `property` getter does is not. A bare `isinstance(x, int)` line still
    /// costs the one step every statement costs.
    ///
    /// Meaningless for the classes that are not resolvable, which is why they
    /// answer zero too and are refused before this is asked.
    #[must_use]
    pub const fn steps(self) -> u32 {
        0
    }
}
