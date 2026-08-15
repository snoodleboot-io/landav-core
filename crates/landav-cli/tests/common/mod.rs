//! Shared harness for the `landav` CLI acceptance tests (LAN-61).
//!
//! Every test in this suite drives the **built binary** through
//! [`std::process::Command`]. That is deliberate: the acceptance criteria are
//! about the *process exit code*, and a unit test that calls a function
//! returning [`landav_bound::ExitCode`] proves nothing about what the process
//! actually hands back to CI. The contract is only observed at the process
//! boundary, so that is where it is tested.
#![allow(
    dead_code,
    reason = "each integration test binary uses a different subset of this harness"
)]

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// The binary under test, as built by cargo for this test run.
pub const BIN: &str = env!("CARGO_BIN_EXE_landav");

// The frozen exit contract. Mirrors `landav_bound::ExitCode`, restated as bare
// integers on purpose: these tests must fail if the enum's discriminants are
// ever renumbered, and importing the enum would let a renumbering pass.
//
/// Analysis ran and there is nothing to report.
pub const EXIT_CLEAN: i32 = 0;
/// Analysis ran and found something the caller should act on.
pub const EXIT_FINDINGS: i32 = 1;
/// The tool could not complete. CI branches on this to tell "we found a
/// problem" from "we could not look".
pub const EXIT_TOOL_ERROR: i32 = 2;

/// Every exit code the CLI is permitted to produce.
pub const SANCTIONED_EXIT_CODES: [i32; 3] = [EXIT_CLEAN, EXIT_FINDINGS, EXIT_TOOL_ERROR];

/// Stand-in for "the process did not exit normally" (killed by a signal).
/// Chosen outside the sanctioned set so it can never satisfy an assertion.
pub const KILLED_BY_SIGNAL: i32 = -1;

/// The exit code the Rust runtime uses for an unwinding panic.
pub const RUST_PANIC_EXIT: i32 = 101;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A function whose cost is linear in its input and fully accounted for.
/// Nothing here is unbounded, unreachable, or pattern-flagged.
pub const CLEAN_PY: &str = r"
def total(items):
    acc = 0
    for item in items:
        acc += item
    return acc
";

/// Two quadratic shapes from the `F-005` rule set, verified against the
/// frontend: `LAV002` at the membership test, `LAV003` at the accumulation.
///
/// **Two rules on purpose.** An earlier version of this fixture leaned on a
/// single shape: `x in ys` against an unannotated parameter, plus a list
/// rebuilt by concatenation. Both went silent when the line-oriented scanner
/// was replaced by the real parser — `LAV002` correctly wants evidence the
/// container is a list, and no list-concatenation rule survived at all
/// (`LAV003` is string concatenation only, and `LAV010` was withdrawn).
/// That left the whole "findings exit 1" criterion resting on rules that had
/// quietly stopped firing. `known = list(allowed)` supplies the evidence
/// `LAV002` asks for, and the string accumulator is `LAV003`'s canonical
/// shape; one rule can be retired without silently disarming the criterion.
pub const FINDINGS_PY: &str = r#"
def summarise(rows, allowed):
    known = list(allowed)
    out = ""
    for row in rows:
        if row in known:
            out += str(row)
    return out
"#;

/// A Python 2 module: the bytes were read, and they are not Python 3.
///
/// This reaches [`Outcome::Inconclusive`] through
/// `landav_python::PythonError::Parse` — the frontend could not read the file,
/// so nothing was derived from it and it is not covered by the run's verdict.
///
/// **It is not the case criterion 3 was written about**, and the tests that
/// use it say so. See the module documentation of `outcome_space.rs` for the
/// coverage that is missing and why it cannot be written at M0.
///
/// Chosen over a merely malformed file because it is stable: Python 2 print
/// syntax will never start parsing as Python 3, so this fixture cannot quietly
/// change what it tests.
pub const UNREADABLE_AS_PYTHON_PY: &str = r#"
print "this module was written for Python 2"
"#;

/// Valid Python 3.12 that the pinned `rustpython-parser` predates: PEP 701
/// allows the same quote character inside an f-string replacement field.
///
/// A different class from [`UNREADABLE_AS_PYTHON_PY`] and worth its own case —
/// here the *file* is fine and the *frontend* is behind, which is the failure
/// mode most likely to appear on a modern codebase.
///
/// This fixture is expected to stop being unparsable when the parser is
/// upgraded. When that happens this test fails, which is correct: somebody has
/// to decide what replaces it rather than discovering later that the case
/// silently became a clean run.
pub const PEP701_FSTRING_PY: &str = r#"
def show(row):
    print(f"{row["name"]}")
"#;

/// A `pyproject.toml` that is valid TOML and says nothing about landav.
pub const PYPROJECT_NO_LANDAV: &str = r#"
[project]
name = "fixture"
version = "0.1.0"

[tool.black]
line-length = 88
"#;

