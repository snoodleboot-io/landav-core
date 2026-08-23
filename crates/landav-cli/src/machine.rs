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

use landav_bound::ResourceKind;
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
    /// The resource this run was asked about, or absent when none was named.
    ///
    /// Carries whether this build derives a number for **that** resource and,
    /// when it does not, what *it* is waiting on. One sentence per resource,
    /// never one for the registry: `ops` is waiting on a measurement and
    /// `peak-mem` on an analysis nobody has started, and a consumer told only
    /// that both are unavailable cannot tell which will arrive first. See
    /// [`crate::resource`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<Resource>,
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
    ///
    /// The headline number, and the one `coverage_percent` is over. It means
    /// "the whole toolchain can handle this function" and must not be confused
    /// with `analysed`.
    pub lowered: usize,
    /// Functions the native engine derived a bound for, complete or partial.
    ///
    /// Always at least `lowered`, and usually larger: a function whose only
    /// obstacle is a call does not lower, and is still analysed apart from that
    /// call. Reported separately rather than folded into `lowered` because a
    /// partial bound carries an unfilled hole, an unfilled hole denotes `omega`,
    /// and counting those as covered would claim an improvement the run did not
    /// make.
    pub analysed: usize,
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

/// The resource a run was asked about, and its status in this build.
#[derive(Debug, Serialize)]
pub struct Resource {
    /// The `--resource` value, so a run can be filed against the invocation
    /// that produced it. Never the semiring alone: three registered resources
    /// share `additive`, so the algebra does not say what was counted.
    pub id: &'static str,
    /// The unit the reported number is in.
    pub unit: &'static str,
    /// The algebra the resource instantiates.
    pub semiring: &'static str,
    /// Whether this build produces a number for this resource.
    ///
    /// A boolean rather than an inference from a missing field: a consumer
    /// cannot otherwise tell "no number was derived" from "the number is zero",
    /// and those call for opposite reactions.
    pub derived: bool,
    /// What *this* resource is waiting on, and `null` exactly when `derived`.
    ///
    /// The two are the same fact in two shapes and are generated from one
    /// function, so a resource can never report both a number and something it
    /// is still waiting for.
    pub awaiting: Option<String>,
}

/// What was derived for the selected resource, for one function.
#[derive(Debug, Serialize)]
pub struct FunctionResource {
    /// The count, rendered as an expression, because it may be symbolic: a
    /// call inside `for i in range(n)` issues `n` queries.
    pub bound: Option<String>,
    /// `"exact"`, `"upper"`, `"partial"`, or `null`.
    ///
    /// `"partial"` means the count mentions a region that was not derived, and
    /// is therefore not comparable against a budget. It is what a `while` gets,
    /// and it is reported instead of `0` because the run has no evidence that
    /// the unread region issues nothing.
    pub bound_kind: Option<&'static str>,
    /// The same quantity as a number when it is a closed constant, so a gate
    /// can threshold without parsing algebra.
    ///
    /// `null` is not zero. It means the count is symbolic, or mentions an
    /// unanalysed region, and a gate that reads it as zero has inverted the
    /// tool - which is the same rule `bound` already carries, and it bites
    /// harder here because a resource count is exactly what gets thresholded.
    pub value: Option<u64>,
}

/// One function, and everything concluded about it.
#[derive(Debug, Serialize)]
pub struct Function {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    /// Whether it became an integer transition system.
    ///
    /// **Not** whether anything was derived for it: the native engine reads the
    /// structured source directly and reports a `"partial"` bound for a function
    /// the lowering refused, naming each region it could not derive. The two are
    /// different numbers and `summary` reports both. A function with
    /// `lowered: false` never carries an `"exact"` or `"upper"` bound; the
    /// reasons it did not lower are in `refused`.
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
    /// What was derived for the selected resource, when one was selected and
    /// this build derives it. Absent otherwise, and absence is not zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<FunctionResource>,
}

