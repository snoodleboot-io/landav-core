//! [`Solver`] - which external program is being asked, and how.

use std::ffi::OsString;
use std::path::Path;

use landav_bound::{Origin, Symbol};

use crate::{
    KOAT_TIMEOUT_GRACE_SECS, answer::Answer, arg_map::ArgMap, direction::Direction, koat_answer,
    loat_answer, report::Report, solver_error::SolverError, timeout::Timeout,
};

/// One external complexity solver.
///
/// # Both are invoked across a process boundary, and that is architectural
///
/// **LoAT is GPL-3.0**, forced by a statically linked GPL Yices 2.
/// `landav-core` is Apache-2.0 and `landav-ee` is a commercial BSL 1.1
/// product. Under the FSF's own test, two programs communicating through
/// pipes, sockets and command-line arguments are separate works, while two
/// sharing an address space are one. **Linking to LoAT, binding to it through
/// FFI, embedding it, or vendoring its source would propagate GPL-3.0 onto
/// `landav-core` and fatally onto `landav-ee`.**
///
/// So the system is handed over as a *file*, the options as *arguments*, and
/// the answer comes back on *stdout*. There is no other interface, and there
/// is no performance argument that can buy one: a process spawn costs
/// milliseconds against solver runs measured in hundreds of them, and the
/// saving would be paid for by relicensing a commercial product.
///
/// KoAT2 is MIT and carries no such constraint. The mechanism is uniform
/// anyway, because a bridge with one linked solver and one spawned solver is a
/// bridge where the licence-critical property is a special case rather than
/// the rule - and special cases are what get optimised away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Solver {
    /// KoAT2 - upper bounds. MIT.
    Koat,
    /// LoAT - lower bounds. **GPL-3.0, and retired.**
    ///
    /// Retained so the answer parser and the licence-boundary tests still have
    /// a subject, **not** because anything invokes it. landav does not depend
    /// on LoAT and must not: the copyleft is forced by a statically linked
    /// Yices 2 and CLN, and it would reach both the Apache-2.0 core and the
    /// commercial offering beside it.
    ///
    /// Lower bounds come from `landav-engine`, which derives exact `Theta` for
    /// the counted-loop fragment from source structure with no solver at all.
    /// See [`crate::loat_answer`] for the full reasoning.
    Loat,
}

impl Solver {
    /// Both solvers, in a fixed order.
    pub const ALL: [Self; 2] = [Self::Koat, Self::Loat];

