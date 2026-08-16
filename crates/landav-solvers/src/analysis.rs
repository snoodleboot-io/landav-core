//! [`Analysis`] - an upper answer beside a lower one, and what to do when
//! they disagree.

use landav_bound::Verdict;

use crate::{direction::Direction, growth::Growth, report::Report, solver_error::SolverError};

/// What an upper and a lower answer say when put together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agreement {
    /// Only one direction answered, or the other one found nothing. No claim
    /// about tightness is available.
    Unpaired,
    /// Both answered in the same class: the upper bound is **tight**. The only
    /// honest way to make that claim.
    Tight(Growth),
    /// Both answered and the lower is strictly below the upper. A real gap,
    /// reported as the two classes rather than smoothed into one.
    Gap {
        /// What the lower-bound solver proved.
        lower: Growth,
        /// What the upper-bound solver proved.
        upper: Growth,
    },
    /// Both answered and the lower is strictly **above** the upper.
    /// Impossible; one of the two solvers is wrong.
    Contradiction {
        /// What the lower-bound solver proved.
        lower: Growth,
        /// What the upper-bound solver proved.
        upper: Growth,
    },
}

/// One function's answers from up to two solvers.
///
/// # A contradiction is reported, never reconciled
///
/// There is an obvious reconciliation available, and it is exactly wrong: keep
/// the upper bound - it is what gets published anyway - and discard the lower
/// one as unreliable. A lower bound above an upper bound is **positive
/// evidence that the upper bound is too small**, and a reported bound the
/// program can exceed is the one failure class with a zero target. Discarding
/// the evidence converts a loud disagreement into a quiet unsound number.
///
/// Nothing in either solver's output says which of them is wrong, and this
/// crate has no way to find out. So [`Analysis::verdict`] publishes **nothing
/// at all** and names both classes, which is a question a human can answer and
/// a bound is not.
///
/// # `omega` is never contradicted
///
/// An upper answer of "no bound found" is `omega`, which nothing exceeds, so
/// no lower bound can disagree with it - including a proved-infinite one,
/// which agrees with it exactly. That case is [`Agreement::Unpaired`] rather
/// than a contradiction, because an upper answer that found nothing is not a
/// claim to be contradicted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    upper: Option<Report>,
    lower: Option<Report>,
}

impl Analysis {
    /// Pair an upper report with a lower one, either of which may be absent.
    ///
    /// # Errors
    ///
    /// [`SolverError::DirectionMismatch`] if a report is filed under a
    /// direction its solver does not bound in. Refused at the constructor
    /// rather than trusted to call sites: a lower bound reported as an upper
    /// bound is, by construction, a bound the program exceeds.
    pub fn new(upper: Option<Report>, lower: Option<Report>) -> Result<Self, SolverError> {
        check(upper.as_ref(), Direction::Upper)?;
        check(lower.as_ref(), Direction::Lower)?;
        Ok(Self { upper, lower })
    }

    /// The upper-bound report, if one was obtained.
    #[must_use]
    pub const fn upper(&self) -> Option<&Report> {
        self.upper.as_ref()
    }

    /// The lower-bound report, if one was obtained.
    #[must_use]
    pub const fn lower(&self) -> Option<&Report> {
        self.lower.as_ref()
    }

    /// What the two answers say together.
    #[must_use]
    pub fn agreement(&self) -> Agreement {
        let paired = self
            .upper
            .as_ref()
            .and_then(|report| report.answer().growth())
            .zip(
                self.lower
                    .as_ref()
                    .and_then(|report| report.answer().growth()),
            );
        let Some((upper, lower)) = paired else {
            return Agreement::Unpaired;
        };
        // An upper answer that found nothing is `omega`. Nothing exceeds it,
        // so nothing contradicts it, and it is not a claim to be tight about
        // either.
        if upper == Growth::Unbounded {
            return Agreement::Unpaired;
        }
        match lower.cmp(&upper) {
            core::cmp::Ordering::Equal => Agreement::Tight(upper),
            core::cmp::Ordering::Less => Agreement::Gap { lower, upper },
            core::cmp::Ordering::Greater => Agreement::Contradiction { lower, upper },
        }
    }

    /// The publishable verdict, which is always the upper bound's.
    ///
    /// # Errors
    ///
    /// [`SolverError::Contradiction`] when the two answers cannot both be
    /// true, in which case nothing is published at all;
    /// [`SolverError::Unpublishable`] when no upper-bound solver answered, so
    /// there is no bound to publish.
    pub fn verdict(&self) -> Result<Verdict, SolverError> {
        if let Agreement::Contradiction { lower, upper } = self.agreement() {
            return Err(SolverError::Contradiction { lower, upper });
        }
        let Some(upper) = self.upper.as_ref() else {
            return Err(SolverError::Unpublishable {
                detail: "no upper-bound solver answered, so there is no bound to publish"
                    .to_owned(),
            });
        };
        upper.verdict()
    }
}

/// Refuse a report filed under a direction its solver does not bound in.
fn check(report: Option<&Report>, expected: Direction) -> Result<(), SolverError> {
    match report {
        Some(report) if report.direction() != expected => Err(SolverError::DirectionMismatch {
            solver: report.solver(),
            expected,
            actual: report.direction(),
        }),
        _ => Ok(()),
    }
}
