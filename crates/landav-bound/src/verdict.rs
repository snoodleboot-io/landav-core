//! [`Verdict`] - the only sanctioned way to publish a derivation result.
//!
//! # There is deliberately no `Verdict::exit_code`
//!
//! A [`Verdict`] states a **fact** about a derivation: a bound was proved, a
//! bound was reached with something unaccounted for, or no execution gets
//! there. A process exit code puts a **price** on that fact, and pricing is
//! policy. This crate declares the code space ([`crate::exit_code::ExitCode`])
//! so the CLI cannot invent one, and stops there - the same line it draws at
//! [`Bound`] implementing no [`Ord`].
//!
//! The mapping is therefore decided in exactly one place,
//! `landav_cli::outcome::Outcome::exit_code`. A `Verdict::exit_code` alongside
//! it (LAN-61) priced [`Verdict::Partial`] as `Clean`, or as `ToolError` under
//! a `--fail-on-partial` flag, while the CLI priced the same state - analysed,
//! no conclusion reached - as `Findings`. One semantic state, three codes,
//! and no caller: nothing outside its own tests ever invoked it.
//!
//! Its `fail_on_partial` parameter was the tell. `landav-cli`'s configuration
//! loader *refuses* `[tool.landav] fail-on-partial`, on the stated grounds
//! that "a setting that is accepted and ignored is worse than one that is
//! refused". A function whose result turns on a flag the product refuses is
//! that same setting, one layer down. The question it answered belongs to
//! whoever ships the flag; until then, [`Verdict::blames`] is the fact a
//! caller needs, and the price is the CLI's to set.

use crate::{
    blames::Blames, bound::Bound, bound_error::BoundError, finite_bound::FiniteBound,
    lifted::Lifted, origin::Origin, partial_bound::PartialBound,
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
        match (cost, ledger) {
            // No execution reaches here, and nothing was left unaccounted for.
            (Lifted::Bottom, None) => Ok(Self::Unreachable(at)),
            // Reachability was not established *and* something was
            // unaccounted for: a partial over `omega`, never `Unreachable`.
            (Lifted::Bottom, Some(blames)) => {
                Ok(Self::Partial(PartialBound::new(Bound::omega(), blames)))
            }
            (Lifted::Elem(bound), Some(blames)) => {
                Ok(Self::Partial(PartialBound::new(bound, blames)))
            }
            // `omega`-freeness is necessary but not sufficient: the empty
            // ledger is what makes this publishable at all.
            (Lifted::Elem(bound), None) => match FiniteBound::try_new(bound) {
                Ok(finite) => Ok(Self::Proved(finite)),
                Err(_) => Err(BoundError::UnblamedOmega),
            },
        }
    }

    /// The reported bound, sound in every case it exists.
    /// `None` for [`Verdict::Unreachable`], which has no cost to report.
    #[must_use]
    pub fn bound(&self) -> Option<&Bound> {
        match self {
            Self::Proved(finite) => Some(finite.get()),
            Self::Partial(partial) => Some(partial.bound()),
            Self::Unreachable(_) => None,
        }
    }

    /// The blame ledger, if the result is partial.
    #[must_use]
    pub fn blames(&self) -> Option<&Blames> {
        match self {
            Self::Partial(partial) => Some(partial.blames()),
            Self::Proved(_) | Self::Unreachable(_) => None,
        }
    }

    /// Whether the derivation reached a conclusion.
    ///
    /// True for [`Verdict::Proved`] and [`Verdict::Unreachable`], false for
    /// [`Verdict::Partial`]. This is the **fact** a caller deciding an exit
    /// code needs; see the module documentation for why the decision itself is
    /// not made here.
    #[must_use]
    pub const fn is_conclusive(&self) -> bool {
        match self {
            Self::Proved(_) | Self::Unreachable(_) => true,
            Self::Partial(_) => false,
        }
    }
}
