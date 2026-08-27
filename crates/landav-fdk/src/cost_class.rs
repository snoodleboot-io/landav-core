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
/// # Only one class is admissible today, and the others are here to be refused
///
/// [`Self::Constant`] is the only class a frontend may resolve. The rest exist
/// so that a row for `join` or `sorted` can be *written down and rejected* -
/// carrying its `why` and its measurement - rather than being absent and
/// looking like an oversight. A pack is a place to record a decision, and half
/// the decisions are refusals.
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
        matches!(self, Self::Constant)
    }

    /// How many source steps of the analysed program a call in this class costs
    /// beyond the statement holding it.
    ///
    /// Zero for [`Self::Constant`], and the zero is a claim rather than a
    /// shrug. The unit this toolchain counts is a *source statement of the
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
