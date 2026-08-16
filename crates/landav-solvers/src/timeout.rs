//! [`Timeout`] - the wall clock a solver run is held to, and the poll budget
//! derived from it.

use std::time::Duration;

use crate::{MAX_TIMEOUT_SECS, MIN_TIMEOUT_SECS, POLL_INTERVAL_MILLIS, solver_error::SolverError};

/// How long a single solver invocation may take.
///
/// # Why there is a timeout at all
///
/// KoAT's search has no bound of its own and LoAT has no timeout option
/// whatsoever, so "the solver returns" is not a fact either binary supplies.
/// An analyser that hangs on a user's CI is worse than one that declines: a
/// timed-out job carries no exit code at all, so it cannot even be read as a
/// finding. Declining is a result; hanging is not.
///
/// # Why thirty seconds
///
/// Measured, on the fragment `landav-its` emits: a countdown loop solves in
/// 0.18 s, a nested `range(n)` pair in 0.63 s, a triangular loop in 0.61 s,
/// nested loops over two parameters in 0.50 s. The default is roughly fifty
/// times the slowest of those, which is enough headroom that a machine under
/// load does not turn an answer into a timeout, and small enough that a
/// function which is genuinely out of reach costs half a minute rather than a
/// job.
///
/// It is a **per-invocation** cap, deliberately. A whole-repository budget is
/// a different quantity, owned by whoever runs the walk, because it depends on
/// how many functions there are and whether they run in parallel - neither of
/// which this crate can see.
///
/// # Two clocks, not one
///
/// KoAT is also given `--timeout`, set strictly below this one (see
/// [`crate::Solver::argv`]). In the ordinary slow case KoAT therefore stops
/// itself and prints `TIMEOUT:`, which is a clean, attributable outcome. This
/// crate's clock is the backstop for a solver that ignores its own - and for
/// LoAT, which has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timeout(u64);

impl Timeout {
    /// Thirty seconds. See the type documentation for the measurements.
    pub const DEFAULT: Self = Self(30);

    /// Validates `seconds` against [`MIN_TIMEOUT_SECS`] and
    /// [`MAX_TIMEOUT_SECS`].
    ///
    /// Zero is refused rather than clamped: it makes [`crate::poll_budget`]
    /// zero, which kills every child before it has started, and a timeout that
    /// can never be met is not a timeout.
    ///
    /// # Errors
    ///
    /// [`SolverError::TimeoutOutOfRange`] if `seconds` is outside the
    /// permitted range.
    pub const fn new(seconds: u64) -> Result<Self, SolverError> {
        if seconds < MIN_TIMEOUT_SECS || seconds > MAX_TIMEOUT_SECS {
            return Err(SolverError::TimeoutOutOfRange {
                got: seconds,
                min: MIN_TIMEOUT_SECS,
                max: MAX_TIMEOUT_SECS,
            });
        }
        Ok(Self(seconds))
    }

    /// The validated budget in whole seconds.
    #[must_use]
    pub const fn seconds(self) -> u64 {
        self.0
    }

    /// The validated budget as a [`Duration`].
    #[must_use]
    pub const fn duration(self) -> Duration {
        Duration::from_secs(self.0)
    }
}

impl Default for Timeout {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl core::fmt::Display for Timeout {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}s", self.0)
    }
}

/// How many times [`crate::run`] polls a child before it gives up on it.
///
/// # Why the wait loop counts instead of watching a clock
///
/// The obvious spelling is `while Instant::now() < deadline { ... }`. It has a
/// failure mode this repository has already paid for once: a mutation that
/// weakens the comparison does not make a test fail, it makes the loop **run
/// forever**, and a hang is invisible to the panic lints and indistinguishable
/// from slow CI. `landav-bound/tests/frozen_invariants.rs` exists because of
/// exactly that class of survivor.
///
/// So the loop counts to this number, which is a pure function of the timeout
/// and can be asserted on without spawning anything. The clock is still
/// consulted - it is what makes the loop stop *early* when the child finishes
/// - but it is no longer the only thing that makes it stop at all.
#[must_use]
pub fn poll_budget(timeout: Timeout) -> u32 {
    let millis = timeout.seconds().saturating_mul(1000);
    let polls = millis / POLL_INTERVAL_MILLIS;
    // `try_from` rather than `as`: a silent truncation here would turn a long
    // timeout into a short one, and `cast_possible_truncation` is denied at
    // the workspace level for exactly that reason. At least one poll, or a
    // child that finished instantly would still be reported as a timeout.
    u32::try_from(polls).unwrap_or(u32::MAX).max(1)
}
