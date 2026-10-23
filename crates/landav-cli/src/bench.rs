//! `landav bench`: run the corpora and keep the baselines by commit.
//!
//! # Why
//!
//! `LAN-123`, the second slice of `F-040`'s coverage metric. `landav diff`
//! (`LAN-119`) compares two runs; until now producing and keeping them was a
//! human remembering four paths. A corpus is machine-specific - the standard
//! library lives wherever Python is installed, an application is a checkout -
//! so the corpora are declared once, in a user-level file, and a baseline is
//! named after the commit the binary was built from, not after what somebody
//! typed.
//!
//! # The three verbs
//!
//! * `run [CORPUS…] [--label LABEL]` - `landav check PATH --json` over each
//!   corpus with the flags the corpus declares, written to
//!   `<baselines>/<label>-<corpus>.json`. The label defaults to
//!   [`BUILT_FROM`], and a dirty tree's label says `-dirty`.
//! * `diff BEFORE AFTER [CORPUS…]` - [`crate::diff`] over the two labels'
//!   files, one corpus after another.
//! * `list` - the labels and corpora the baselines directory holds.
//!
//! # Why `run` spawns the binary
//!
//! `check` writes its report to the process's stdout as it goes; capturing it
//! in-process would mean threading a sink through the whole walk for one
//! caller. Running `current_exe()` with the same arguments a person would type
//! is the same analysis by construction, and the file it leaves behind is
//! byte-for-byte what `landav check --json` prints.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::{diagnostic::ToolError, outcome::Outcome};

/// The commit this binary was built from, from `build.rs`.
pub const BUILT_FROM: &str = env!("LANDAV_COMMIT");

/// The corpora file.
#[derive(Debug, Deserialize)]
struct Corpora {
    #[serde(default)]
    corpus: BTreeMap<String, Corpus>,
}

/// One corpus: where it is and how `check` must be told to walk it.
#[derive(Debug, Deserialize)]
struct Corpus {
    path: PathBuf,
    /// `--include-vendored`: a virtual environment is only a corpus on purpose.
    #[serde(default)]
    include_vendored: bool,
}

/// Where the user's things live, by the XDG rules with `HOME` as the fallback.
fn user_dir(xdg: &str, under_home: &str) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(xdg).filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(under_home))
}

/// `~/.config/landav/corpora.toml` unless told otherwise.
fn default_config() -> Result<PathBuf, ToolError> {
    user_dir("XDG_CONFIG_HOME", ".config")
        .map(|dir| dir.join("landav").join("corpora.toml"))
        .ok_or_else(|| ToolError::new("--config", "no HOME to find the corpora file under"))
}

/// `~/.cache/landav/baselines` unless told otherwise.
fn default_baselines() -> Result<PathBuf, ToolError> {
    user_dir("XDG_CACHE_HOME", ".cache")
        .map(|dir| dir.join("landav").join("baselines"))
        .ok_or_else(|| ToolError::new("--baselines", "no HOME to keep baselines under"))
}

/// What every verb needs: the corpora and where the baselines go.
pub struct Setup {
    pub config: Option<PathBuf>,
    pub baselines: Option<PathBuf>,
}

struct Resolved {
    corpora: Corpora,
    config: PathBuf,
    baselines: PathBuf,
}

fn resolve(setup: &Setup, need_corpora: bool) -> Result<Resolved, ToolError> {
    let config = match &setup.config {
        Some(path) => path.clone(),
        None => default_config()?,
    };
    let baselines = match &setup.baselines {
        Some(path) => path.clone(),
        None => default_baselines()?,
    };
    let corpora = if need_corpora {
        let text = fs::read_to_string(&config).map_err(|why| {
            ToolError::at_path(
                &config,
                format!(
                    "could not read the corpora file: {why}. It declares each corpus as \
                     `[corpus.NAME]` with `path = \"...\"` and, for a virtual environment, \
                     `include_vendored = true`"
                ),
            )
        })?;
        toml::from_str(&text)
            .map_err(|why| ToolError::at_path(&config, format!("not a corpora file: {why}")))?
    } else {
        Corpora {
            corpus: BTreeMap::new(),
        }
    };
    Ok(Resolved {
        corpora,
        config,
        baselines,
    })
}

