//! [`Config`] - where the solvers are, how long they get, and where they work.

use std::path::{Path, PathBuf};

use crate::{solver::Solver, timeout::Timeout};

/// How [`crate::run`] invokes a solver.
///
/// # Discovery is `PATH`, and deliberately nothing cleverer
///
/// The default program is a bare name (`koat2`, `loat`), resolved by the
/// operating system's own `PATH` lookup at `exec` time. There is no bundled
/// binary, no download step, no vendored source and no search of well-known
/// directories.
///
/// For LoAT that is not a convenience, it is the licence boundary: LoAT is
/// GPL-3.0 and `landav-ee` is a commercial BSL 1.1 product, so LoAT must be a
/// program the *user* installed and this crate invokes, never a component
/// distributed with landav. See [`Solver`].
///
/// [`Config::with_program`] exists for the case where a solver is installed
/// somewhere unusual, and for tests that need a stub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Indexed the same way [`Solver::ALL`] is ordered.
    programs: [PathBuf; 2],
    timeout: Timeout,
    workspace_root: Option<PathBuf>,
}

impl Config {
    /// The program to run for `solver`.
    #[must_use]
    pub fn program(&self, solver: Solver) -> &Path {
        &self.programs[index(solver)]
    }

    /// Run `solver` from `program` instead of from `PATH`.
    #[must_use]
    pub fn with_program<P: AsRef<Path>>(mut self, solver: Solver, program: P) -> Self {
        self.programs[index(solver)] = program.as_ref().to_path_buf();
        self
    }

    /// The wall clock each invocation is held to.
    #[must_use]
    pub const fn timeout(&self) -> Timeout {
        self.timeout
    }

    /// Hold each invocation to `timeout`.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Timeout) -> Self {
        self.timeout = timeout;
        self
    }

    /// Where per-run working directories are created.
    ///
    /// [`std::env::temp_dir`] by default. Worth overriding when the system
    /// temp directory is read-only, is on a filesystem shared with other
    /// tenants, or is too small for the input - all ordinary CI conditions.
    #[must_use]
    pub fn workspace_root(&self) -> PathBuf {
        self.workspace_root
            .clone()
            .unwrap_or_else(std::env::temp_dir)
    }

    /// Create per-run working directories under `root`.
    #[must_use]
    pub fn with_workspace_root<P: AsRef<Path>>(mut self, root: P) -> Self {
        self.workspace_root = Some(root.as_ref().to_path_buf());
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            programs: [
                PathBuf::from(Solver::Koat.program()),
                PathBuf::from(Solver::Loat.program()),
            ],
            timeout: Timeout::DEFAULT,
            workspace_root: None,
        }
    }
}

/// Where `solver` sits in [`Config::programs`].
const fn index(solver: Solver) -> usize {
    match solver {
        Solver::Koat => 0,
        Solver::Loat => 1,
    }
}
