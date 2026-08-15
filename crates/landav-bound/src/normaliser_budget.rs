//! [`NormaliserBudget`] - the frozen determinism contract for LAN-58.
//!
//! LAN-58 implements normalisation; this module freezes the configuration it
//! must use. It is here, in the frozen API, because getting any of it wrong
//! produces a bound that differs between two runs of the same binary on the
//! same input - and that is invisible on a single developer machine.
//!
//! # The e-graph configuration, in full
//!
//! 1. **No wall clock.** `egg`'s `Runner` defaults to a five-second
//!    `time_limit`, and its limit check returns the time-limit stop *before*
//!    the node and iteration checks. On an idle laptop the runner saturates
//!    and extraction returns the fully-normalised term; on a loaded CI runner,
//!    under coverage instrumentation, or under a parallel mutation run, it
//!    stops earlier and extraction returns a less-normalised term. Same input,
//!    same binary, different normal form, different golden output, different
//!    cache key. The runner **must** be built with
//!    `.with_time_limit(Duration::MAX)` and with the deterministic budgets
//!    below.
//! 2. **Only deterministic stop reasons.** The run must stop on
//!    `Saturated`, `IterationLimit` or `NodeLimit`. Anything else - in
//!    particular `TimeLimit` - is
//!    [`crate::BoundError::NonDeterministicNormalisation`], a hard error,
//!    never a silently less-normalised bound.
//! 3. **No `egg::Symbol`.** It is `symbol_table::GlobalSymbol`, whose `Ord`
//!    and `Hash` are an index into a **process-global** interner. `rebuild`
//!    sorts each e-class's nodes by the language's `Ord`, and extraction picks
//!    with `min_by`, which returns the *first* minimum - so every cost tie is
//!    broken by interner index, which is a function of the order files were
//!    walked. The mirror language must carry [`crate::VarId`] (content-`Ord`)
//!    instead.
//! 4. **`features = ["deterministic"]`.** `egg`'s internal maps are
//!    `hashbrown` with a fixed-seed hasher, so iteration is stable within one
//!    binary but is a function of hashbrown's layout and the hasher's
//!    constants, both of which move within semver. The `deterministic`
//!    feature swaps them for `IndexMap`/`IndexSet`. `egg` must additionally
//!    appear in a member crate's dependency list so that it and its whole
//!    transitive tree are captured in a committed `Cargo.lock`, and
//!    `cargo update` touching `hashbrown`, `rustc-hash` or `indexmap` must
//!    require golden re-verification.
//! 5. **The cost function must guarantee four things.**
//!    * `Cost: Ord`, integer valued, **no floats**. `egg` compares costs with
//!      `partial_cmp(..).unwrap()`, so an `f64` `NaN` panics inside a
//!      dependency where the workspace lints cannot see it.
//!    * A **total** tie-break to a unique winner. A residual tie falls through
//!      to the language's `Ord`. The workable shape is a lexicographic
//!      `(size, canonical_key)` where the key is built bottom-up from the
//!      children's already-chosen costs.
//!    * **No saturation-induced ties.** `egg`'s own `AstSize` accumulates with
//!      `saturating_add` into a `usize`; two enormous distinct terms both
//!      reach `usize::MAX` and tie. Widen the accumulator, or let the
//!      tie-break carry the distinction.
//!    * **Monotone in the sound direction.** If the rewrite set is not
//!      equality preserving, extraction is choosing between semantically
//!      *different* terms and the cost function becomes a soundness surface.
//!      Note that `log_2(x) -> log_4(x)` is unsound while
//!      `log_4(x) -> log_2(x)` is sound-but-loosening.
//! 6. **Version everything together.** Any change to the canonical order, the
//!    rewrite set or the cost function bumps [`crate::NORMAL_FORM_VERSION`].
//! 7. **CI must assert it.** A gate that normalises a fixed corpus twice in
//!    one process *and* in two separate processes, and byte-diffs both the
//!    rendered bound and [`crate::Bound::canonical_bytes`]. Without it this
//!    whole class of defect is invisible to the existing gates.

/// The frozen, deterministic stopping budget for e-graph normalisation.
///
/// Both limits are counts, not durations. That is the entire point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormaliserBudget {
    iter_limit: usize,
    node_limit: usize,
}

impl NormaliserBudget {
    /// The frozen budget. Changing either number changes the normal form and
    /// requires bumping [`crate::NORMAL_FORM_VERSION`].
    pub const FROZEN: Self = Self {
        iter_limit: 60,
        node_limit: 100_000,
    };

    /// The maximum number of equality-saturation iterations.
    #[must_use]
    pub const fn iter_limit(self) -> usize {
        self.iter_limit
    }

    /// The maximum number of e-nodes.
    #[must_use]
    pub const fn node_limit(self) -> usize {
        self.node_limit
    }
}
