//! [`DeclaredEffect`] - what a frontend can say about a node it did not
//! translate but can nevertheless account for.

/// A refused node's declared cost and effect on the frame it stands in.
///
/// # Why a node says this and not its [`crate::Construct`]
///
/// [`crate::Construct::may_rebind_locals`] answers the same question one level
/// too coarsely. It is a property of the *kind* of thing - an attribute access,
/// a subscript - and `Construct::Call` has to answer for every callee at once,
/// so it answers `true`: `foo(x)` may be a closure over this frame's namespace
/// and nothing here knows otherwise.
///
/// `isinstance(x, int)` is not that call. Its cost is a walk of a class
/// hierarchy the program fixed at import time, and it runs in its own frame, so
/// neither the cost nor the rebinding is unknown - they are known and *small*.
/// The difference is per-callee and the construct is per-kind, so the answer has
/// to hang on the node.
///
/// # This crate does not know what a signature pack is, and must not
///
/// Nothing here mentions Python, a callee name, a pack file or a table. A
/// frontend that has decided - by whatever means, and the means are its own
/// business - that a particular node costs a bounded amount and cannot rebind a
/// local says exactly that, in these three fields, and this crate and
/// `landav-engine` read them. Teaching either crate to look a callee up in a
/// table would put a Python fact in a language-agnostic layer and make every
/// future frontend inherit it.
///
/// # A declaration is a claim, and the claims are checked by whoever makes them
///
/// A node carrying one of these is **not** a refusal: [`crate::lower`] emits a
/// transition for it rather than refusing the program, and `landav-engine`
/// charges its declared cost rather than a hole. That is exactly as sound as the
/// declaration, which is why the frontend that writes one is expected to be able
/// to point at the row it came from and the sentence explaining it.
///
/// # What each field licenses
///
/// | field | `false` / `0` means | `true` / `n` means |
/// |---|---|---|
/// | [`Self::steps`] | the node costs nothing beyond the statement holding it | it costs `n` source steps of its own |
/// | [`Self::rebinds_locals`] | no local of this frame changes across it | every value the analysis knew is forgotten |
/// | [`Self::mutates_arguments`] | no object reachable from here changes either | anything standing for an object's state is forgotten |
///
/// The second column of the last two rows is the *conservative* answer, and it
/// is what [`crate::Construct::may_rebind_locals`] already says for a call. A
/// declaration is only ever worth making because it says something narrower.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeclaredEffect {
    cost: DeclaredCost,
    rebinds_locals: bool,
    mutates_arguments: bool,
}

/// What a declared node costs. `LAN-116`.
///
/// # Why an index and not a bound
///
/// A constant is the common case and is `Copy`, which this type and every
/// node carrying it rely on - thirteen sites read a declaration by value. A
/// *composed* cost is a whole polynomial in the caller's variables, which is
/// neither `Copy` nor small, so it lives in a side table on the
/// [`crate::SourceProgram`] and the node carries its index. The engine looks
/// it up at the one place it charges a declaration; nothing else needs to.
///
/// # What `Composed` means to the two consumers
///
/// `landav-engine` charges the bound exactly as it charges a constant. The
/// integer-transition-system lowering in this crate does **not**: a bound may
/// hold `max`, `log` or `omega`, none of which an ITS transition cost can
/// express, so a composed node is a *refusal* there - the native engine
/// composes, the ITS reach is unchanged, and a function that composed is
/// reported through `derived::cost_of`'s "refused but every refusal was in
/// front of the engine" rule. Expressing the polynomial subset in the ITS is
/// a separate change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeclaredCost {
    /// This many source steps, beyond the statement holding the node.
    Steps(u32),
    /// The bound at this index of [`crate::SourceProgram::composed`] - a
    /// sibling method's own derived bound, substituted into the caller's
    /// variables.
    Composed(u32),
}

impl DeclaredEffect {
    /// A node costing `steps` source steps of its own.
    #[must_use]
    pub const fn new(steps: u32, rebinds_locals: bool, mutates_arguments: bool) -> Self {
        Self {
            cost: DeclaredCost::Steps(steps),
            rebinds_locals,
            mutates_arguments,
        }
    }

    /// A node whose cost is the composed bound at `index` of the program's
    /// side table. `LAN-116`; see [`DeclaredCost::Composed`].
    #[must_use]
    pub const fn composed(index: u32, rebinds_locals: bool, mutates_arguments: bool) -> Self {
        Self {
            cost: DeclaredCost::Composed(index),
            rebinds_locals,
            mutates_arguments,
        }
    }

    /// What this node costs.
    #[must_use]
    pub const fn cost(self) -> DeclaredCost {
        self.cost
    }

    /// Whether the cost is a composed bound rather than a constant.
    #[must_use]
    pub const fn is_composed(self) -> bool {
        matches!(self.cost, DeclaredCost::Composed(_))
    }

    /// How many source steps this node costs **beyond** the statement it stands
    /// in.
    ///
    /// Zero for every callee admitted so far, and that is not a shrug. The unit
    /// this toolchain counts is a *source statement of the program being
    /// analysed* - see [`crate::lower`]'s cost table - and the work
    /// `isinstance` does is not statements of that program. A bare
    /// `isinstance(x, int)` line still costs one step, because it is one
    /// statement; what is declared here is that the callee adds nothing to it
    /// that grows with anything.
    ///
    /// `None` for a composed cost, which is not a number of steps at all; see
    /// [`DeclaredCost`].
    #[must_use]
    pub const fn steps(self) -> Option<u32> {
        match self.cost {
            DeclaredCost::Steps(steps) => Some(steps),
            DeclaredCost::Composed(_) => None,
        }
    }

    /// Whether evaluating this node can change what a **local name** of the
    /// frame it stands in denotes.
    ///
    /// See [`crate::Construct::may_rebind_locals`], where the argument lives.
    #[must_use]
    pub const fn rebinds_locals(self) -> bool {
        self.rebinds_locals
    }

    /// Whether evaluating this node can mutate an object reachable from it.
    ///
    /// `true` for every callee admitted so far, and deliberately so: an
    /// `__instancecheck__` is user code and may call `items.append(...)`, so a
    /// length read on entry is not the length a loop below walks. This inherits
    /// [`crate::SourceProgram::is_volatile`] rather than reopening it.
    #[must_use]
    pub const fn mutates_arguments(self) -> bool {
        self.mutates_arguments
    }
}
