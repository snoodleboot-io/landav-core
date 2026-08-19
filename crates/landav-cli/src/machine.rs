//! The machine-readable shape of a run.
//!
//! # Why this exists beside the text output
//!
//! Both of landav's stated consumers are programs: a CI gate, and an agent
//! deciding what to change. The text output is written for a person at a
//! terminal, and everything a program needs from it - which function, what
//! bound, which construct was refused and where - has to be recovered by
//! parsing English.
//!
//! It does not need recovering. The run already holds all of it, structured;
//! it was being flattened on the way out.
//!
//! # This is a contract
//!
//! Anything emitted here is something a consumer will depend on. So the shape
//! carries [`SCHEMA_VERSION`], and the field names are chosen to be stable
//! rather than convenient. A consumer that breaks silently across an upgrade
//! is worse than one that never worked.
//!
//! # Constructs are named, never coded
//!
//! `"construct": "call"` rather than an opaque identifier. The agent's
//! transcript is read by a human, and a name survives that reading where a
//! code does not.

use serde::Serialize;

/// The version of this schema.
///
/// Bumped when a field changes meaning or disappears. Adding a field is not a
/// bump: a consumer that ignores unknown fields keeps working, and one that
/// does not was already fragile.
pub const SCHEMA_VERSION: u32 = 1;

/// A whole run.
#[derive(Debug, Serialize)]
pub struct Run {
    /// See [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The same verdict the exit code carries, as a stable name.
    ///
    /// Duplicated deliberately. A CI gate should be able to branch on the exit
    /// code without parsing anything at all, and an agent reading the JSON
    /// should not have to know the code table.
    pub outcome: &'static str,
    /// What was analysed and what came of it.
    pub summary: Summary,
    /// One entry per function the run met, in the order it met them.
    pub functions: Vec<Function>,
    /// Rule findings, independent of whether their function lowered.
    pub findings: Vec<Finding>,
    /// Paths the tool could not look at, and why.
    ///
    /// Present even when empty. A consumer that only ever sees successes will
    /// read silence as cleanliness, which is the failure `LAN-68` exists to
    /// prevent, and it applies to machine output at least as strongly.
    pub problems: Vec<Problem>,
}

/// The counts a gate is likely to threshold on.
#[derive(Debug, Serialize)]
pub struct Summary {
    pub files_analysed: usize,
    pub statements: usize,
    /// Functions met, whether or not they lowered.
    pub functions: usize,
    /// Functions that became an integer transition system.
    pub lowered: usize,
    /// `lowered / functions`, as a percentage, or `null` when there were no
    /// functions - which is not the same as zero percent and must not be
    /// reported as it.
    pub coverage_percent: Option<u32>,
    /// Total refused constructs. Occurrences, not functions: one function
    /// refusing for several reasons contributes several.
    pub refusals: usize,
    pub findings: usize,
    pub suppressed: usize,
    pub stale_waivers: usize,
}

/// One function, and everything concluded about it.
#[derive(Debug, Serialize)]
pub struct Function {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    /// Whether it became an integer transition system. A function that did not
    /// lower has no bound, and the reasons are in `refused`.
    pub lowered: bool,
    /// The derived cost, or `null`.
    ///
    /// `null` is not zero. A consumer that treats a missing bound as a cheap
    /// function has inverted the whole point of the tool.
    pub bound: Option<String>,
    /// `"exact"`, `"upper"`, `"partial"`, or `null` when there is no bound.
    ///
    /// `"partial"` means the bound is real but mentions regions that were not
    /// derived - see `holes`. It is not comparable against a budget until
    /// those are filled.
    pub bound_kind: Option<&'static str>,
    /// Whether everything outside the holes was derived exactly.
    ///
    /// Distinguishes "exact except for that `while`" from "approximate, and
    /// also there is a `while`".
    pub exact_outside_holes: bool,
    /// Regions inside this function that were not derived.
    pub holes: Vec<Hole>,
    /// Constructs that stopped this function lowering.
    pub refused: Vec<Refusal>,
}

/// A region whose cost is unknown, standing in the bound as a variable.
#[derive(Debug, Serialize)]
pub struct Hole {
    /// The variable it appears as in `bound`, so the two can be connected.
    pub variable: String,
    pub construct: String,
    pub origin: String,
}

