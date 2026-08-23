//! [`Extent`] - how much of a source statement an [`crate::SourceStmt::Unsupported`]
//! node stands for.

/// Whether an unsupported statement node **is** a source statement or was
/// lifted out of the one beside it.
///
/// # Why this distinction has to be in the node
///
/// The fragment has no expression slot on `Return`, and none at all on a
/// refused assignment, so a frontend that finds something unanalysable inside
/// one of those has nowhere to put it but a statement of its own. Two quite
/// different source facts therefore arrive as the same node kind:
///
/// * `f(n)` on a line by itself - the node **is** the statement; nothing else
///   in the program accounts for the step that executing it costs;
/// * the `f(n)` in `return f(n)` - the node is a *fragment* of a statement that
///   is also in the arena and pays its own step.
///
/// A consumer that charges one step per statement cannot tell those apart from
/// the arena, and no single charge is right for both: charging a step for the
/// fragment double-counts the `return`, and charging none for the statement
/// loses a step per region - which is the direction a cost may not move, and
/// which breaks composition, since filling the hole with what the call costs
/// then yields a total one step short of what the engine would have derived had
/// it known the callee all along.
///
/// # The default is the safe one
///
/// [`Self::Statement`] is what [`crate::SourceProgramBuilder::unsupported_stmt`]
/// builds. A frontend that never thinks about this over-charges by one step per
/// hoisted fragment; one that marks a genuine statement as a fragment
/// *under*-charges. Only the second is a soundness failure, and it takes a
/// deliberate call to reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Extent {
    /// The node stands for a whole source statement.
    ///
    /// Executing it costs one step, as every statement does, **plus** whatever
    /// the unanalysable part costs.
    #[default]
    Statement,
    /// The node was lifted out of a neighbouring statement.
    ///
    /// That statement is in the arena too and pays the step; this node is
    /// charged for the region alone.
    Fragment,
}
