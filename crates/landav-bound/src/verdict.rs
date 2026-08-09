//! [`Verdict`] - the only sanctioned way to publish a derivation result.

use crate::{
    blames::Blames, bound::Bound, bound_error::BoundError, exit_code::ExitCode,
    finite_bound::FiniteBound, lifted::Lifted, origin::Origin, partial_bound::PartialBound,
};

/// The result of a derivation. None of the outcomes is "unknown".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// A finite bound, with nothing unaccounted for.
    Proved(FiniteBound),
    /// A sound over-approximation, with at least one named unaccounted term.
    Partial(PartialBound),
    /// No execution path reaches the analysed region.
    ///
    /// A third outcome exists because the carrier has a bottom element, and
    /// `Bottom` is *not* a cost of zero. Without this arm, an engine that
    /// hands back `Bottom` - which it does for every not-yet-visited fixpoint
    /// node - would publish "peak memory 0, proved, no blame".
    Unreachable(Origin),
}

impl Verdict {
    /// **Total in intent, and the only sanctioned way to publish a result.**
    ///
    /// The blame ledger is a *value*, computed by the caller and passed in
    /// eagerly. It is not a closure: a lazily-invoked closure that is never
    /// invoked drops the blame *silently*, so no logging or assertion inside
    /// it can fire.
    ///
    /// Classification, exhaustively:
    ///
    /// | `cost` | `ledger` | result |
    /// |---|---|---|
    /// | `Bottom` | empty | [`Verdict::Unreachable`] |
    /// | `Bottom` | non-empty | [`Verdict::Partial`] over `omega` - reachability was not established *and* something was unaccounted for |
    /// | `Elem(b)` | non-empty | [`Verdict::Partial`] |
    /// | `Elem(b)`, `b` finite | empty | [`Verdict::Proved`] |
    /// | `Elem(b)`, `b` mentions `omega` | empty | `Err(`[`BoundError::UnblamedOmega`]`)` |
    ///
    /// The last row is the load-bearing one. `omega`-freeness is **not** a
    /// sound proxy for "nothing was unaccounted for" - blame provenance flows
    /// through the ledger, not by scanning the final term - so an unbounded
    /// result with no blame is refused rather than published.
    ///
    /// # Errors
    ///
    /// [`BoundError::UnblamedOmega`], as above.
    pub fn classify(
        cost: Lifted<Bound>,
        at: Origin,
        ledger: Option<Blames>,
    ) -> Result<Self, BoundError> {
        todo!()
    }

    /// The reported bound, sound in every case it exists.
    /// `None` for [`Verdict::Unreachable`], which has no cost to report.
    #[must_use]
    pub fn bound(&self) -> Option<&Bound> {
        todo!()
    }

    /// The blame ledger, if the result is partial.
    #[must_use]
    pub fn blames(&self) -> Option<&Blames> {
        todo!()
    }

    /// The process exit code.
    ///
    /// `fail_on_partial` corresponds to the `--fail-on-partial` flag.
    /// **Without it, a file where every function came back `Partial` with
    /// blame reports clean**, because "we could not look" has no code of its
    /// own - so the flag must exist and the default must be documented rather
    /// than assumed.
    #[must_use]
    pub fn exit_code(&self, fail_on_partial: bool) -> ExitCode {
        todo!()
    }
}
