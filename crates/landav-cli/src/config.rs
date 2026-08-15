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

use landav_python::PathWaiver;
use toml::{Table, Value};

use crate::diagnostic::ToolError;

/// The file discovery looks for, and the only file it looks for.
const PYPROJECT: &str = "pyproject.toml";

/// The section within it. Not `pycost`.
const SECTION: &str = "tool.landav";

/// The only key `[tool.landav]` understands: `LAN-66` criterion 2.
const SUPPRESS: &str = "suppress";

/// The keys one `[[tool.landav.suppress]]` entry understands.
///
/// `E-001` adds `expires` and `approved-by` on the paid side. When it does,
/// this crate learns to *carry* them, not to enforce them — recording an
/// expiry date is bookkeeping, and refusing to run when it has passed is
/// governance. `docs/EDITIONS.md` puts the second half in `landav-ee`.
const WAIVER_KEYS: [&str; 3] = ["path", "rules", "reason"];

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
/// # One setting
///
/// `suppress` — `LAN-66` criterion 2 — and nothing else. Every other key is
/// still refused by [`reject_unknown_keys`], because a key landav accepts
/// today and ignores is a key the user believes is in effect.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where the configuration came from, for the run summary.
    source: Source,
    /// Per-path waivers, in the order they were written.
    waivers: Vec<PathWaiver>,
}

impl Config {
    /// Where this configuration came from.
    pub const fn source(&self) -> &Source {
        &self.source
    }

