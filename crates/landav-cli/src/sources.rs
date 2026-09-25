//! Resolving a target path into the set of source files to analyse.
//!
//! # Everything the walk cannot resolve is a failure, not a skip
//!
//! A walk that shrugs at an entry it cannot handle and carries on produces a
//! verdict over a subset of the tree while reporting it as a verdict over the
//! tree. That is the criterion 3 failure arriving through the filesystem
//! instead of through the solver, so a symlink loop, an unreadable directory,
//! a dangling `.py` link or a `.py` path that is not a regular file all stop
//! the run and name themselves.
//!
//! # Every problem is reported, not just the first
//!
//! The walk collects failures rather than returning at the first one. Two
//! reasons. An operator fixing a broken checkout wants the whole list, not one
//! item per run. And which failure comes first is decided by directory
//! iteration order, so a walk that reported only the first would name a
//! different path on a different filesystem for identical input.
//!
//! # Identity is `(device, inode)`, and the ancestor set is the recursion path
//!
//! A cycle is a directory that is its own ancestor. It is *not* a directory
//! reachable by two routes: `pkg/` beside `alias -> pkg` is a DAG, traverses
//! fine, and is an ordinary monorepo shape. Tracking every directory ever
//! entered would report that healthy tree as a loop, so the ancestor set is
//! the current path and entries leave it on the way back out.
//!
//! Identity comes from metadata the walk has already read. Canonicalising each
//! directory instead costs one `readlink` per path component, which makes the
//! whole walk cubic in nesting depth — and nesting depth is attacker
//! controlled on a pull-request gate, where a timed-out job carries no exit
//! code at all.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::diagnostic::ToolError;

/// The extension the Python frontend recognises.
const PYTHON_EXTENSION: &str = "py";

/// Whether the caller named a file or a directory.
///
/// The distinction matters to the "nothing was analysed" rule: a directory
/// that holds no code may be a path that stopped matching, while a file the
/// caller named by hand demonstrably exists and was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The caller named one file.
    File,
    /// The caller named a directory, and the walk expanded it.
    Directory,
}

/// What the walk found, what it could not look at, and what it chose not to.
#[derive(Debug, Default)]
pub struct Walk {
    /// Python files to analyse, sorted.
    pub sources: Vec<PathBuf>,
    /// Paths the walk could not resolve, sorted by their diagnostic.
    pub problems: Vec<ToolError>,
    /// Directories the walk declined to enter, sorted by path. `LAN-111`.
    ///
    /// Recorded rather than dropped for the reason a waiver that suppressed
    /// nothing is recorded: a skip nobody can see is a skip nobody can
    /// question, and the one this exists for hid 42,831 of 43,203 functions.
    pub skipped: Vec<Skipped>,
}

/// A directory the walk declined to enter, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The directory, as the walk reached it.
    pub path: PathBuf,
    /// The rule that matched.
    pub reason: SkipReason,
}

/// Why a directory was not entered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// It holds a `pyvenv.cfg`, so it is a virtual environment whatever it is
    /// called.
    Virtualenv,
    /// Its name is one every Python tool skips: a dependency directory or a
    /// build product, never the project's own source.
    Vendored,
    /// Its name is a cache or version-control directory, which cannot hold
    /// source anyone wrote.
    Cache,
    /// It matched an `exclude` pattern in the configuration.
    Configured(String),
}

impl SkipReason {
    /// Whether the report should name the directory on its own line.
    ///
    /// A virtualenv, a `node_modules`, a `dist`, an `exclude` match: each is a
    /// place a reader may have *wanted* analysed, so it is named. A
    /// `__pycache__` in every package directory is not, and naming forty of
    /// them would bury the one line that matters.
    #[must_use]
    pub const fn is_reported_by_name(&self) -> bool {
        !matches!(self, Self::Cache)
    }
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Virtualenv => f.write_str("a virtual environment"),
            Self::Vendored => f.write_str("a dependency or build directory"),
            Self::Cache => f.write_str("a cache or version-control directory"),
            Self::Configured(pattern) => write!(f, "excluded by `{pattern}` in the configuration"),
        }
    }
}

