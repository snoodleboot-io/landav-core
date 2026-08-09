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

/// What the walk found, and what it could not look at.
#[derive(Debug, Default)]
pub struct Walk {
    /// Python files to analyse, sorted.
    pub sources: Vec<PathBuf>,
    /// Paths the walk could not resolve, sorted by their diagnostic.
    pub problems: Vec<ToolError>,
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
pub fn collect(target: &Path) -> Result<(Target, Walk), ToolError> {
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
            },
        ));
    }

    if !meta.is_dir() {
        return Err(ToolError::at_path(
            target,
            "is neither a regular file nor a directory, so there is nothing to analyse",
        ));
    }

    let mut walk = Walk::default();
    match dir_id(target, &meta) {
        Ok(id) => descend(target, id, &mut walk),
        Err(problem) => walk.problems.push(problem),
    }
    walk.sources.sort();
    walk.problems.sort_by_key(ToolError::to_string);
    Ok((Target::Directory, walk))
}

/// Walk the tree beneath `root`.
///
/// Iterative rather than recursive: nesting depth is attacker controlled, and
/// a stack overflow is a signal death with no exit code. Each directory's
/// entries are read into memory and the handle dropped before descending, so
/// the walk holds one open directory at a time rather than one per level.
fn descend(root: &Path, root_id: DirId, walk: &mut Walk) {
    let mut frames: Vec<Frame> = Vec::new();
    if let Some(pending) = read_level(root, walk) {
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

        if let Some(pending) = read_level(&path, walk) {
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
fn read_level(dir: &Path, walk: &mut Walk) -> Option<Vec<(PathBuf, DirId)>> {
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
            // Note this runs for a directory *named* `something.py` too, which
            // is a directory and not a file that failed to be one.
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
    use super::{Target, collect, is_python};
    use std::path::Path;

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
        let err = collect(Path::new("definitely/absent/no_such_file.py"))
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(err.contains("no_such_file.py"), "{err}");
    }

    #[test]
    fn a_directory_target_is_distinguished_from_a_file_target() {
        let kind = collect(Path::new(".")).ok().map(|(kind, _)| kind);
        assert_eq!(kind, Some(Target::Directory));
    }
}