/// A construct outside the analysable fragment.
#[derive(Debug, Serialize)]
pub struct Refusal {
    /// Named by behaviour - `call`, `non-integer-value` - never by a code.
    pub construct: String,
    /// What that construct means, so a consumer need not carry a table.
    pub describes: String,
    pub origin: String,
    /// Frontend specifics, where it had any.
    pub detail: Option<String>,
}

/// A rule finding.
#[derive(Debug, Serialize)]
pub struct Finding {
    pub rule: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub explanation: String,
}

/// A path the tool could not analyse.
#[derive(Debug, Serialize)]
pub struct Problem {
    pub path: Option<String>,
    pub detail: String,
}

/// Accumulates a run's structured shape as the walk proceeds.
///
/// Populated from the same passes that produce the text, so the two cannot
/// come to different conclusions about what was found.
#[derive(Debug, Default)]
pub struct Collector {
    functions: Vec<Function>,
    findings: Vec<Finding>,
    problems: Vec<Problem>,
}

impl Collector {
    /// Record every finding in a module.
    pub fn absorb_findings(&mut self, module: &landav_python::ModuleAnalysis) {
        for finding in module.findings() {
            let at = finding.location();
            self.findings.push(Finding {
                rule: finding.rule().to_string(),
                file: at.file().display().to_string(),
                line: at.line(),
                column: at.column(),
                explanation: finding.explanation().to_owned(),
            });
        }
    }

    /// Record one function and everything concluded about it.
    pub fn absorb_function(
        &mut self,
        function: &landav_python::LoweredFunction,
        lowered: Result<(), &landav_its::LoweringError>,
    ) {
        let at = function.location();
        let refused = lowered.err().map_or_else(Vec::new, |error| {
            error.refusals().map_or_else(Vec::new, |refusals| {
                refusals
                    .as_slice()
                    .iter()
                    .map(|unsupported| Refusal {
                        construct: unsupported.construct().tag().to_owned(),
                        describes: unsupported.construct().describe().to_owned(),
                        origin: unsupported.origin().as_str().to_owned(),
                        detail: unsupported.detail().map(ToString::to_string),
                    })
                    .collect()
            })
        });

        // A function that did not lower never reached the engine, so it has no
        // bound - and reporting one would be inventing a conclusion the run did
        // not reach.
        let derived = lowered
            .is_ok()
            .then(|| landav_engine::cost(function.program()));
        let (bound, kind, exact_outside, holes) = derived.as_ref().map_or_else(
            || (None, None, false, Vec::new()),
            |cost| {
                let kind = match cost {
                    landav_engine::TripCount::Exact(_) => Some("exact"),
                    landav_engine::TripCount::AtMost(_) => Some("upper"),
                    landav_engine::TripCount::Partial { .. } => Some("partial"),
                    landav_engine::TripCount::Unknown => None,
                };
                let holes = cost
                    .holes()
                    .iter()
                    .map(|hole| Hole {
                        variable: hole.var().symbol().to_string(),
                        construct: hole.construct().to_owned(),
                        origin: hole.origin().as_str().to_owned(),
                    })
                    .collect();
                (
                    cost.bound().map(ToString::to_string),
                    kind,
                    cost.exact_outside_holes(),
                    holes,
                )
            },
        );

        self.functions.push(Function {
            name: function.name().to_owned(),
            file: at.file().display().to_string(),
            line: at.line(),
            column: at.column(),
            lowered: lowered.is_ok(),
            bound,
            bound_kind: kind,
            exact_outside_holes: exact_outside,
            holes,
            refused,
        });
    }

    /// Record a path the tool could not analyse.
    pub fn problem(&mut self, path: Option<String>, detail: String) {
        self.problems.push(Problem { path, detail });
    }

    /// Seal the run, carrying the verdict the exit code will report.
    #[must_use]
    pub fn finish(
        mut self,
        outcome: crate::outcome::Outcome,
        summary: Summary,
        tool_errors: &[crate::diagnostic::ToolError],
    ) -> Run {
        for error in tool_errors {
            self.problems.push(Problem {
                path: None,
                detail: error.to_string(),
            });
        }
        Run {
            schema_version: SCHEMA_VERSION,
            outcome: outcome.tag(),
            summary,
            functions: self.functions,
            findings: self.findings,
            problems: self.problems,
        }
    }
}
