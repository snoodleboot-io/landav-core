//! Configuration discovery and loading.
//!
//! Three sources, in strict precedence order:
//!
//! 1. `--config FILE` — an explicit file. It **replaces** discovery rather
//!    than layering on top of it, so a usable `pyproject.toml` can never
//!    rescue an unusable `--config`, and an unusable `pyproject.toml` can
//!    never break a run that named its own configuration.
//! 2. `[tool.landav]` in the nearest `pyproject.toml`, searched for by
//!    ascending from the target.
//! 3. Nothing. Zero config is the default path, not a degraded one: a
//!    checkout that has never heard of landav must get a verdict rather than
//!    a lecture.
//!
//! # Configuration that cannot be honoured is refused, not ignored
//!
//! Every failure here is a [`ToolError`], never a fall back to defaults.
//! Silently discarding configuration the user wrote is worse than rejecting
//! it: the run still reports a verdict, but under settings nobody chose, and
//! nothing in the output says so.
//!
//! # The section is `[tool.landav]`
//!
//! The delivery workbook says `pycost`, the pre-rename working title.
//! `[tool.pycost]` is *not* consulted; honouring it would ship the old name as
//! a supported interface by accident, and supported interfaces are forever.

use std::fmt;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::diagnostic::ToolError;

/// The file discovery looks for, and the only file it looks for.
const PYPROJECT: &str = "pyproject.toml";

/// The section within it. Not `pycost`.
const SECTION: &str = "tool.landav";

/// Where a run's configuration came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// No configuration file was named or found. Criterion 1.
    Defaults,
    /// An explicit `--config` file. Criterion 4.
    Explicit(PathBuf),
    /// A discovered `pyproject.toml`. Criterion 5.
    PyProject(PathBuf),
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Defaults => write!(f, "defaults (no configuration file)"),
            Self::Explicit(path) => write!(f, "{} (--config)", path.display()),
            Self::PyProject(path) => write!(f, "[{}] in {}", SECTION, path.display()),
        }
    }
}

/// The effective configuration for one run.
///
/// # No settings yet
///
/// The key schema for `[tool.landav]` has not been decided, so this carries
/// none. That is *not* the same as not reading the section: an unusable
/// section is refused by [`section_of`], and any key at all is refused by
/// [`reject_unknown_keys`], because a key landav accepts today and ignores is
/// a key the user believes is in effect.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where the configuration came from, for the run summary.
    source: Source,
}

impl Config {
    /// Where this configuration came from.
    pub const fn source(&self) -> &Source {
        &self.source
    }
}

/// Load the configuration for a run over `target`.
///
/// # Errors
///
/// [`ToolError`] if an explicit file is missing, unreadable, not a file, or
/// not valid TOML; or if a discovered `pyproject.toml` is not valid TOML or
/// carries a `[tool.landav]` section that cannot be used.
pub fn load(target: &Path, explicit: Option<&Path>) -> Result<Config, ToolError> {
    match explicit {
        Some(path) => load_explicit(path),
        None => match discover(target) {
            Some(path) => load_pyproject(&path),
            None => Ok(Config {
                source: Source::Defaults,
            }),
        },
    }
}

/// Load an explicit `--config` file.
///
/// A path the user typed and the tool cannot use is a hard failure. Falling
/// back to discovery would run under a configuration they did not ask for and
/// report a verdict for it.
fn load_explicit(path: &Path) -> Result<Config, ToolError> {
    let meta = std::fs::metadata(path)
        .map_err(|err| ToolError::at_path(path, format!("cannot read the --config file: {err}")))?;
    if meta.is_dir() {
        return Err(ToolError::at_path(
            path,
            "--config expects a configuration file, but this is a directory",
        ));
    }

    let document = read_toml(path)?;

    // A `--config` file may be either a landav configuration on its own, or a
    // `pyproject.toml`-shaped document. The presence of a top-level `tool`
    // table decides which, so pointing `--config` at a real `pyproject.toml`
    // does the obvious thing instead of complaining about `[project]`.
    let section = if document.contains_key("tool") {
        section_of(path, &document)?
    } else {
        document
    };
    reject_unknown_keys(path, &section)?;

    Ok(Config {
        source: Source::Explicit(path.to_path_buf()),
    })
}

/// Load `[tool.landav]` from a discovered `pyproject.toml`.
fn load_pyproject(path: &Path) -> Result<Config, ToolError> {
    let document = read_toml(path)?;
    let section = section_of(path, &document)?;
    reject_unknown_keys(path, &section)?;

    Ok(Config {
        source: Source::PyProject(path.to_path_buf()),
    })
}

/// Read and parse a TOML document, blaming the file for anything that fails.
fn read_toml(path: &Path) -> Result<Table, ToolError> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| ToolError::at_path(path, format!("cannot be read: {err}")))?;
    text.parse::<Table>()
        .map_err(|err| ToolError::at_path(path, format!("is not valid TOML: {err}")))
}

