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

use std::collections::BTreeSet;

use landav_bound::ResourceKind;
use landav_fdk::SignaturePack;
use landav_its::Construct;
use serde::Serialize;

/// The version of this schema.
///
/// Bumped when a field changes meaning or disappears. Adding a field is not a
/// bump: a consumer that ignores unknown fields keeps working, and one that
/// does not was already fragile.
pub const SCHEMA_VERSION: u32 = 2;

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
    ///
    /// **Over the files that could be read.** A file the frontend cannot parse
    /// contributes no functions to either count, so where
    /// [`Self::unreadable_files`] is non-zero this *overstates* coverage of the
    /// target as a whole. It is not withheld, because one unreadable file in a
    /// large tree would then erase the figure for every file that was read -
    /// the Python 3.12 standard library has three - and the run's `outcome` is
    /// already `inconclusive` whenever it happens, which is the signal a gate
    /// acts on. A gate thresholding on this number should read it beside
    /// `unreadable_files`. `LAN-82`.
    pub coverage_percent: Option<u32>,
    /// Which annotations this run read a collection's length from:
    /// `"protocol"` or `"concrete"`.
    ///
    /// Recorded because the field exists to be *compared*. Two runs of the same
    /// tree differ in their bounds exactly where the weaker premise was used,
    /// and a reader diffing them has to be able to tell which is which without
    /// reconstructing the command line. `LAN-106`.
    pub trust: String,
    /// Rows a supplied pack replaced, in the order the overlays happened.
    ///
    /// Empty unless `--signatures` was given and a pack disagreed with a row
    /// already in force. Reported rather than merely performed for the reason
    /// a suppression that suppressed nothing is reported: a claim nobody can
    /// see is a claim nobody can review — and an override that loosens a row
    /// can make a bound the program exceeds. Added by `LAN-13`; a new field,
    /// which on its own would not need a bump. The bump is `Premise::subject`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub signature_overrides: Vec<SignatureOverride>,
    /// Files the frontend could not read as Python.
    ///
    /// The functions in them are in no count in this summary, because they
    /// cannot be counted without being parsed. Each one is also named, with
    /// its position and the parser's reason, in the run's `problems`. Always
    /// present, so a gate watching it can see it change. `LAN-82`.
    pub unreadable_files: usize,
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
pub struct Premise {
    /// What the premise is about: a bound variable, or a callee.
    ///
    /// Renamed from `variable` by `LAN-13`, which is why [`SCHEMA_VERSION`] is
    /// 2. A premise resting on a supplied signature is about a *call*, and
    /// calling that a variable would have been the field meaning something
    /// different depending on the value of another field.
    pub subject: String,
    /// `"concrete-type"`, `"protocol"`, or `"supplied-signature"`.
    pub trust: &'static str,
    /// Why the number is believed, in one sentence.
    ///
    /// Owned rather than `&'static str` because a supplied-signature premise
    /// has to name the file the row came from: told only that a bound rests on
    /// a pack, the first thing a reader needs is which pack.
    pub because: String,
}

/// What the bound this run reports for `program` is believed on.
///
/// # Read, not mentioned
///
/// The filter is whether the program **read** the length, not whether the
/// reported bound still mentions it. Those differ, and the difference hides the
/// case that matters most:
///
/// ```python
/// def max_len(size: int, element: Collection[object]) -> bool:
///     return len(element) <= size
/// ```
///
/// Trusting the protocol makes `len(element)` a variable this fragment can
/// read, so the function is `Theta(1)` - complete, exact, and mentioning no
/// length at all. Decline the trust and `len()` is an unknown call, and the
/// result is partial. So that `1` rests entirely on the premise while naming
/// nothing, and a rule keyed on the rendered bound would publish no premise for
/// precisely the bound whose *completeness* the premise bought. Measured on the
/// typed corpus, two functions turn on this and both claim constant cost.
///
/// A length the function never reads is still not listed: nothing about the
/// result depends on it, and listing it would make the field noise.
fn premises_of(program: &landav_its::SourceProgram, pack: Option<&SignaturePack>) -> Vec<Premise> {
    let mut premises: Vec<Premise> = program
        .params()
        .iter()
        .filter(|name| program.reads_length(name))
        .map(|name| {
            if program.rests_on_a_protocol(name) {
                Premise {
                    subject: name.symbol().as_str().to_owned(),
                    trust: "protocol",
                    because: "the parameter is annotated with an abstract collection type, so its \
                              length is a user `__len__` that is trusted to equal the number of \
                              iterations; a class implementing the two inconsistently makes this \
                              bound exceedable"
                        .to_owned(),
                }
            } else {
                Premise {
                    subject: name.symbol().as_str().to_owned(),
                    trust: "concrete-type",
                    because: "the parameter is annotated with a concrete builtin collection, and \
                              iterating one yields exactly `len` items; the premise is that the \
                              caller passes what the annotation says"
                        .to_owned(),
                }
            }
        })
        .collect();
    premises.extend(supplied_premises(program, pack));
    premises
}

