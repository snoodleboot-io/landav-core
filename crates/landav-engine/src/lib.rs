//! Native worst-case bound analysis over landav's structured source fragment.
//!
//! # Why this exists beside the solver bridge
//!
//! The external path lowers a program to an integer transition system and asks
//! a solver for a bound. That lowering is faithful, but it is also lossy in a
//! way that matters: a flat transition graph does not record which loop was a
//! counted `for` over a range, how loops were nested syntactically, or that the
//! fragment forbids leaving a loop early. The solver then spends its effort
//! *rediscovering* that structure through ranking functions and control-flow
//! refinement.
//!
//! For the fragment landav accepts, the structure **is** the answer. A counted
//! loop's iteration space is fixed before it starts, and nothing can exit
//! early, so the trip count is arithmetic on values the program already
//! computed rather than something to be inferred. This crate reads the
//! structured program directly and says so.
//!
//! The difference is measurable. Asked for the cost of a loop running `n` times
//! at `n` units each - exactly `n^2` - the external solver returns
//! `2*Arg_0^2+1`: right shape, twice as large, plus a constant. This crate
//! returns `n^2`.
//!
//! # What it does not do
//!
//! No `while` loops. Bounding one needs a ranking argument, and there is no
//! honest arithmetic shortcut. Those return [`TripCount::Unknown`] and the
//! caller falls back to the solver, which is good at exactly that case.
//!
//! # Exactness is a claim, and it is tracked
//!
//! Every result carries whether it is exact or merely an upper bound. The two
//! support different claims - `Theta` against `O` - and a caller that cannot
//! tell them apart would report tightness it has not established. See
//! [`TripCount`].

pub mod analyse;
pub mod expr_bound;
pub mod hole;
pub mod rational;
pub mod summation;
pub mod trip_count;

pub use crate::{analyse::cost, hole::Hole, trip_count::TripCount};