    /// The per-path waivers this run is operating under.
    ///
    /// Order is the order they were written in the file: the report names them
    /// back to the author, and reordering them would make a config diff and a
    /// report diff disagree.
    pub fn waivers(&self) -> &[PathWaiver] {
        &self.waivers
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
        None => match discover(target)? {
            Some(path) => load_pyproject(&path),
            None => Ok(Config {
                source: Source::Defaults,
                waivers: Vec::new(),
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
    require_regular_file(path, &meta, "--config")?;

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
        waivers: waivers_of(path, &section)?,
    })
}

/// Load `[tool.landav]` from a discovered `pyproject.toml`.
///
/// Discovery has already established that the *name* exists. Everything from
/// here is a hard failure, because a `pyproject.toml` that is sitting right
/// there and cannot be read is not the same as no configuration at all — and
/// reporting "defaults (no configuration file)" for it would be a false
/// statement about the run.
fn load_pyproject(path: &Path) -> Result<Config, ToolError> {
    let meta = std::fs::metadata(path).map_err(|err| {
        ToolError::at_path(
            path,
            format!(
                "is present but cannot be read: {err}; landav will not fall back to \
                 defaults, because that would report a verdict under configuration \
                 nobody chose"
            ),
        )
    })?;
    require_regular_file(path, &meta, "configuration discovery")?;

    let document = read_toml(path)?;
    let section = section_of(path, &document)?;
    reject_unknown_keys(path, &section)?;

    Ok(Config {
        source: Source::PyProject(path.to_path_buf()),
        waivers: waivers_of(path, &section)?,
    })
}

/// Refuse anything that is not a regular file.
///
/// This is what keeps a configuration read from *blocking*. `read_to_string`
/// on a FIFO waits for a writer that may never come, and `/dev/zero` never
/// reaches end of file at all; either way the process produces no exit code,
/// which is strictly worse than producing the wrong one. A hung job is killed
/// by the runner's timeout and triaged as infrastructure flake rather than as
/// a landav error. `metadata` answers the question without opening anything.
fn require_regular_file(path: &Path, meta: &std::fs::Metadata, who: &str) -> Result<(), ToolError> {
    if meta.is_file() {
        return Ok(());
    }
    Err(ToolError::at_path(
        path,
        format!(
            "{who} expects a regular file, but this is {}",
            describe(meta)
        ),
    ))
}

/// A human name for a file type, for a diagnostic.
fn describe(meta: &std::fs::Metadata) -> &'static str {
    if meta.is_dir() {
        "a directory"
    } else if meta.is_symlink() {
        "a symbolic link that does not resolve to one"
    } else {
        "not a regular file (a device, socket or named pipe); reading it could \
         block forever, and a process that never exits has no exit code at all"
    }
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

/// Refuse any key that is not `suppress`, naming it.
///
/// Accepting a key and ignoring it would let a user write
/// `[tool.landav] fail-on-partial = true`, watch the run report clean, and
/// believe the flag was honoured.
fn reject_unknown_keys(path: &Path, section: &Table) -> Result<(), ToolError> {
    match section.keys().find(|key| key.as_str() != SUPPRESS) {
        None => Ok(()),
        Some(key) => Err(ToolError::at_path(
            path,
            format!(
                "[{SECTION}] sets `{key}`, which landav does not understand; the only \
                 setting is `{SUPPRESS}`, and a setting that is accepted and ignored is \
                 worse than one that is refused"
            ),
        )),
    }
}

/// Reads `[[tool.landav.suppress]]` — `LAN-66` criterion 2.
///
/// ```toml
/// [[tool.landav.suppress]]
/// path = "tests/**"
/// rules = ["LAV003"]
/// reason = "fixtures build strings the slow way on purpose"
/// ```
///
/// An array of tables rather than a `{ glob = [codes] }` map, because the
/// entry has to have room for a sentence — and, later, for the approver and
/// the expiry date `E-001` adds. A map keyed by glob has nowhere to put them
/// and would need a migration the day governance ships.
///
/// # Every malformed entry is refused by name
///
/// A waiver the tool cannot read is the one kind of configuration error that
/// silently *widens* what is suppressed if it is guessed at, and silently
/// narrows it if it is skipped. Both are worse than refusing to run.
fn waivers_of(path: &Path, section: &Table) -> Result<Vec<PathWaiver>, ToolError> {
    let Some(value) = section.get(SUPPRESS) else {
        return Ok(Vec::new());
    };
    let Some(entries) = value.as_array() else {
        return Err(ToolError::at_path(
            path,
            format!(
                "[{SECTION}] `{SUPPRESS}` must be a list of waivers written as \
                 [[{SECTION}.{SUPPRESS}]] tables, found {}",
                type_name(value)
            ),
        ));
    };

    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| waiver_of(path, index, entry))
        .collect()
}

/// Reads one `[[tool.landav.suppress]]` entry.
fn waiver_of(path: &Path, index: usize, entry: &Value) -> Result<PathWaiver, ToolError> {
    // 1-based, because the author counts their entries from one.
    let ordinal = index + 1;
    let Some(table) = entry.as_table() else {
        return Err(waiver_error(
            path,
            ordinal,
            format!(
                "must be a table with `path`, `rules` and `reason`, found {}",
                type_name(entry)
            ),
        ));
    };

    if let Some(key) = table
        .keys()
        .find(|key| !WAIVER_KEYS.contains(&key.as_str()))
    {
        return Err(waiver_error(
            path,
            ordinal,
            format!("sets `{key}`, which landav does not understand"),
        ));
    }

    let pattern = required_string(path, ordinal, table, "path")?;
    let reason = required_string(path, ordinal, table, "reason")?;
    let rules = required_rules(path, ordinal, table)?;

    Ok(PathWaiver::new(pattern, rules, reason))
}

/// A non-empty string value, or a diagnostic naming the entry and the key.
fn required_string(
    path: &Path,
    ordinal: usize,
    table: &Table,
    key: &str,
) -> Result<String, ToolError> {
    // `reason` is required, unlike an inline one. A per-path waiver covers
    // files that do not exist yet and is approved once by somebody who will
    // not be in the room when it is next read; a sentence is the only thing a
    // later auditor — or `E-001` — has to work with.
    let Some(value) = table.get(key) else {
        return Err(waiver_error(path, ordinal, format!("has no `{key}`")));
    };
    match value.as_str() {
        Some(text) if !text.trim().is_empty() => Ok(text.to_owned()),
        Some(_) => Err(waiver_error(path, ordinal, format!("has an empty `{key}`"))),
        None => Err(waiver_error(
            path,
            ordinal,
            format!("`{key}` must be a string, found {}", type_name(value)),
        )),
    }
}

/// A non-empty list of rule codes, or a diagnostic naming the entry.
///
/// The codes are **not** checked against the registry here. A waiver naming a
/// retired or misspelled code is a fact worth reporting rather than a reason
/// to refuse the whole run — the run would then fail on a stale waiver while
/// saying nothing about the code it was actually protecting. The report says
/// so instead, per waiver, every run.
fn required_rules(path: &Path, ordinal: usize, table: &Table) -> Result<Vec<String>, ToolError> {
    let Some(value) = table.get("rules") else {
        return Err(waiver_error(
            path,
            ordinal,
            "has no `rules`; a waiver has to name what it waives, because there is no \
             spelling that means all of them"
                .to_owned(),
        ));
    };
    let Some(items) = value.as_array() else {
        return Err(waiver_error(
            path,
            ordinal,
            format!(
                "`rules` must be a list of rule codes, found {}",
                type_name(value)
            ),
        ));
    };
    if items.is_empty() {
        return Err(waiver_error(
            path,
            ordinal,
            "has an empty `rules`, so it waives nothing and hides that it does".to_owned(),
        ));
    }

    items
        .iter()
        .map(|item| match item.as_str() {
            Some(code) if !code.trim().is_empty() => Ok(code.trim().to_owned()),
            _ => Err(waiver_error(
                path,
                ordinal,
                format!(
                    "`rules` lists {} where a rule code such as \"LAV003\" belongs",
                    type_name(item)
                ),
            )),
        })
        .collect()
}

/// A diagnostic naming the configuration file and which waiver is at fault.
fn waiver_error(path: &Path, ordinal: usize, detail: String) -> ToolError {
    ToolError::at_path(
        path,
        format!("[[{SECTION}.{SUPPRESS}]] entry {ordinal} {detail}"),
    )
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
/// Ascends to the filesystem root. A genuine absence returns `Ok(None)` — no
/// configuration is the supported default, not a degraded mode.
///
/// # Why this asks `symlink_metadata` rather than `is_file`
///
/// `Path::is_file` folds *every* `stat` failure into `false`. A
/// `pyproject.toml` that is a dangling symlink, a symlink loop, a directory,
/// or a file in an unreadable directory would therefore not exist as far as
/// discovery is concerned, and the ascent would carry on and bind a *farther*
/// configuration — or none — with nothing in the output saying so. That is the
/// precise failure this module says it exists to prevent.
///
/// The question discovery has to answer is "is there a name here", which
/// `symlink_metadata` answers without resolving it. Whether the thing behind
/// the name is usable is [`load_pyproject`]'s problem, and it is a hard
/// failure there.
fn discover(target: &Path) -> Result<Option<PathBuf>, ToolError> {
    let mut dir = discovery_root(target);
    loop {
        let candidate = dir.join(PYPROJECT);
        match std::fs::symlink_metadata(&candidate) {
            // The name exists. Whether it is usable is decided by the loader,
            // so that an unusable one stops the run instead of being skipped.
            Ok(_) => return Ok(Some(candidate)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(ToolError::at_path(
                    &candidate,
                    format!(
                        "cannot be inspected during configuration discovery: {err}; \
                         landav will not treat an unreadable path as an absent one"
                    ),
                ));
            }
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return Ok(None),
        }
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
    use super::{reject_unknown_keys, section_of, waivers_of};
    use std::path::Path;
    use toml::Table;

    fn parse(text: &str) -> Table {
        text.parse::<Table>().unwrap_or_default()
    }

    /// `[[tool.landav.suppress]]` inside a `pyproject.toml` is nested one
    /// level down; the tests below write the section body directly.
    fn waivers(text: &str) -> Result<Vec<landav_python::PathWaiver>, String> {
        waivers_of(Path::new("pyproject.toml"), &parse(text)).map_err(|error| error.to_string())
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

    /// `suppress` is the one key that is understood, and understanding it must
    /// not open the door to any other.
    #[test]
    fn suppress_is_accepted_and_nothing_else_is() {
        let good = parse("[[suppress]]\npath = \"a/**\"\nrules = [\"LAV003\"]\nreason = \"r\"\n");
        assert!(reject_unknown_keys(Path::new("pyproject.toml"), &good).is_ok());

        let bad = parse("suppressions = []\n");
        let err = reject_unknown_keys(Path::new("pyproject.toml"), &bad)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(err.contains("suppressions"), "{err}");
    }

    #[test]
    fn no_suppress_key_means_no_waivers() {
        assert_eq!(waivers("").map(|w| w.len()), Ok(0));
    }

    #[test]
    fn a_well_formed_waiver_is_read_whole() {
        let parsed = waivers(
            "[[suppress]]\npath = \"tests/**\"\nrules = [\"LAV003\", \"LAV002\"]\n\
             reason = \"fixtures are deliberately slow\"\n",
        );
        let one = parsed.as_ref().ok().and_then(|list| list.first());
        assert_eq!(one.map(super::PathWaiver::pattern), Some("tests/**"));
        assert_eq!(
            one.map(super::PathWaiver::rules),
            Some(["LAV003".to_owned(), "LAV002".to_owned()].as_slice())
        );
        assert_eq!(
            one.map(super::PathWaiver::reason),
            Some("fixtures are deliberately slow")
        );
    }

    /// Waivers keep the order they were written in, so a configuration diff
    /// and a report diff line up.
    #[test]
    fn waivers_keep_the_order_they_were_written_in() {
        let parsed = waivers(
            "[[suppress]]\npath = \"b/**\"\nrules = [\"LAV003\"]\nreason = \"r\"\n\
             [[suppress]]\npath = \"a/**\"\nrules = [\"LAV003\"]\nreason = \"r\"\n",
        );
        let patterns: Vec<String> = parsed
            .unwrap_or_default()
            .iter()
            .map(|waiver| waiver.pattern().to_owned())
            .collect();
        assert_eq!(patterns, ["b/**", "a/**"]);
    }

    /// Each way of writing an unusable waiver is refused, and the diagnostic
    /// names the entry and the key so the author can find it.
    #[test]
    fn every_malformed_waiver_is_refused_and_named() {
        let cases = [
            ("suppress = \"tests/**\"\n", "suppress"),
            ("suppress = [\"tests/**\"]\n", "entry 1"),
            (
                "[[suppress]]\nrules = [\"LAV003\"]\nreason = \"r\"\n",
                "path",
            ),
            ("[[suppress]]\npath = \"a\"\nreason = \"r\"\n", "rules"),
            (
                "[[suppress]]\npath = \"a\"\nrules = [\"LAV003\"]\n",
                "reason",
            ),
            (
                "[[suppress]]\npath = \"a\"\nrules = []\nreason = \"r\"\n",
                "rules",
            ),
            (
                "[[suppress]]\npath = \"\"\nrules = [\"LAV003\"]\nreason = \"r\"\n",
                "path",
            ),
            (
                "[[suppress]]\npath = \"a\"\nrules = [\"LAV003\"]\nreason = \"   \"\n",
                "reason",
            ),
            (
                "[[suppress]]\npath = 7\nrules = [\"LAV003\"]\nreason = \"r\"\n",
                "path",
            ),
            (
                "[[suppress]]\npath = \"a\"\nrules = \"LAV003\"\nreason = \"r\"\n",
                "rules",
            ),
            (
                "[[suppress]]\npath = \"a\"\nrules = [7]\nreason = \"r\"\n",
                "rules",
            ),
            (
                "[[suppress]]\npath = \"a\"\nrules = [\"LAV003\"]\nreason = \"r\"\nexpires = \"x\"\n",
                "expires",
            ),
        ];

        for (config, named) in cases {
            let err = waivers(config).err().unwrap_or_default();
            assert!(
                err.contains(named),
                "`{config}` must be refused with a diagnostic naming `{named}`, got `{err}`"
            );
        }
    }

    /// A waiver naming a code that does not exist is a fact to report, not a
    /// reason to refuse the run: refusing would fail the build over a stale
    /// waiver while saying nothing about the code it was protecting.
    #[test]
    fn a_waiver_may_name_a_code_the_registry_does_not_have() {
        let parsed = waivers(
            "[[suppress]]\npath = \"a\"\nrules = [\"LAV010\", \"LAV999\"]\nreason = \"r\"\n",
        );
        assert_eq!(
            parsed.map(|list| list.len()),
            Ok(1),
            "an unknown code must not stop the run"
        );
    }
}
