//! [`Workspace`] - the private directory one solver run works in.

use std::path::{Path, PathBuf};

use crate::solver_error::SolverError;

/// How many distinct names are tried before giving up on the root.
///
/// A collision means another process took the name between this one composing
/// it and creating it, which is exactly what the atomic create is there to
/// detect. A handful of retries covers that; a hundred would mean the root is
/// hostile rather than busy.
const ATTEMPTS: u32 = 8;

/// A private directory holding one run's input and captured output.
///
/// # Why a directory per run rather than a file
///
/// Three files go into it - the system, the solver's stdout, the solver's
/// stderr - and creating one directory atomically is a stronger guarantee than
/// creating three files carefully. Once the directory exists and is ours, the
/// paths inside it cannot have been pre-created by anybody else.
///
/// # Created, never opened
///
/// [`std::fs::create_dir`] fails with `EEXIST` if anything is already at the
/// path, including a symbolic link pointing somewhere else entirely. That is
/// the whole anti-hijack argument, and it is why this uses `create_dir` rather
/// than `create_dir_all`, which succeeds on an existing directory and would
/// happily adopt one somebody else prepared. The same reasoning runs through
/// `landav_cli::sources`: an entry the walk cannot resolve is a failure, not a
/// shrug.
///
/// On Unix the mode is `0o700`, so a shared `/tmp` does not expose the
/// analysed program's structure to other users on the machine.
///
/// # Removed on drop
///
/// Including on the error paths, which are the majority of them. A gate that
/// leaked one directory per analysed function would fill `/tmp` on a monorepo,
/// and what it leaks is the user's source in another form.
#[derive(Debug)]
pub(crate) struct Workspace {
    dir: PathBuf,
}

impl Workspace {
    /// Create a fresh private directory under `root`.
    pub(crate) fn create(root: &Path, label: &str) -> Result<Self, SolverError> {
        let fail = |detail: String| SolverError::Workspace {
            root: root.display().to_string(),
            detail,
        };
        let mut last = String::from("no attempt was made");
        for attempt in 0..ATTEMPTS {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or(0);
            let dir = root.join(format!(
                "landav-solvers-run-{}-{label}-{stamp}-{attempt}",
                std::process::id()
            ));
            match create(&dir) {
                Ok(()) => return Ok(Self { dir }),
                Err(error) => last = error.to_string(),
            }
        }
        Err(fail(last))
    }

    /// A path inside this directory.
    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

/// Create `dir` atomically, private to this user where the platform allows it.
#[cfg(unix)]
fn create(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    std::fs::DirBuilder::new().mode(0o700).create(dir)
}

/// Create `dir` atomically.
///
/// `create_dir` rather than `create_dir_all` here too: the `EEXIST` is the
/// point. Nothing on a non-Unix target tests this arm, and mutation testing
/// reports it as a survivor for that reason - it is not compiled on the
/// platform the suite runs on.
#[cfg(not(unix))]
fn create(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir(dir)
}

impl Drop for Workspace {
    fn drop(&mut self) {
        // Best effort by necessity: `drop` cannot report, and a failure to
        // clean up is not a reason to lose the bound the run produced.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