/// Which directories the walk enters. `LAN-111`.
///
/// # The default is what every other Python tool does
///
/// `ruff`, `black`, `mypy` and `pytest` all decline to enter a virtualenv,
/// `node_modules`, `build`, `dist` and the caches, because a user asking about
/// their project is never asking about their dependencies. landav did not,
/// and on the one application tree it was measured against, 42,831 of the
/// 43,203 functions in the coverage denominator were `site-packages`. The
/// project's own 372 were invisible in the summary.
///
/// # What the rules never apply to
///
/// The target itself. `landav check .venv` is a request to analyse the venv,
/// and the walk honours it; the rules run on directories *beneath* the target
/// only. Naming a thing is asking for it.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// Enter virtualenvs and vendored directories anyway.
    ///
    /// The caches are still skipped: there is nothing in a `__pycache__` that
    /// anybody wrote, whatever the flag says.
    pub include_vendored: bool,
    /// Patterns from the configuration's `exclude`, in the waiver glob
    /// dialect ([`landav_python::path_matches`]), applied whatever
    /// `include_vendored` says: an exclusion the user wrote is not vendoring.
    pub exclude: Vec<String>,
}

/// Directory names that are dependencies or build products, never source.
const VENDORED: [&str; 8] = [
    ".venv",
    "venv",
    "site-packages",
    "node_modules",
    "__pypackages__",
    ".eggs",
    "build",
    "dist",
];

/// Directory names that hold caches or version control.
const CACHE: [&str; 9] = [
    "__pycache__",
    ".git",
    ".hg",
    ".svn",
    ".tox",
    ".nox",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
];

/// The file CPython writes at the root of every virtual environment.
const PYVENV: &str = "pyvenv.cfg";

impl Policy {
    /// Why `dir` should not be entered, or `None` to enter it.
    ///
    /// Configured exclusions are checked first and unconditionally. The
    /// `pyvenv.cfg` test comes before the name list because a venv called
    /// `tooling` is still a venv, and the name list exists for the trees where
    /// nothing so reliable is there to read.
    #[must_use]
    pub fn skip(&self, dir: &Path) -> Option<SkipReason> {
        if let Some(pattern) = self
            .exclude
            .iter()
            .find(|pattern| landav_python::path_matches(pattern, dir))
        {
            return Some(SkipReason::Configured(pattern.clone()));
        }
        let name = dir.file_name()?.to_string_lossy();
        if CACHE.contains(&name.as_ref()) {
            return Some(SkipReason::Cache);
        }
        if self.include_vendored {
            return None;
        }
        if dir.join(PYVENV).is_file() {
            return Some(SkipReason::Virtualenv);
        }
        if VENDORED.contains(&name.as_ref()) {
            return Some(SkipReason::Vendored);
        }
        None
    }
}

/// A directory's identity on the filesystem.
///
/// On Unix this is `(device, inode)`, read from metadata the walk already
/// holds. Elsewhere it falls back to a canonical path, which is correct but
/// costs a resolution per directory.
#[cfg(unix)]
type DirId = (u64, u64);
/// A directory's identity on the filesystem.
#[cfg(not(unix))]
type DirId = PathBuf;

/// Identify a directory for cycle detection.
#[cfg(unix)]
fn dir_id(_path: &Path, meta: &std::fs::Metadata) -> Result<DirId, ToolError> {
    use std::os::unix::fs::MetadataExt as _;
    Ok((meta.dev(), meta.ino()))
}

/// Identify a directory for cycle detection.
#[cfg(not(unix))]
fn dir_id(path: &Path, _meta: &std::fs::Metadata) -> Result<DirId, ToolError> {
    std::fs::canonicalize(path)
        .map_err(|err| ToolError::at_path(path, format!("cannot be resolved: {err}")))
}

/// Whether `path` is itself a symbolic link, rather than what it resolves to.
///
/// A failure to `lstat` answers `false`: the caller has already resolved the
/// path through [`std::fs::metadata`], so the entry exists, and the honest
/// reading of "cannot tell" here is to treat it as the ordinary directory the
/// resolution says it is. Skipping on a failed check would drop a real
/// directory's contents on a transient error.
fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// A source file's identity on the filesystem.
///
/// The same notion [`DirId`] carries for a directory, and deliberately not a
/// canonical path: two **hard links** to one inode are two distinct canonical
/// paths, so a canonical-path key analyses that file twice and reports it
/// twice. `(device, inode)` sees it, and sees a bind mount too.
#[cfg(unix)]
type FileId = (u64, u64);

