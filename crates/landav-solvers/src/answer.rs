//! [`Answer`] - what a solver said, once it has been read.

use landav_bound::Bound;

use crate::growth::Growth;

/// A solver's answer about one integer transition system.
///
/// # Three outcomes, and none of them is a silent absence
///
/// The two solvers answer in different currencies and the type keeps them
/// apart rather than flattening one into the other:
///
/// * KoAT prints an expression **and** a class, so it produces
///   [`Answer::Symbolic`].
/// * LoAT prints a class alone, so it produces [`Answer::Class`]. Inventing a
///   symbolic bound from a class would put a term in front of a user that no
///   solver ever proved.
/// * Either can decline, which is [`Answer::Unknown`].
///
/// # `Unknown` is an answer, not a missing one
///
/// KoAT prints `inf {Infinity}` when its search finds nothing, and it does so
/// often - the published figure is 548 of 838 curated integer-only benchmarks.
/// That is not an error to swallow and it is not an empty result: it is
/// `omega`, and [`crate::Report::verdict`] publishes it as
/// [`landav_bound::Verdict::Partial`] with blame naming the function. A bare
/// "unknown" has no representation here.
///
/// # `Unknown` and a proved infinity are different findings
///
/// LoAT's `WORST_CASE(INF,?)` means it **proved** the runtime unbounded, which
/// is a positive statement about the program, and it becomes
/// `Answer::Class(Growth::Unbounded)`. KoAT's `inf` means it found no bound,
/// which is a statement about the search. Collapsing the two would report "we
/// learned nothing" as "this program does not terminate".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// A symbolic bound, together with the growth class the solver announced
    /// beside it. The two have been checked against each other.
    Symbolic {
        /// The bound, over the analysed function's own variable names.
        bound: Bound,
        /// The class the solver stated.
        growth: Growth,
    },
    /// A growth class with no symbolic bound.
    Class(Growth),
    /// The solver ran and found nothing in its direction.
    Unknown,
}

impl Answer {
    /// The growth class, for the solvers that state one.
    #[must_use]
    pub const fn growth(&self) -> Option<Growth> {
        match self {
            Self::Symbolic { growth, .. } => Some(*growth),
            Self::Class(growth) => Some(*growth),
            Self::Unknown => None,
        }
    }

    /// The symbolic bound, for the answers that carry one.
    #[must_use]
    pub const fn bound(&self) -> Option<&Bound> {
        match self {
            Self::Symbolic { bound, .. } => Some(bound),
            Self::Class(_) | Self::Unknown => None,
        }
    }
}