/// The corpora named, or all of them; a name the file does not declare is an
/// error naming the file.
fn select<'a>(
    resolved: &'a Resolved,
    names: &[String],
) -> Result<Vec<(&'a str, &'a Corpus)>, ToolError> {
    if names.is_empty() {
        if resolved.corpora.corpus.is_empty() {
            return Err(ToolError::at_path(
                &resolved.config,
                "declares no corpus; add a `[corpus.NAME]` table",
            ));
        }
        return Ok(resolved
            .corpora
            .corpus
            .iter()
            .map(|(name, corpus)| (name.as_str(), corpus))
            .collect());
    }
    names
        .iter()
        .map(|name| {
            resolved
                .corpora
                .corpus
                .get_key_value(name)
                .map(|(name, corpus)| (name.as_str(), corpus))
                .ok_or_else(|| {
                    ToolError::at_path(
                        &resolved.config,
                        format!(
                            "declares no corpus named `{name}` (it has: {})",
                            resolved
                                .corpora
                                .corpus
                                .keys()
                                .map(|k| format!("`{k}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )
                })
        })
        .collect()
}

fn baseline_path(baselines: &Path, label: &str, corpus: &str) -> PathBuf {
    baselines.join(format!("{label}-{corpus}.json"))
}

/// `landav bench run`.
pub fn run(setup: &Setup, names: &[String], label: Option<&str>) -> Outcome {
    match run_inner(setup, names, label) {
        Ok(()) => Outcome::Clean,
        Err(error) => failed(&error),
    }
}

fn run_inner(setup: &Setup, names: &[String], label: Option<&str>) -> Result<(), ToolError> {
    let resolved = resolve(setup, true)?;
    let selected = select(&resolved, names)?;
    let label = label.unwrap_or(BUILT_FROM);
    if label.is_empty() || label.contains('/') || label.contains('-') && label.ends_with('-') {
        return Err(ToolError::new(
            "--label",
            "a label is a plain word, used as a file name",
        ));
    }
    fs::create_dir_all(&resolved.baselines).map_err(|why| {
        ToolError::at_path(
            &resolved.baselines,
            format!("could not create the baselines directory: {why}"),
        )
    })?;
    let exe = std::env::current_exe()
        .map_err(|why| ToolError::new("landav", format!("could not find its own binary: {why}")))?;
    for (name, corpus) in selected {
        let mut command = Command::new(&exe);
        command.arg("check").arg(&corpus.path).arg("--json");
        if corpus.include_vendored {
            command.arg("--include-vendored");
        }
        let output = command.output().map_err(|why| {
            ToolError::at_path(
                &corpus.path,
                format!("could not run `check` over `{name}`: {why}"),
            )
        })?;
        // 0 and 1 are both a completed run (clean, or with findings); 2 is
        // the tool failing, and a baseline of a failed run is no baseline.
        if output.status.code().is_none_or(|code| code > 1) {
            return Err(ToolError::at_path(
                &corpus.path,
                format!(
                    "`check` over `{name}` did not complete (exit {:?}): {}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        let target = baseline_path(&resolved.baselines, label, name);
        // Written whole or not at all: a reader must never see a half file
        // under the final name.
        let partial = target.with_extension("json.part");
        fs::write(&partial, &output.stdout)
            .and_then(|()| fs::rename(&partial, &target))
            .map_err(|why| {
                ToolError::at_path(&target, format!("could not write the baseline: {why}"))
            })?;
        let summary = crate::diff::summary_of(&target).unwrap_or_default();
        println!("{name}: {summary} -> {}", target.display());
    }
    Ok(())
}

/// `landav bench diff`.
pub fn diff(setup: &Setup, before: &str, after: &str, names: &[String], json: bool) -> Outcome {
    let resolved = match resolve(setup, true) {
        Ok(resolved) => resolved,
        Err(error) => return failed(&error),
    };
    let selected = match select(&resolved, names) {
        Ok(selected) => selected,
        Err(error) => return failed(&error),
    };
    let mut outcome = Outcome::Clean;
    for (name, _) in selected {
        let (from, to) = (
            baseline_path(&resolved.baselines, before, name),
            baseline_path(&resolved.baselines, after, name),
        );
        if !json {
            println!("===== {name}: {before} -> {after} =====");
        }
        if crate::diff::diff(&from, &to, json) == Outcome::Failed {
            outcome = Outcome::Failed;
        }
    }
    outcome
}

/// `landav bench list`.
pub fn list(setup: &Setup) -> Outcome {
    let resolved = match resolve(setup, false) {
        Ok(resolved) => resolved,
        Err(error) => return failed(&error),
    };
    let entries = match fs::read_dir(&resolved.baselines) {
        Ok(entries) => entries,
        Err(why) => {
            return failed(&ToolError::at_path(
                &resolved.baselines,
                format!("could not list the baselines: {why}"),
            ));
        }
    };
    // `<label>-<corpus>.json`, split at the *last* dash: a label may carry one
    // (`a875c2b-dirty`), a corpus name may not.
    let mut by_label: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(stem) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
            continue;
        };
        let Some((label, corpus)) = stem.rsplit_once('-') else {
            continue;
        };
        by_label
            .entry(label.to_owned())
            .or_default()
            .insert(corpus.to_owned());
    }
    if by_label.is_empty() {
        println!("no baselines in {}", resolved.baselines.display());
        return Outcome::Clean;
    }
    println!("baselines in {}", resolved.baselines.display());
    for (label, corpora) in by_label {
        println!(
            "  {label:<16} {}",
            corpora.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    Outcome::Clean
}

fn failed(error: &ToolError) -> Outcome {
    eprintln!("{error}");
    Outcome::Failed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binary_knows_what_it_was_built_from() {
        assert!(!BUILT_FROM.is_empty());
        assert!(!BUILT_FROM.contains('/'));
    }

    #[test]
    fn a_baseline_is_named_label_dash_corpus() {
        assert_eq!(
            baseline_path(Path::new("/b"), "a875c2b-dirty", "stdlib"),
            PathBuf::from("/b/a875c2b-dirty-stdlib.json")
        );
    }

    #[test]
    fn a_corpora_file_declares_paths_and_flags() {
        let Ok(corpora) = toml::from_str::<Corpora>(
            "[corpus.stdlib]\npath = \"/usr/lib/python3.12\"\n\n[corpus.app]\npath = \"/src/app\"\ninclude_vendored = true\n",
        ) else {
            unreachable!("the literal above is a valid corpora file");
        };
        assert_eq!(corpora.corpus.len(), 2);
        assert!(!corpora.corpus["stdlib"].include_vendored);
        assert!(corpora.corpus["app"].include_vendored);
    }
}