/// A source file's identity on the filesystem: a canonical path where inode
/// numbers are not available.
#[cfg(not(unix))]
type FileId = PathBuf;

/// Identify a source file, or `None` if it cannot be resolved.
#[cfg(unix)]
fn file_id(path: &Path) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata(path)
        .ok()
        .map(|meta| (meta.dev(), meta.ino()))
}

/// Identify a source file, or `None` if it cannot be resolved.
#[cfg(not(unix))]
fn file_id(path: &Path) -> Option<FileId> {
    std::fs::canonicalize(path).ok()
}

/// Drop every source that names a file already in the list.
///
/// # Why this is not subsumed by the walk policy
///
/// Not descending a directory symlink fixes the case that prompted `LAN-95` -
/// a venv's `lib64 -> lib` - and it is a rule about *directories*. Two paths
/// can still reach one file without one: a `.py` symlink to a file inside the
/// target (which the walk follows deliberately, because it is source), a hard
/// link, a bind mount. Keying on the file's identity is what makes the count
/// *correct* rather than usually-correct, and it is what a consumer comparing
/// two runs needs.
///
/// The surviving path is the lexicographically first, because the list is
/// sorted before this runs - so which of two names for one file gets reported
/// does not depend on `read_dir` order, and two runs over one tree report the
/// same path.
///
/// A path that cannot be identified is **kept**, under its own name. It was
/// resolvable a moment ago or it would not be in this list; failing to
/// deduplicate is a file analysed twice, and dropping it is a file analysed
/// never, which is the direction that turns a defect into silence.
fn deduplicate(sources: &mut Vec<PathBuf>) {
    let mut seen: BTreeSet<FileId> = BTreeSet::new();
    sources.retain(|path| file_id(path).is_none_or(|id| seen.insert(id)));
}

/// One directory being walked, and the subdirectories it still owes.
struct Frame {
    /// Identity, for cycle detection against the rest of the path.
    id: DirId,
    /// Subdirectories not yet entered.
    pending: std::vec::IntoIter<(PathBuf, DirId)>,
}

/// Collect the source files under `target`.
///
/// Sources come back sorted, so that two runs over the same tree analyse the
/// same files in the same order. Directory iteration order is
/// filesystem-dependent, and an exit code that depends on it is not a
/// contract.
///
/// An empty result is *not* an error here — the caller decides what "nothing
/// to analyse" means for the run as a whole — but every resolution failure is
/// recorded in [`Walk::problems`].
///
/// # Errors
///
/// [`ToolError`] if the target itself cannot be resolved, or is neither a
/// directory nor a Python file. Failures *beneath* the target are collected
/// rather than returned.
pub fn collect(target: &Path, policy: &Policy) -> Result<(Target, Walk), ToolError> {
    // `metadata` follows symlinks, which is what makes a symlink loop show up
    // here as `ELOOP` rather than as a walk that never returns.
    let meta = std::fs::metadata(target)
        .map_err(|err| ToolError::at_path(target, format!("cannot be resolved: {err}")))?;

    if meta.is_file() {
        if !is_python(target) {
            return Err(ToolError::at_path(
                target,
                format!(
                    "is not a Python source file; expected a .{PYTHON_EXTENSION} file \
                     or a directory"
                ),
            ));
        }
        return Ok((
            Target::File,
            Walk {
                sources: vec![target.to_path_buf()],
                problems: Vec::new(),
                skipped: Vec::new(),
            },
        ));
    }

    if !meta.is_dir() {
        return Err(ToolError::at_path(
            target,
            "is neither a regular file nor a directory, so there is nothing to analyse",
        ));
    }

    // Naming a thing is asking for it. `landav check .venv` names a venv, and
    // a walk that entered it only to skip the `site-packages` inside would
    // analyse nothing and call that honouring the request. So a target that
    // is itself a venv or a vendored directory lifts those rules for the whole
    // walk beneath it; the caches stay out, and a configured `exclude` still
    // applies, because the user wrote that one.
    let asked_for_vendored = matches!(
        policy.skip(target),
        Some(SkipReason::Virtualenv | SkipReason::Vendored)
    );
    let effective = Policy {
        include_vendored: policy.include_vendored || asked_for_vendored,
        exclude: policy.exclude.clone(),
    };
    let policy = &effective;

    let mut walk = Walk::default();
    match dir_id(target, &meta) {
        Ok(id) => descend(target, id, &mut walk, policy),
        Err(problem) => walk.problems.push(problem),
    }
    walk.sources.sort();
    deduplicate(&mut walk.sources);
    walk.problems.sort_by_key(ToolError::to_string);
    walk.skipped.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((Target::Directory, walk))
}

