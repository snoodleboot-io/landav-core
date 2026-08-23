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
//! honest arithmetic shortcut. No call, and nothing else outside the numeric
//! fragment: the cost of a region this crate cannot read is genuinely unknown.
//!
//! It also keeps **no assignment environment**. A variable read is turned into a
//! bound only while the name still holds the value the caller supplied, and an
//! unanalysable region forgets every such name - it may assign to anything. That
//! absence is what makes holing a call sound, so it is enforced at the single
//! point a region is charged rather than restated at each caller; see
//! `analyse::Walk::region`.
//!
//! None of those is a refusal. Each becomes a [`Hole`] - a variable standing for
//! that region's cost, carrying the construct that caused it and where it is -
//! and everything around it is derived as usual. A function is `Partial` rather
//! than `Unknown`, and the report names what it could not derive instead of
//! saying nothing.
//!
//! There is deliberately **no** fallback to a solver behind any of this. A
//! program containing a call does not lower, so there is no transition system
//! for a solver to be given; that was the whole reason those functions produced
//! nothing before. [`landav_bound::Bound::subst`] is the seam instead: a hole filled later,
//! from a solver or from a ranking-function engine or from a caller's own
//! knowledge of the callee, yields the bound this crate would have produced had
//! it known the region all along. An **unfilled** hole denotes `omega`, which is
//! why a bound carrying one is never reported as a complete claim.
//!
//! Two things make that seam hold, and both have been lost once:
//!
//! * **a complete result's bound mentions no hole variable.** Every transform of
//!   a bound goes through [`TripCount::substituting`] rather than rebuilding a
//!   variant from a bare [`landav_bound::Bound`], because rebuilding one is how
//!   a nested loop beside a `while` came to be published as
//!   `O(2 + n * (1 + #hole0 + 2n))` with an empty hole ledger - a finite-looking
//!   upper bound, offered for comparison against a budget, with an omega-valued
//!   variable in it that a caller supplying only the parameters reads as zero;
//! * **a hole is charged what its region costs, and not what the statement
//!   around it costs.** A region that *is* a statement is charged the
//!   statement's own step beside its hole; one lifted out of a neighbouring
//!   statement is not, because that statement is charging it. See
//!   [`landav_its::Extent`]. Getting it backwards does not make a bound wrong
//!   today - it makes every *filled* bound one step per region away from the one
//!   this crate would have derived, which is the guarantee above.
//!
//! # Total over [`landav_its::SourceProgram`]
//!
//! Every program the frontend can build has an answer here, and every
//! `Unsupported` node in it is charged at its position in the control
//! structure, so a region in a loop body costs once per iteration rather than
//! once. The walk is reconciled afterwards against
//! [`landav_its::SourceProgram::unsupported_nodes`]. A node the walk could not
//! reach has no position, therefore no sound charge, and fails the whole result
//! closed to [`TripCount::Unknown`] rather than being swept up at the top
//! level: charged flat, a region that belonged inside a loop is counted once
//! instead of once per iteration, and the bound **understates**.
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
