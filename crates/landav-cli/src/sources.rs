//! Resolving a target path into the set of source files to analyse.
//!
//! # Everything the walk cannot resolve is a failure, not a skip
//!
//! A walk that shrugs at an entry it cannot handle and carries on produces a
//! verdict over a subset of the tree while reporting it as a verdict over the
//! tree. That is the criterion 3 failure arriving through the filesystem
//! instead of through the solver, so a symlink loop, an unreadable directory
//! or a broken link stops the run and names itself.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::diagnostic::ToolError;

/// The extension the Python frontend recognises.
const PYTHON_EXTENSION: &str = "py";

/// Collect the source files under `target`.
///
/// Returns them sorted, so that two runs over the same tree analyse the same
/// files in the same order. `read_dir` order is filesystem-dependent, and an
/// exit code that depends on it is not a contract.
///
/// An empty result is *not* an error here — the caller decides what "nothing
/// to analyse" means for the run as a whole — but every other resolution
/// failure is.
///
/// # Errors
///
/// [`ToolError`] if the target cannot be resolved, is neither a directory nor
/// a Python file, or if any entry beneath it cannot be traversed.
pub fn collect(target: &Path) -> Result<Vec<PathBuf>, ToolError> {
    // `metadata` follows symlinks, which is what makes a symlink loop show up
    // here as `ELOOP` rather than as a walk that never returns.
    let meta = std::fs::metadata(target)
        .map_err(|err| ToolError::at_path(target, format!("cannot be resolved: {err}")))?;

    if meta.is_file() {
        if !is_python(target) {
            return Err(ToolError::at_path(
                target,
                format!(
                    "is not a Python source file; expected a .{PYTHON_EXTENSION} file or a directory"
                ),
            ));
        }
        return Ok(vec![target.to_path_buf()]);
    }

    if !meta.is_dir() {
        return Err(ToolError::at_path(
            target,
            "is neither a regular file nor a directory, so there is nothing to analyse",
        ));
    }

    let mut found = Vec::new();
    let mut visited = BTreeSet::new();
    walk(target, &mut visited, &mut found)?;
    found.sort();
    Ok(found)
}

/// Recursively collect Python files under `dir`.
///
/// `visited` holds the canonical path of every directory already entered. A
/// directory whose canonical path is already present is a cycle: a naive walk
/// would recurse until the stack or the path length gave out, and neither is
/// an exit code.
fn walk(
    dir: &Path,
    visited: &mut BTreeSet<PathBuf>,
    found: &mut Vec<PathBuf>,
) -> Result<(), ToolError> {
    let canonical = std::fs::canonicalize(dir)
        .map_err(|err| ToolError::at_path(dir, format!("cannot be resolved: {err}")))?;
    if !visited.insert(canonical) {
        return Err(ToolError::at_path(
            dir,
            "is a symbolic link back into a directory already being walked; \
             the tree cannot be traversed, so no statement about it is available",
        ));
    }

    let entries = std::fs::read_dir(dir)
        .map_err(|err| ToolError::at_path(dir, format!("cannot be listed: {err}")))?;

    for entry in entries {
        let entry =
            entry.map_err(|err| ToolError::at_path(dir, format!("cannot be listed: {err}")))?;
        let path = entry.path();

        // `entry.file_type` does not follow links, so a link is resolved
        // explicitly and its failure blamed on the link itself.
        let file_type = entry
            .file_type()
            .map_err(|err| ToolError::at_path(&path, format!("cannot be inspected: {err}")))?;
        let resolved = if file_type.is_symlink() {
            std::fs::metadata(&path).map_err(|err| {
                ToolError::at_path(&path, format!("is a link that cannot be resolved: {err}"))
            })?
        } else {
            entry
                .metadata()
                .map_err(|err| ToolError::at_path(&path, format!("cannot be inspected: {err}")))?
        };

        if resolved.is_dir() {
            walk(&path, visited, found)?;
        } else if resolved.is_file() && is_python(&path) {
            found.push(path);
        }
    }
    Ok(())
}

/// Whether `path` is a file the Python frontend recognises.
fn is_python(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(PYTHON_EXTENSION))
}

#[cfg(test)]
mod tests {
    use super::{collect, is_python};
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
}
