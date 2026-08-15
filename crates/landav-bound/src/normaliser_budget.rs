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
    ///
    /// # Why the node limit is 10 000 and not larger
    ///
    /// The limits bound *e-nodes*, not bytes or seconds, and the node limit is
    /// checked *between* iterations — so one iteration overshoots it, and the
    /// memory high-water mark is set by the overshoot rather than by the limit.
    ///
    /// At a node limit of 100 000 that overshoot is not affordable. The Gate 2
    /// algebra adversary found a **six-node** term that reaches the ceiling:
    ///
    /// ```text
    /// 0 * (x + z) * (z + z)
    /// ```
    ///
    /// Three factors and two sums are enough, because distribution runs in both
    /// directions and each pass multiplies the ways the same value can be
    /// spelled. Measured at 100 000: 9 iterations, 101 724 e-nodes, and the run
    /// did not finish inside three minutes on a loaded machine — the adversary
    /// recorded 22 s and roughly 5 GB resident on an idle one. Either way it is
    /// an out-of-memory surface reachable from ordinary input, which is the same
    /// class of defect as the hangs `frozen_invariants.rs` exists to prevent.
    ///
    /// At 10 000 the same term stops at 38 525 e-nodes in 376 ms. Terms that
    /// would have saturated between the two limits now stop earlier and reach a
    /// **less normalised** form — that is a tightness loss, not a soundness one,
    /// because every rewrite preserves the ideal value in both directions.
    ///
    /// This is why the normal form is defined by the rule set *and* the budget
    /// together, and why lowering this number bumped
    /// [`crate::NORMAL_FORM_VERSION`].
    pub const FROZEN: Self = Self {
        iter_limit: 60,
        node_limit: 10_000,
    };

    /// An explicit budget, for tests and for tuning.
    ///
    /// **Not for anything persisted.** [`Self::FROZEN`] is the budget the
    /// normal form is defined at; a bound normalised at any other budget may
    /// be less normalised, so it must never reach a golden, a report or an
    /// F-008 cache entry. It exists because a budget type with exactly one
    /// inhabitant cannot be tested: the iteration-limit and node-limit stop
    /// paths are unreachable at [`Self::FROZEN`] for any term small enough to
    /// put in a test, and an untestable stop path is where a silently
    /// non-deterministic bound would hide.
    #[must_use]
    pub const fn new(iter_limit: usize, node_limit: usize) -> Self {
        Self {
            iter_limit,
            node_limit,
        }
    }

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

#[cfg(test)]
mod frozen_budget {
    use super::NormaliserBudget;

    /// Both numbers are part of the normal form: changing either changes the
    /// term LAN-58 extracts and therefore every persisted cache entry. They
    /// must fail here before they reach a golden file.
    #[test]
    fn the_frozen_budget_is_pinned() {
        assert_eq!(NormaliserBudget::FROZEN.iter_limit(), 60);
        assert_eq!(NormaliserBudget::FROZEN.node_limit(), 10_000);
    }

    /// Two `usize` accessors on a two-`usize` struct: swapping them compiles,
    /// type checks, and silently swaps a 60-iteration budget for a
    /// 100000-iteration one. Pinned on a budget whose fields cannot be
    /// confused for each other.
    #[test]
    fn the_accessors_do_not_alias() {
        let budget = NormaliserBudget {
            iter_limit: 7,
            node_limit: 11,
        };
        assert_eq!(budget.iter_limit(), 7);
        assert_eq!(budget.node_limit(), 11);
        assert_ne!(
            NormaliserBudget::FROZEN.iter_limit(),
            NormaliserBudget::FROZEN.node_limit()
        );
    }

    /// Both limits are **counts**, never durations, and both are non-zero: a
    /// zero iteration budget saturates nothing and returns the input term,
    /// which is a silently less-normalised bound rather than an error.
    #[test]
    fn both_limits_are_positive_counts() {
        assert!(NormaliserBudget::FROZEN.iter_limit() > 0);
        assert!(NormaliserBudget::FROZEN.node_limit() > 0);
        assert!(NormaliserBudget::FROZEN.node_limit() > NormaliserBudget::FROZEN.iter_limit());
    }

    /// `FROZEN` is a `const`, and the accessors are `const fn`, so the budget
    /// can be compared at compile time.
    #[test]
    fn the_accessors_are_usable_in_a_const_context() {
        const ITERS: usize = NormaliserBudget::FROZEN.iter_limit();
        const NODES: usize = NormaliserBudget::FROZEN.node_limit();
        assert_eq!(ITERS, 60);
        assert_eq!(NODES, 10_000);
    }
}