/// A region whose cost is unknown, standing in the bound as a variable.
///
/// Carries both halves of `CONTRIBUTING.md`'s third non-negotiable: the
/// unaccounted **term** (`variable`, `construct`, `origin`) and the
/// **assumption** that could not be discharged. LAN-14 is the second half; it
/// was built as [`landav_bound::Assumption`] and left unwired, so a report
/// named the `while` and never said that what was missing was a termination
/// argument rather than a cost rule.
#[derive(Debug, Serialize)]
pub struct Hole {
    /// The variable it appears as in `bound`, so the two can be connected.
    pub variable: String,
    pub construct: String,
    /// The obligation this region left undischarged, as a stable name:
    /// `termination-not-proved`, `callee-cost-unknown`, `recursion-not-ranked`,
    /// `resource-not-modelled`.
    ///
    /// Named, never coded, for the reason this module's documentation gives.
    /// Distinct from `construct` on purpose: `construct` says what the region
    /// *is*, and this says what could not be *established* about it. Two `while`
    /// loops and a call all read as "we could not derive it" without this field,
    /// and they need different responses - a ranking function, a cost contract.
    pub assumption: String,
    /// What the assumption is about: the callee whose cost is unknown, the
    /// variable with no size bound, the construct with no rule. `null` where the
    /// obligation has no subject beyond the region itself.
    pub assumption_subject: Option<String>,
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
    /// The resource this run was asked about, if any. Held so that the
    /// per-function block and the run-level block are decided from one value
    /// and cannot disagree about which question was asked.
    resource: Option<ResourceKind>,
    functions: Vec<Function>,
    findings: Vec<Finding>,
    problems: Vec<Problem>,
}

impl Collector {
    /// A collector for a run that named `resource`.
    #[must_use]
    pub fn for_resource(resource: Option<ResourceKind>) -> Self {
        Self {
            resource,
            ..Self::default()
        }
    }

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
        // Kept as the frontend's own records for as long as possible: the hole
        // ledger is joined against these to find the specifics a hole does not
        // carry, and re-deriving them from the rendered strings would be
        // parsing English back into a value.
        let records = lowered.err().and_then(landav_its::LoweringError::refusals);
        let records: &[landav_its::Unsupported] =
            records.map_or(&[], landav_its::Refusals::as_slice);
        let refused = records
            .iter()
            .map(|unsupported| Refusal {
                construct: unsupported.construct().tag().to_owned(),
                describes: unsupported.construct().describe().to_owned(),
                origin: unsupported.origin().as_str().to_owned(),
                detail: unsupported.detail().map(ToString::to_string),
            })
            .collect();

        // The engine is consulted for **every** function, including the ones
        // that did not lower: a call is the sole construct blocking most of the
        // corpus, and cost derived apart from a named call site is the first
        // thing a user of that corpus can act on. What a run is entitled to say
        // about a function the toolchain refused is decided once, in
        // `crate::derived`, so the text and this cannot drift apart.
        let derived = crate::derived::cost_of(function, lowered.is_ok(), records);
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
                    .map(|hole| {
                        // Both halves of the blame, decided in one place:
                        // `crate::assumption` keys on the hole rather than on a
                        // refusal record, because the most common hole on the
                        // corpus - a `while` - lowers and so has no refusal
                        // record to key on.
                        let assumption =
                            crate::assumption::undischarged(hole, records, function.name());
                        Hole {
                            variable: hole.var().symbol().to_string(),
                            construct: hole.construct().to_owned(),
                            assumption: crate::assumption::name(&assumption).to_owned(),
                            assumption_subject: crate::assumption::subject(&assumption),
                            origin: hole.origin().as_str().to_owned(),
                        }
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

        // The selected resource, projected out of the same derived cost the
        // bound above was rendered from - never a second walk, so the two
        // numbers printed beside each other cannot use different control-flow
        // rules. Absent when no resource was named, and when the named one
        // derives nothing in this build: a per-function number beside
        // `derived: false` would be the fabrication the exit contract exists to
        // prevent, and it would look entirely plausible.
        let resource = self
            .resource
            .filter(|kind| crate::resource::derives(*kind))
            .and(derived.as_ref())
            .map(|cost| {
                let projected = crate::resource_bound::ResourceBound::queries_of(cost);
                FunctionResource {
                    bound: projected.bound,
                    bound_kind: projected.kind,
                    value: projected.value,
                }
            });

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
            resource,
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
            resource: self.resource.map(|kind| {
                let descriptor = kind.descriptor();
                Resource {
                    id: descriptor.id().as_str(),
                    unit: descriptor.unit(),
                    semiring: descriptor.semiring().as_str(),
                    derived: crate::resource::derives(kind),
                    // `null` exactly when derived: the two are generated from
                    // one function, so a resource cannot report both a number
                    // and something it is still waiting for.
                    awaiting: crate::resource::awaiting(kind),
                }
            }),
            summary,
            functions: self.functions,
            findings: self.findings,
            problems: self.problems,
        }
    }
}