/// A `pyproject.toml` carrying an empty but well-formed `[tool.landav]`
/// section. Behaviour must be identical to having no section at all.
pub const PYPROJECT_EMPTY_LANDAV: &str = r#"
[project]
name = "fixture"
version = "0.1.0"

[tool.landav]
"#;

/// Valid TOML, but `tool.landav` is a scalar where a configuration table
/// belongs. Any reader that actually *consults* the section fails here; a
/// reader that pattern-matches for a table and shrugs when it does not find
/// one will silently exit clean, which is the bug this pins.
pub const PYPROJECT_LANDAV_NOT_A_TABLE: &str = r#"
[project]
name = "fixture"

[tool]
landav = 42
"#;

/// Not valid TOML at all: unterminated string, unclosed table header.
pub const PYPROJECT_MALFORMED: &str = r#"
[project
name = "fixture
"#;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// One invocation of the binary, reduced to what the contract is about.
pub struct Run {
    /// The process exit code, or [`KILLED_BY_SIGNAL`].
    pub code: i32,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
    /// The argument vector, for assertion messages.
    pub args: Vec<String>,
}

impl Run {
    /// Everything the process wrote, for substring checks.
    pub fn output(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }

    /// A rendering suitable for an assertion message.
    pub fn describe(&self) -> String {
        format!(
            "`landav {}` exited {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.args.join(" "),
            self.code,
            self.stdout,
            self.stderr
        )
    }

    /// Whether the combined output mentions `needle`, case-insensitively.
    pub fn mentions(&self, needle: &str) -> bool {
        self.output()
            .to_lowercase()
            .contains(&needle.to_lowercase())
    }

    /// The tool must fail cleanly, never by unwinding out of `main` and never
    /// by dying on a signal. A panic carries no blame, which rule 2 of
    /// `CONTRIBUTING.md` exists to prevent.
    pub fn assert_did_not_crash(&self) {
        assert_ne!(
            self.code,
            KILLED_BY_SIGNAL,
            "the process was killed by a signal instead of exiting.\n{}",
            self.describe()
        );
        assert_ne!(
            self.code,
            RUST_PANIC_EXIT,
            "the process exited 101, the panic code.\n{}",
            self.describe()
        );
        assert!(
            !self.output().contains("panicked at"),
            "the process panicked.\n{}",
            self.describe()
        );
        assert!(
            !self
                .output()
                .contains("internal error: entered unreachable code"),
            "the process hit an `unreachable!`.\n{}",
            self.describe()
        );
        assert!(
            !self.output().contains("not yet implemented"),
            "the process hit a `todo!`.\n{}",
            self.describe()
        );
    }

    /// The exit code must be one of the three the contract allows. A fourth
    /// code is a broken contract even if it is "obviously" an error, because
    /// CI branches on the enumerated set.
    pub fn assert_code_is_sanctioned(&self) {
        assert!(
            SANCTIONED_EXIT_CODES.contains(&self.code),
            "exit code {} is outside the frozen contract {:?}.\n{}",
            self.code,
            SANCTIONED_EXIT_CODES,
            self.describe()
        );
    }

    /// A tool error must say what went wrong, on stderr, naming the thing that
    /// caused it. An exit code with no explanation is not actionable in CI.
    pub fn assert_explains(&self, subject: &str) {
        assert!(
            !self.stderr.trim().is_empty(),
            "a tool error must write a diagnostic to stderr.\n{}",
            self.describe()
        );
        assert!(
            self.mentions(subject),
            "the diagnostic does not name `{}`, so the operator cannot tell \
             what went wrong.\n{}",
            subject,
            self.describe()
        );
    }
}

/// A throwaway project tree. Dropped with the temporary directory.
pub struct Project {
    dir: TempDir,
}

impl Project {
    /// An empty project directory.
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            dir: TempDir::new()?,
        })
    }

    /// The project root.
    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Write `contents` to `rel`, creating parent directories as needed.
    pub fn write(&self, rel: &str, contents: &str) -> io::Result<PathBuf> {
        let path = self.dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, contents)?;
        Ok(path)
    }

    /// Create directory `rel` inside the project.
    pub fn mkdir(&self, rel: &str) -> io::Result<PathBuf> {
        let path = self.dir.path().join(rel);
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    /// Run the binary with `args`, with the project root as the working
    /// directory so that configuration discovery has somewhere to discover
    /// from regardless of whether it walks up from the target path or the cwd.
    pub fn run(&self, args: &[&str]) -> io::Result<Run> {
        let output = Command::new(BIN)
            .args(args)
            .current_dir(self.dir.path())
            .output()?;

        Ok(Run {
            code: output.status.code().unwrap_or(KILLED_BY_SIGNAL),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        })
    }

    /// `landav check <target>`, plus any extra arguments.
    pub fn check(&self, target: &Path, extra: &[&str]) -> io::Result<Run> {
        let target = target.to_string_lossy().into_owned();
        let mut args: Vec<&str> = vec!["check", &target];
        args.extend_from_slice(extra);
        self.run(&args)
    }
}