/// Walk the tree beneath `root`.
///
/// Iterative rather than recursive: nesting depth is attacker controlled, and
/// a stack overflow is a signal death with no exit code. Each directory's
/// entries are read into memory and the handle dropped before descending, so
/// the walk holds one open directory at a time rather than one per level.
fn descend(root: &Path, root_id: DirId, walk: &mut Walk, policy: &Policy) {
    let mut frames: Vec<Frame> = Vec::new();
    if let Some(pending) = read_level(root, walk, policy) {
        frames.push(Frame {
            id: root_id,
            pending: pending.into_iter(),
        });
    }

    while let Some(frame) = frames.last_mut() {
        let Some((path, id)) = frame.pending.next() else {
            frames.pop();
            continue;
        };

        // A cycle is a directory that is its own ancestor. Two routes to one
        // directory are a DAG and traverse fine.
        if frames.iter().any(|open| open.id == id) {
            walk.problems.push(ToolError::at_path(
                &path,
                "is a symbolic link back into a directory that encloses it, so the \
                 tree cannot be traversed and no statement about it is available",
            ));
            continue;
        }

        if let Some(pending) = read_level(&path, walk, policy) {
            frames.push(Frame {
                id,
                pending: pending.into_iter(),
            });
        }
    }
}

/// Read one directory: record its Python files and problems, return its
/// subdirectories.
///
/// Returns `None` if the directory could not be listed at all, having recorded
/// that as a problem — the files under it were never enumerated, so the run
/// covers less than the target it names.
fn read_level(dir: &Path, walk: &mut Walk, policy: &Policy) -> Option<Vec<(PathBuf, DirId)>> {
    let listing = match std::fs::read_dir(dir) {
        Ok(listing) => listing,
        Err(err) => {
            walk.problems
                .push(ToolError::at_path(dir, format!("cannot be listed: {err}")));
            return None;
        }
    };

    let mut names: Vec<PathBuf> = Vec::new();
    for entry in listing {
        match entry {
            Ok(entry) => names.push(entry.path()),
            Err(err) => walk
                .problems
                .push(ToolError::at_path(dir, format!("cannot be listed: {err}"))),
        }
    }
    // Sorted so the report is stable, and dropped before descending so the
    // walk never holds more than one open directory handle.
    names.sort();

    let mut subdirectories = Vec::new();
    for path in names {
        // Deliberately follows links: a `.py` symlink to a real file is source.
        let meta = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(err) => {
                // A path that claims to be Python and does not resolve cannot
                // be waved through — it may be the file the verdict was about.
                // Something that is not source and does not resolve is not
                // this run's business, unless even `lstat` fails, in which
                // case nothing is known about it at all.
                if is_python(&path) || std::fs::symlink_metadata(&path).is_err() {
                    walk.problems.push(ToolError::at_path(
                        &path,
                        format!("cannot be resolved: {err}"),
                    ));
                }
                continue;
            }
        };

        if meta.is_dir() {
            // A directory reached through a symbolic link is **not** descended
            // into. `LAN-95`: a CPython virtual environment contains
            // `lib64 -> lib`, so following it analysed every file under
            // `.venv/lib` twice - once under each path - and doubled the file
            // count, the coverage denominator, every `--resource` total, the
            // findings a CI gate counts, and the wall time. `ruff`, `mypy` and
            // `pytest` all decline for the same reason.
            //
            // Silent rather than a recorded problem, because it is a policy and
            // not a failure. Where the link points inside the target, the files
            // are analysed under their real path and nothing is lost; where it
            // points outside, they are no more this run's business than any
            // other file outside the target - which is also what stops
            // `landav check .` being induced to read arbitrary files.
            //
            // This leaves the cycle check below nearly unreachable, since a
            // symbolic link is the ordinary way to make a directory its own
            // ancestor. It is kept because a bind mount is the other way, and a
            // walk that does not terminate has no exit code at all.
            //
            // Note this runs for a directory *named* `something.py` too, which
            // is a directory and not a file that failed to be one.
            if is_link(&path) {
                continue;
            }
            // A directory the policy declines: recorded, never entered. The
            // target itself never reaches here, so naming a venv analyses it.
            if let Some(reason) = policy.skip(&path) {
                walk.skipped.push(Skipped { path, reason });
                continue;
            }
            match dir_id(&path, &meta) {
                Ok(id) => subdirectories.push((path, id)),
                Err(problem) => walk.problems.push(problem),
            }
        } else if !is_python(&path) {
            // Not source. Nothing to say about it.
        } else if meta.is_file() {
            walk.sources.push(path);
        } else {
            // A FIFO, socket or device named `*.py`. It cannot be read without
            // blocking and it is not source, but it is *claiming* to be source,
            // so dropping it silently would let a clean neighbour carry the
            // tree to `0` with two files neither analysed nor blamed.
            walk.problems.push(ToolError::at_path(
                &path,
                "claims to be Python source but is not a regular file (a device, \
                 socket or named pipe); it was never read, so nothing can be \
                 concluded about it",
            ));
        }
    }
    Some(subdirectories)
}