    /// The executable's default name, looked up on `PATH`.
    #[must_use]
    pub const fn program(self) -> &'static str {
        match self {
            Self::Koat => "koat2",
            Self::Loat => "loat",
        }
    }

    /// Which side of the runtime this solver bounds.
    ///
    /// A property of the program, not of its output. See [`Direction`].
    #[must_use]
    pub const fn direction(self) -> Direction {
        match self {
            Self::Koat => Direction::Upper,
            Self::Loat => Direction::Lower,
        }
    }

    /// The file extension the solver expects on its input.
    ///
    /// Both are given the same KoAT-format text. LoAT selects its parser by
    /// extension, so this is the one place the two could ever differ.
    #[must_use]
    pub const fn input_extension(self) -> &'static str {
        match self {
            Self::Koat | Self::Loat => "koat",
        }
    }

    /// What to tell a user whose machine does not have this solver.
    #[must_use]
    pub const fn install_hint(self) -> &'static str {
        match self {
            Self::Koat => {
                "Install KoAT2 (https://github.com/aprove-developers/KoAT2-Releases) \
                 and put `koat2` on PATH."
            }
            Self::Loat => {
                "Install LoAT (https://loat-developers.github.io/LoAT/) and put `loat` \
                 on PATH. LoAT is GPL-3.0 and is invoked as a separate process; it is \
                 never linked into landav."
            }
        }
    }

    /// The argument vector for `input`, under `budget`.
    ///
    /// # Two elements of the KoAT vector are load bearing
    ///
    /// `--preprocessors` is passed **without `eliminate`**. KoAT's default
    /// preprocessing removes variables that "do not contribute to the problem"
    /// and then renumbers the survivors, so a system declaring
    /// `(VAR vaaa vi vn)` whose `vaaa` is never read answers about `Arg_1`
    /// where this crate expects `Arg_2`. Every positional mapping downstream
    /// would then be off by the number of variables eliminated before it - and
    /// the result is a bound attributed to the wrong variable, which is a
    /// wrong answer that looks entirely right.
    ///
    /// This is not a theoretical hazard. Measured on the ITS `landav-its`
    /// emits for
    ///
    /// ```python
    /// def nested(n: int) -> None:
    ///     total: int = 0
    ///     for i in range(n):
    ///         for j in range(n):
    ///             total = total + 1
    /// ```
    ///
    /// whose variable tuple is
    /// `(for.counter#0, for.counter#2, for.limit#1, for.limit#3, i, j, n, total)`:
    ///
    /// | preprocessors | KoAT's answer | reads as |
    /// |---|---|---|
    /// | with `eliminate` (default) | `3*Arg_4^2+7*Arg_4+5` | `3i^2 + 7i + 5` — **the loop counter** |
    /// | without `eliminate` | `3*Arg_6^2+8*Arg_6+6` | `3n^2 + 8n + 6` — the parameter |
    ///
    /// Both are correct bounds on the *system*; only the second can be read
    /// back onto the function. The triangular loop shows the same shift onto
    /// `Arg_5`, and a countdown loop - whose variables are all live - is
    /// unaffected, which is precisely why this cannot be caught by testing
    /// simple cases.
    ///
    /// The cost of disabling elimination is a slightly looser constant
    /// (`8n+6` against `7n+5` above) and no change of complexity class on any
    /// system this crate has been run against. That is a good trade against
    /// attributing a bound to the wrong variable.
    ///
    /// `--timeout` is KoAT's own clock, set [`KOAT_TIMEOUT_GRACE_SECS`] below
    /// this crate's wall clock so that the ordinary slow case ends in KoAT
    /// printing `TIMEOUT:` - an orderly, attributable outcome - rather than in
    /// this crate killing the process. It never reaches zero, which KoAT reads
    /// as "no limit".
    ///
    /// LoAT has no timeout option at all, so its vector carries none and this
    /// crate's wall clock is the only thing that stops it.
    #[must_use]
    pub fn argv(self, input: &Path, budget: Timeout) -> Vec<OsString> {
        match self {
            Self::Koat => {
                let inner = budget
                    .seconds()
                    .saturating_sub(KOAT_TIMEOUT_GRACE_SECS)
                    .max(1);
                vec![
                    OsString::from("analyse"),
                    OsString::from("--preprocessors=invgen,sat,reachable,tmp"),
                    OsString::from(format!("--timeout={inner}")),
                    OsString::from("-i"),
                    input.as_os_str().to_owned(),
                ]
            }
            Self::Loat => vec![
                OsString::from("--mode"),
                OsString::from("complexity"),
                input.as_os_str().to_owned(),
            ],
        }
    }

    /// Read this solver's standard output.
    ///
    /// # Errors
    ///
    /// Whatever the solver's own parser refuses; see [`crate::koat_answer`]
    /// and [`crate::loat_answer`].
    pub fn parse(self, stdout: &str, map: &ArgMap) -> Result<Answer, SolverError> {
        match self {
            Self::Koat => koat_answer::parse(stdout, map),
            // LoAT's answer is a growth class and carries no `Arg_i`, so the
            // positional map has nothing to contribute to it.
            Self::Loat => loat_answer::parse(stdout),
        }
    }

    /// Pair `answer` with the function it is about, computing its blame.
    ///
    /// Exposed because it is the whole of what [`crate::run`] does once the
    /// child has exited, and testing it needs no child.
    #[must_use]
    pub fn report(
        self,
        answer: Answer,
        raw: impl Into<String>,
        function: impl Into<Symbol>,
        origin: impl Into<Symbol>,
        map: &ArgMap,
    ) -> Report {
        Report::new(self, answer, raw, function, Origin::new(origin), map)
    }
}

impl core::fmt::Display for Solver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Koat => "KoAT",
            Self::Loat => "LoAT",
        })
    }
}