/// Extract `[tool.landav]` from a parsed document.
///
/// An absent section is not an error — a `pyproject.toml` belonging entirely
/// to other tools is the overwhelmingly common case, and refusing to run on it
/// would make zero config a fiction. A section that is *present but not a
/// table* is an error: the user wrote configuration, and there is no reading
/// of it under which the run is doing what they asked.
fn section_of(path: &Path, document: &Table) -> Result<Table, ToolError> {
    let Some(tool) = document.get("tool") else {
        return Ok(Table::new());
    };
    let Some(tool) = tool.as_table() else {
        return Err(ToolError::at_path(
            path,
            format!("[tool] must be a table, found {}", type_name(tool)),
        ));
    };
    let Some(section) = tool.get("landav") else {
        return Ok(Table::new());
    };
    match section.as_table() {
        Some(table) => Ok(table.clone()),
        None => Err(ToolError::at_path(
            path,
            format!(
                "[{SECTION}] must be a table of settings, found {}; landav cannot \
                 run under configuration it cannot read",
                type_name(section)
            ),
        )),
    }
}

/// Refuse any key, naming it.
///
/// No setting is defined yet. Accepting a key and ignoring it would let a user
/// write `[tool.landav] fail-on-partial = true`, watch the run report clean,
/// and believe the flag was honoured.
fn reject_unknown_keys(path: &Path, section: &Table) -> Result<(), ToolError> {
    match section.keys().next() {
        None => Ok(()),
        Some(key) => Err(ToolError::at_path(
            path,
            format!(
                "[{SECTION}] sets `{key}`, which landav does not understand; no \
                 settings are defined yet, and a setting that is accepted and \
                 ignored is worse than one that is refused"
            ),
        )),
    }
}

/// The TOML type of `value`, for a diagnostic.
const fn type_name(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "a string",
        Value::Integer(_) => "an integer",
        Value::Float(_) => "a float",
        Value::Boolean(_) => "a boolean",
        Value::Datetime(_) => "a datetime",
        Value::Array(_) => "an array",
        Value::Table(_) => "a table",
    }
}

/// Find the nearest `pyproject.toml` at or above `target`.
///
/// Ascends to the filesystem root. Returns `None` rather than an error: no
/// configuration is the supported default, not a degraded mode.
fn discover(target: &Path) -> Option<PathBuf> {
    let mut dir = discovery_root(target);
    loop {
        let candidate = dir.join(PYPROJECT);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = dir.parent()?.to_path_buf();
    }
}

/// Where the ascent starts.
///
/// The target directory if the target is one, otherwise the directory holding
/// it. A target that does not exist still has a parent worth searching, and if
/// even that is unusable the working directory is the honest fallback.
fn discovery_root(target: &Path) -> PathBuf {
    if target.is_dir() {
        return target.to_path_buf();
    }
    match target.parent() {
        Some(parent) if parent.is_dir() => parent.to_path_buf(),
        _ => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

#[cfg(test)]
mod tests {
    use super::{reject_unknown_keys, section_of};
    use std::path::Path;
    use toml::Table;

    fn parse(text: &str) -> Table {
        text.parse::<Table>().unwrap_or_default()
    }

    #[test]
    fn a_document_without_tool_landav_yields_an_empty_section() {
        let doc = parse("[project]\nname = \"fixture\"\n\n[tool.black]\nline-length = 88\n");
        let section = section_of(Path::new("pyproject.toml"), &doc);
        assert!(section.is_ok_and(|s| s.is_empty()));
    }

    #[test]
    fn an_empty_section_is_indistinguishable_from_no_section() {
        let with = parse("[tool.landav]\n");
        let without = parse("[project]\nname = \"fixture\"\n");
        let path = Path::new("pyproject.toml");
        assert_eq!(
            section_of(path, &with).ok(),
            section_of(path, &without).ok(),
            "an empty [tool.landav] must behave exactly like no section"
        );
    }

    #[test]
    fn a_scalar_where_the_table_belongs_is_refused_by_name() {
        let doc = parse("[project]\nname = \"fixture\"\n\n[tool]\nlandav = 42\n");
        let err = section_of(Path::new("pyproject.toml"), &doc)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            err.contains("landav"),
            "diagnostic must name the section: {err}"
        );
        assert!(err.contains("an integer"), "and say what it found: {err}");
    }

    #[test]
    fn the_pre_rename_pycost_section_is_not_consulted() {
        let doc = parse("[project]\nname = \"fixture\"\n\n[tool]\npycost = 42\n");
        let section = section_of(Path::new("pyproject.toml"), &doc);
        assert!(
            section.is_ok_and(|s| s.is_empty()),
            "[tool.pycost] must be ignored the way any other tool's section is"
        );
    }

    #[test]
    fn an_unknown_key_is_refused_and_named() {
        let section = parse("fail-on-partial = true\n");
        let err = reject_unknown_keys(Path::new("pyproject.toml"), &section)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(err.contains("fail-on-partial"), "{err}");
    }
}