/// One premise per callee this program resolved against a row nobody shipped.
///
/// # Why this is read back off the program rather than recorded during lowering
///
/// A resolved call is still a node in the arena — it stops being a *hole*, not
/// a node — and it carries the callee in `detail`. So the program already says
/// which callees a row accounted for, and the pack already says where each row
/// came from. Recording a third copy during lowering would be a fact that could
/// disagree with the two that produced it.
///
/// Only non-builtin rows earn a premise. A bound resting on the shipped pack
/// rests on a table this build was released with, which is the tool's own claim
/// and not an external one; a bound resting on a supplied row rests on
/// something the operator asserted, and that is exactly what a premise is for.
fn supplied_premises(
    program: &landav_its::SourceProgram,
    pack: Option<&SignaturePack>,
) -> Vec<Premise> {
    let Some(pack) = pack else {
        return Vec::new();
    };
    // Owned: `unsupported_nodes` yields records built on the fly, so a `&str`
    // into one would not outlive the loop.
    let mut callees: BTreeSet<String> = BTreeSet::new();
    for node in program.unsupported_nodes() {
        if node.construct() != Construct::Call || node.declared().is_none() {
            continue;
        }
        if let Some(detail) = node.detail() {
            callees.insert(detail.as_str().to_owned());
        }
    }
    callees
        .into_iter()
        .filter_map(|callee| {
            let origin = pack.origin_of(&callee)?;
            if origin.is_builtin() {
                return None;
            }
            Some(Premise {
                subject: callee.clone(),
                trust: "supplied-signature",
                because: format!(
                    "the cost of `{callee}` was not derived - it was declared by a row in \
                     {origin}, and this bound closes over that call because the row says it \
                     is bounded and cannot rebind a local; the premise is that the row is true \
                     of the code actually called"
                ),
            })
        })
        .collect()
}

/// One row a supplied pack replaced.
///
/// The losing row's `why` is carried because it is the half a reader cannot
/// reconstruct: the shipped pack argued a case for the row in writing, and an
/// override that contradicts it should be read next to what it contradicts.
#[derive(Debug, Serialize)]
pub struct SignatureOverride {
    /// The callee both rows claim.
    pub callee: String,
    /// Where the row that lost came from.
    pub overridden: String,
    /// Where the row that won came from.
    pub overridden_by: String,
    /// The argument the losing row made for itself.
    pub replaced_why: String,
}

/// Every override a supplied pack performed, for [`Summary::signature_overrides`].
#[must_use]
pub fn overrides_of(pack: Option<&SignaturePack>) -> Vec<SignatureOverride> {
    pack.map(|pack| {
        pack.shadowed()
            .iter()
            .map(|record| SignatureOverride {
                callee: record.callee.clone(),
                overridden: record.overridden.to_string(),
                overridden_by: record.overridden_by.to_string(),
                replaced_why: record.replaced.why.clone(),
            })
            .collect()
    })
    .unwrap_or_default()
}

/// One function's result.
#[derive(Debug, Serialize)]
pub struct Function {
    /// `Class.method` for a method, the bare name otherwise.
    pub name: String,
    /// The class a method belongs to; absent for a module-level function.
    /// Added by `LAN-104` without a schema bump: a new field, not a changed one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
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
    /// What this function's bound is **believed on**, for the variables it
    /// actually mentions.
    ///
    /// # Not the same thing as a hole's assumption
    ///
    /// A hole names something this run could **not** derive, and makes the
    /// result `partial`. A premise is the opposite: the result may be complete
    /// and exact, and still rest on a fact nothing verified. Every `len(items)`
    /// rests on the annotation being true, which the tool has always assumed
    /// silently; `LAN-106` made a second, weaker premise possible, and
    /// therefore made both worth publishing.
    ///
    /// `trust: "protocol"` is the weaker one: the length comes from a user
    /// `__len__`, and a class whose iteration outruns it makes this bound
    /// exceedable. A consumer that cannot accept that can filter on this field,
    /// and a run can be repeated without those annotations to see which results
    /// change.
    ///
    /// Only the variables the bound mentions appear, so a premise here is
    /// always one the number in front of you depends on. Empty for a function
    /// whose bound rests on nothing of the kind.
    pub premises: Vec<Premise>,
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
        pack: Option<&SignaturePack>,
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
                let projected =
                    crate::resource_bound::ResourceBound::queries_of(cost, function.program());
                FunctionResource {
                    bound: projected.bound,
                    bound_kind: projected.kind,
                    value: projected.value,
                }
            });

        self.functions.push(Function {
            name: function.name().to_owned(),
            class: function.class().map(str::to_owned),
            premises: premises_of(function.program(), pack),
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