/// Whether `path` is a file the Python frontend recognises.
fn is_python(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(PYTHON_EXTENSION))
}

#[cfg(test)]
mod tests {
    use super::{Policy, SkipReason, Target, collect, is_python};
    use std::path::Path;

    #[test]
    fn a_directory_holding_pyvenv_cfg_is_a_virtualenv_whatever_its_name() {
        let dir = std::env::temp_dir().join(format!("landav-policy-{}", std::process::id()));
        let odd = dir.join("tooling");
        // Asserted rather than unwrapped: the crate forbids the panic lints in
        // library code and this module follows suit, and a failed write would
        // fail the assertion below on its own anyway.
        assert!(std::fs::create_dir_all(&odd).is_ok());
        assert!(std::fs::write(odd.join("pyvenv.cfg"), "home = /usr/bin\n").is_ok());
        let policy = Policy::default();
        assert_eq!(policy.skip(&odd), Some(SkipReason::Virtualenv));
        assert_eq!(policy.skip(&dir.join("src")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_vendored_keeps_the_caches_out() {
        let policy = Policy {
            include_vendored: true,
            exclude: Vec::new(),
        };
        assert_eq!(policy.skip(Path::new("/p/.venv")), None);
        assert_eq!(policy.skip(Path::new("/p/node_modules")), None);
        assert_eq!(
            policy.skip(Path::new("/p/__pycache__")),
            Some(SkipReason::Cache)
        );
        assert_eq!(policy.skip(Path::new("/p/.git")), Some(SkipReason::Cache));
    }

    #[test]
    fn a_configured_exclusion_applies_whatever_the_flag_says() {
        let policy = Policy {
            include_vendored: true,
            exclude: vec!["legacy".to_owned(), "src/generated".to_owned()],
        };
        assert_eq!(
            policy.skip(Path::new("/p/legacy")),
            Some(SkipReason::Configured("legacy".to_owned()))
        );
        assert_eq!(
            policy.skip(Path::new("/p/src/generated")),
            Some(SkipReason::Configured("src/generated".to_owned()))
        );
        assert_eq!(policy.skip(Path::new("/p/src")), None);
    }

    #[test]
    fn python_files_are_recognised_by_extension() {
        assert!(is_python(Path::new("a.py")));
        assert!(is_python(Path::new("/nested/dir/a.PY")));
        assert!(!is_python(Path::new("notes.txt")));
        assert!(!is_python(Path::new("py")));
        assert!(!is_python(Path::new("a.pyc")));
    }

    #[test]
    fn a_target_that_does_not_exist_is_blamed_by_name() {
        let err = collect(
            Path::new("definitely/absent/no_such_file.py"),
            &Policy::default(),
        )
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
        assert!(err.contains("no_such_file.py"), "{err}");
    }

    #[test]
    fn a_directory_target_is_distinguished_from_a_file_target() {
        let kind = collect(Path::new("."), &Policy::default())
            .ok()
            .map(|(kind, _)| kind);
        assert_eq!(kind, Some(Target::Directory));
    }
}
