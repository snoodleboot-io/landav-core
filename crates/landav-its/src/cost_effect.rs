//! [`CostEffect`] - what holing a refused construct costs a consumer that
//! derives bounds from the structured source rather than from a transition
//! system.

/// How an unanalysable construct affects the cost of the code **around** it.
///
/// # Why this classification lives beside [`crate::Construct`] and not in the
/// consumer
///
/// [`crate::Construct`] is `#[non_exhaustive]`, so a classifier written in
/// another crate needs a wildcard arm - and a wildcard arm is precisely how a
/// construct added tomorrow inherits a default nobody chose. Adding a variant
/// here is a compile error at the one exhaustive `match` that decides this,
/// which is the review the decision deserves.
///
/// # The distinction
///
/// A consumer that meets a construct it cannot analyse may stand a variable in
/// for its cost - a *hole* - and carry on deriving everything around it. That
/// move is sound only when the region's effect is confined to its own cost.
/// [`Self::ControlFlow`] is the case where it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CostEffect {
    /// The construct costs an unknown amount **at that point**, and nothing
    /// else about the surrounding control flow changes.
    ///
    /// A call is the archetype: whatever it does, execution resumes at the next
    /// statement, so the enclosing loop still runs the number of times its range
    /// says and everything outside the region keeps whatever was derived for it.
    Region,
    /// The construct is an **edge out of** the region being counted.
    ///
    /// `break`, `continue`, `raise`: a loop containing one may stop before its
    /// counter is exhausted, so the trip count derived from the range is an
    /// over-estimate rather than an equality. A consumer holing one of these
    /// must weaken every enclosing count it claimed exactly, or it keeps
    /// asserting an equality the construct can falsify.
    ControlFlow,
}
