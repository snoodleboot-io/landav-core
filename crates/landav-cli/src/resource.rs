//! `--resource` — which resource a run is about, and what each one is waiting
//! on.
//!
//! Every string in this module is rendered from
//! [`landav_bound::registered`]. Nothing here spells out
//! `ops|alloc|peak-mem|queries`, because the registered set is extensible and a
//! second, hand-written list drifts from the first the day an instance is
//! added. That is LAN-60 criterion 3's whole point, and it applies to `--help`
//! for exactly the same reason it applies to the error message.
//!
//! # There is one dynamic-to-static conversion, and this module does not add a
//! second
//!
//! [`landav_bound::ResourceKind::parse`] is it: a `&str` from `argv` becomes a
//! value of a **closed** enum, or a [`landav_bound::BoundError::UnknownResource`]
//! naming the value and the registered set. It is used directly as the `clap`
//! value parser in [`crate::cli`], so the flag cannot be accepted by one
//! spelling of the set and rejected by another.
//!
//! Everything downstream of that point dispatches on the enum by exhaustive
//! `match`, which is static. [`landav_bound::Dioid`] is a generic bound and
//! never a trait object: `fn zero() -> Self::Carrier` is not object safe, and a
//! `dyn Dioid<Carrier = Bound>` registry would hard-block any future analysis
//! whose carrier is not a `Bound` — R4's AARA, for one.
//!
//! # The resource is named, never the algebra alone
//!
//! Three registered resources share the `additive` algebra, so a
//! [`landav_bound::SemiringId`] does not identify what was counted.
//! [`landav_bound::CacheKeyMaterial`] records what follows from forgetting
//! that: a cache keyed on the semiring "serves the allocation bound as the
//! operation count, silently, with plausible numbers". The same trap is
//! available one layer up, in the report — an operator reading a run
//! identified only by `additive` cannot tell which of three questions it
//! answered, and both answers look right. So every line this module produces
//! leads with the [`landav_bound::ResourceId`].
//!
//! # LAN-86: the status is **per resource**, and one of them derives
//!
//! This module used to make one claim for the whole registry: "no bound is
//! derived for any resource in this build". That sentence was true, and it was
//! also the least useful true sentence available. It told a reader that
//! everything was unavailable and never *why*, so `ops` — which is a scaling of
//! a number the engine already derives — read as being in the same state as
//! `peak-mem`, which needs an analysis nothing in the product performs.
//!
//! Two things replace it, and both are per resource:
//!
//! * [`derives`] says whether **this** resource produces a number, and
//! * [`awaiting`] says what **this** resource is waiting on when it does not.
//!
//! The four answers are deliberately four different sentences:
//!
//! | resource | status |
//! |---|---|
//! | `ops` | waiting on a calibration profile — a measurement, not an analysis |
//! | `alloc` | waiting on an IR that can represent data |
//! | `peak-mem` | the same, **and** a second analysis over a different semiring |
//! | `queries` | derived |
//!
//! `ops` is the only resource waiting on a profile, and
//! [`landav_calibrate`] is the component that emits one. That crate is named
//! here rather than described in the abstract because "waiting on a
//! calibration" sends a reader nowhere, and "waiting on the profile
//! `landav-calibrate` produces" sends them to a thing they can go and make.
//!
//! # Why `queries` derives now and did not when LAN-86 was written
//!
//! The ticket says: "`queries` counts external calls, and calls are refused
//! outright. It cannot be non-zero for any function that currently lowers."
//! That was true. LAN-87 changed it: a call is no longer a refusal that
//! discards the function, it is a named [`landav_engine::Hole`] carried in the
//! result at its position in the control structure. Counting the calls a
//! function issues is therefore real, derivable information, read off a
//! structure the engine already builds. See [`crate::resource_bound`] for how,
//! and for the one direction that count may never move.

use landav_bound::ResourceKind;

/// The summary clause for a resource this build derives no number for.
///
/// Every statement this crate makes about a resource's *status* is rendered
/// from [`derives`] and [`awaiting`], so the lane that unblocks one of them has
/// one function to revisit rather than a phrase to hunt for. A caveat left
/// behind in a corner of the report after it has stopped being true is a lie
/// the tool tells about itself — which is exactly what LAN-86 found.
const NO_BOUND: &str = "no bound derived";

/// The summary clause for a resource this build does derive a number for.
const BOUND_DERIVED: &str = "bound derived per function";

/// Whether this build produces a number for `kind`.
///
/// Written as an exhaustive match with **no wildcard arm**: registering a fifth
/// resource is a compile error here until somebody decides whether it derives,
/// which is the same discipline [`crate::outcome`] uses for the exit codes. A
/// `_ => false` would quietly enrol every future resource in the excuse list.
pub const fn derives(kind: ResourceKind) -> bool {
    match kind {
        // LAN-87 turned a call from a refusal that discarded the function into
        // a named hole carried in the result. Counting them is real.
        ResourceKind::Queries => true,
        ResourceKind::Ops | ResourceKind::Alloc | ResourceKind::PeakMem => false,
    }
}

/// What `kind` is waiting on, or `None` when it derives a number.
///
/// # Why these are four sentences and not one
///
/// This is LAN-86's acceptance criterion. A blanket "no bound is derived for
/// any resource in this build" is true of three of the four and tells a reader
/// nothing they can act on: it does not distinguish `ops`, which is waiting on
/// a *measurement* of an analysis that already exists, from `peak-mem`, which
/// is waiting on two pieces of work neither of which has started. A reader
/// given the blanket message reasonably concludes that all four arrive
/// together, and none of them do.
///
/// The `None`/`Some` split is the same fact [`derives`] reports, in the shape a
/// report needs, and the two are checked against each other in this module's
/// tests: a resource that derives a number and also claims to be waiting on
/// something is making two contradictory claims about one run.
#[must_use]
pub fn awaiting(kind: ResourceKind) -> Option<String> {
    let descriptor = kind.descriptor();
    match kind {
        ResourceKind::Queries => None,

        // A *measurement*, not an analysis. The engine already derives the step
        // count this resource scales; what is missing is what one step costs on
        // the machine the code will run on. `landav-calibrate` is the component
        // that emits that, and naming it is the difference between a caveat and
        // an instruction.
        ResourceKind::Ops => Some(format!(
            "a calibration profile. The engine already derives the step count \
             `{id}` scales, so this is the one registered resource that is not \
             waiting on an analysis: it is waiting on a measurement of what one \
             step costs on the target machine, which is the profile the \
             `landav-calibrate` component emits and which every concrete \
             estimate must name. Until a profile is loaded, a number in {unit} \
             would be a step count wearing a different unit",
            id = descriptor.id(),
            unit = descriptor.unit(),
        )),

        // Not blocked on a constant and not blocked on a solver. The analysable
        // fragment is integer scalars, and an integer scalar has no size.
        ResourceKind::Alloc => Some(format!(
            "an IR that can represent data. The analysable fragment is integer \
             scalars and nothing in it allocates, so `{id}` is blocked on \
             neither a calibration nor a solver: until the Landav IR can \
             represent values that have sizes, there is nothing for a bound in \
             {unit} to be a bound on",
            id = descriptor.id(),
            unit = descriptor.unit(),
        )),

        // Everything `alloc` waits on, plus an analysis that does not exist. A
        // peak is a maximum over the run's lifetime, and no scaling of a sum
        // produces one - which is why this is the only registered resource
        // whose semiring is not the additive one.
        ResourceKind::PeakMem => Some(format!(
            "an IR that can represent data, and then a second analysis. Nothing \
             in an integer-scalar fragment allocates, so `{id}` waits on \
             representation exactly as `{alloc}` does - and then it waits again, \
             because it instantiates the `{peak}` semiring rather than the \
             `{additive}` semiring `{alloc}` shares with `{ops}`. A live-memory \
             peak is a maximum over the whole run, not a scaled sum, so it is a \
             different analysis over a different algebra and does not arrive \
             with `{alloc}`",
            id = descriptor.id(),
            peak = descriptor.semiring().as_str(),
            alloc = ResourceKind::Alloc.descriptor().id(),
            additive = ResourceKind::Alloc.descriptor().semiring().as_str(),
            ops = ResourceKind::Ops.descriptor().id(),
        )),
    }
}

/// The one-line `--help` description, listing the registered values.
///
/// The list is [`ResourceKind::registered_names`] joined in registration order
/// — the same rendering [`landav_bound::BoundError::UnknownResource`] uses, so
/// what `-h` advertises and what a rejection lists cannot disagree.
pub fn summary() -> String {
    format!(
        "Which resource to bound: {}",
        ResourceKind::registered_names().join(", ")
    )
}

/// The `--help` long form: one line per registered resource, then its status.
///
/// Each line carries the descriptor's own id, unit and summary, so registering
/// a resource extends `--help` with no edit here. The status underneath it is
/// rendered from [`derives`] and [`awaiting`] for the same reason, and it is
/// not optional garnish: without it the flag reads as a promise to report a
/// number for all four, and this build reports one for one of them.
pub fn detail() -> String {
    let mut text = String::from("Which resource to bound.\n\nRegistered resources:\n");
    for kind in ResourceKind::ALL {
        let descriptor = kind.descriptor();
        text.push_str(&format!(
            "  {}: {}, in {} (`{}` semiring)\n    {}\n",
            descriptor.id(),
            descriptor.summary(),
            descriptor.unit(),
            descriptor.semiring().as_str(),
            status(*kind),
        ));
    }
    text.push_str(
        "\nSelecting a resource that derives no bound reports the run as\n\
         inconclusive rather than clean, and never reports a number.\n\
         Selecting one that does derive reports it per function, beside\n\
         the cost bound, and a run whose question was answered is clean.",
    );
    text
}

/// One resource's status, as the sentence `--help` and the JSON both carry.
///
/// The two surfaces render the same function rather than two strings that agree
/// today, which is the whole failure LAN-86 is about.
#[must_use]
pub fn status(kind: ResourceKind) -> String {
    let descriptor = kind.descriptor();
    awaiting(kind).map_or_else(
        || {
            format!(
                "a bound is derived for `{}` in this build, per function, \
                 counted once per loop iteration and taking the worse arm of a \
                 branch",
                descriptor.id()
            )
        },
        |excuse| {
            format!(
                "no bound is derived for `{}` in this build; it is waiting on {excuse}",
                descriptor.id()
            )
        },
    )
}

/// The report line for a run that was asked for a bound it cannot derive, or
/// `None` when the resource derives one.
///
/// Names the resource first and the algebra second, and says what *this*
/// resource is waiting on rather than printing a number. "inconclusive" is the
/// same word the unreadable-source diagnostic uses, because it is the same fact
/// about the run: something was asked and nothing was concluded.
///
/// `None` is not "say nothing": it is "this resource has an answer, so the
/// unaccounted-for line would be false". [`crate::check`] prints the answer
/// instead.
#[must_use]
pub fn unaccounted(kind: ResourceKind) -> Option<String> {
    let descriptor = kind.descriptor();
    let excuse = awaiting(kind)?;
    Some(format!(
        "landav: inconclusive: no bound was derived for `{}` ({}); landav would \
         bound it in the `{}` semiring, so nothing is claimed about the code \
         analysed here. It is waiting on {excuse}",
        descriptor.id(),
        descriptor.unit(),
        descriptor.semiring().as_str(),
    ))
}

/// The run summary's clause naming the question the run was asked.
///
/// Printed on every run that selected a resource, including the ones that found
/// nothing to analyse. A run whose summary does not say which resource it was
/// about is a run whose output cannot be filed against the invocation that
/// produced it.
///
/// It ends with whether a bound was derived because the summary is one line and
/// is often the only line that gets read. The rest of that line counts
/// *units* — findings, waivers, files nothing could be concluded about — and a
/// selected resource is not a unit, so it contributes to none of them. A
/// summary reading `0 finding(s), ..., 0 inconclusive` beside exit `1` looks
/// like a defect in the tool unless the same line says why.
pub fn selected(kind: ResourceKind) -> String {
    let descriptor = kind.descriptor();
    format!(
        "`{}` ({}), `{}` semiring, {}",
        descriptor.id(),
        descriptor.unit(),
        descriptor.semiring().as_str(),
        if derives(kind) {
            BOUND_DERIVED
        } else {
            NO_BOUND
        }
    )
}

#[cfg(test)]
mod tests {
    use super::{awaiting, derives, detail, selected, status, summary, unaccounted};
    use landav_bound::{ResourceKind, registered};

    /// Every registered value appears in both help renderings, and neither
    /// carries a name the registry does not have. Driven from the registry, so
    /// a resource registered tomorrow is covered by this test today.
    #[test]
    fn both_help_renderings_are_generated_from_the_registry() {
        let short = summary();
        let long = detail();
        for descriptor in registered() {
            assert!(short.contains(descriptor.id().as_str()), "{short}");
            assert!(long.contains(descriptor.id().as_str()), "{long}");
            assert!(long.contains(descriptor.summary()), "{long}");
            assert!(long.contains(descriptor.unit()), "{long}");
        }
        assert!(
            short.ends_with(&ResourceKind::registered_names().join(", ")),
            "the short help must end with the registered set and nothing after \
             it, or it is advertising a value the registry does not hold: \
             {short}"
        );
    }

    /// LAN-86, in the one place a caller is most likely to read it: `--help`
    /// makes its claim **per resource**, and never for the registry as a whole.
    ///
    /// This replaces `the_long_help_does_not_promise_a_bound`, which asserted
    /// "No bound is derived for any resource". That sentence stopped being true
    /// the moment `queries` derived one, and a stale caveat is a lie the tool
    /// tells about itself - the argument this module's own documentation makes.
    /// The honest replacement is the same statement per resource, which is what
    /// this checks: the help must not promise a number for a resource that
    /// derives none, and must not deny one for the resource that does.
    #[test]
    fn the_long_help_promises_a_bound_only_where_one_is_derived() {
        let long = detail();
        assert!(
            !long.to_lowercase().contains("no bound is derived for any"),
            "the help still makes one claim for the whole registry, which is \
             the state LAN-86 exists to end: {long}"
        );
        for kind in ResourceKind::ALL {
            let id = kind.descriptor().id();
            let denial = format!("no bound is derived for `{id}`");
            if derives(*kind) {
                assert!(
                    !long.contains(&denial),
                    "`{id}` derives a number and `--help` says it does not: {long}"
                );
            } else {
                assert!(
                    long.contains(&denial),
                    "`{id}` derives no number and `--help` does not say so: {long}"
                );
            }
        }
    }

    /// Every resource either derives a number or says what **it** is waiting
    /// on, and never both.
    ///
    /// The "never both" half is the one that catches a half-finished
    /// implementation: a resource reporting a derived number *and* something it
    /// is waiting on has made two contradictory claims about the same run, and
    /// a consumer has no way to tell which one to believe.
    #[test]
    fn a_resource_either_derives_or_says_what_it_is_waiting_on() {
        let mut derived_any = false;
        for kind in ResourceKind::ALL {
            let id = kind.descriptor().id();
            match awaiting(*kind) {
                None => {
                    derived_any = true;
                    assert!(
                        derives(*kind),
                        "`{id}` is waiting on nothing and derives nothing, so \
                         the report has no honest thing to say about it"
                    );
                }
                Some(excuse) => {
                    assert!(
                        !derives(*kind),
                        "`{id}` derives a number and also states what it is \
                         waiting on: {excuse}"
                    );
                    assert!(
                        !excuse.trim().is_empty(),
                        "`{id}` states an empty excuse, which is the blanket \
                         message with the words removed"
                    );
                }
            }
        }
        assert!(
            derived_any,
            "no registered resource derives a number; `queries` counts the call \
             holes LAN-87 made real, so at least one must"
        );
    }

    /// No two resources give the same excuse.
    ///
    /// This is LAN-86's acceptance criterion stated as a property rather than
    /// as four strings. One blanket message satisfies "every resource says
    /// something" for all of them and this for none, which is exactly the state
    /// the ticket was filed about: a reader learns that everything is
    /// unavailable and never why `peak-mem` is unavailable for a different
    /// reason from `ops`.
    #[test]
    fn no_two_resources_give_the_same_excuse() {
        let excuses: Vec<(String, String)> = ResourceKind::ALL
            .iter()
            .filter_map(|kind| {
                awaiting(*kind).map(|excuse| (kind.descriptor().id().to_string(), excuse))
            })
            .collect();
        for (index, (left_id, left)) in excuses.iter().enumerate() {
            for (right_id, right) in excuses.iter().skip(index + 1) {
                assert_ne!(
                    left, right,
                    "`{left_id}` and `{right_id}` are waiting on different \
                     things and gave the same sentence"
                );
            }
        }
    }

    /// The excuses say the specific thing each resource is blocked on, and the
    /// three are told apart by what they name.
    ///
    /// Spelling the resources out is allowed here, and only here, for the
    /// reason `resource_registry.rs` gives: "`ops` needs a calibration profile"
    /// is a claim about a *particular* resource, and the ticket's acceptance is
    /// precisely that those claims stop being interchangeable. Everything
    /// set-shaped above is still driven from the registry.
    #[test]
    fn each_excuse_names_what_that_resource_is_blocked_on() {
        let ops = awaiting(ResourceKind::Ops)
            .unwrap_or_default()
            .to_lowercase();
        assert!(ops.contains("calibration"), "{ops}");
        assert!(
            ops.contains("profile"),
            "`ops` must name the profile as the missing input, not calibration \
             in the abstract - the profile is the artefact a user can go and \
             make: {ops}"
        );

        for kind in [ResourceKind::Alloc, ResourceKind::PeakMem] {
            let text = awaiting(kind).unwrap_or_default().to_lowercase();
            assert!(text.contains("data"), "{text}");
            assert!(
                text.contains("represent"),
                "`{}` must name representation as what is missing, so a reader \
                 can tell it from `ops`, which is waiting on a number for an \
                 analysis that already exists: {text}",
                kind.descriptor().id()
            );
        }

        let peak = ResourceKind::PeakMem.descriptor();
        let text = awaiting(ResourceKind::PeakMem)
            .unwrap_or_default()
            .to_lowercase();
        assert!(text.contains("semiring"), "{text}");
        assert!(
            text.contains(&peak.semiring().as_str().to_lowercase()),
            "`peak-mem` must name the `{}` semiring it instantiates: a peak is \
             a maximum over the run, not a scaled sum, so it is a different \
             analysis from `alloc` and not a later delivery of the same one: \
             {text}",
            peak.semiring().as_str()
        );
    }

    /// The two report renderings name the resource, not just the algebra.
    ///
    /// Asserted over a pair that *shares* an algebra, which is the only case
    /// where it can go wrong: identifying a run by its semiring makes `ops` and
    /// `alloc` indistinguishable, which is `CacheKeyMaterial`'s documented trap
    /// moved into the report.
    #[test]
    fn resources_sharing_an_algebra_render_differently() {
        assert_eq!(
            ResourceKind::Ops.descriptor().semiring(),
            ResourceKind::Alloc.descriptor().semiring()
        );
        assert_ne!(
            unaccounted(ResourceKind::Ops),
            unaccounted(ResourceKind::Alloc)
        );
        assert_ne!(selected(ResourceKind::Ops), selected(ResourceKind::Alloc));
    }

    /// Both renderings carry the resource *and* its algebra, for every
    /// registered resource. Criterion 2 is only observable if the mapping is
    /// printed.
    ///
    /// The unaccounted-for line exists only for a resource that derives
    /// nothing; for one that derives, there is nothing unaccounted for and the
    /// line would be false rather than merely redundant.
    #[test]
    fn every_rendering_names_the_resource_and_its_algebra() {
        for kind in ResourceKind::ALL {
            let descriptor = kind.descriptor();
            let mut lines = vec![selected(*kind), status(*kind)];
            lines.extend(unaccounted(*kind));
            assert_eq!(
                unaccounted(*kind).is_none(),
                derives(*kind),
                "the unaccounted-for line and the derived flag must agree about \
                 `{}`",
                descriptor.id()
            );
            for line in lines {
                assert!(
                    line.contains(descriptor.id().as_str()),
                    "a rendering that does not name the resource cannot be \
                     filed against the invocation that produced it: {line}"
                );
            }
            let clause = selected(*kind);
            assert!(
                clause.contains(&format!("`{}` ({})", descriptor.id(), descriptor.unit())),
                "{clause}"
            );
            assert!(
                clause.contains(&format!("`{}` semiring", descriptor.semiring().as_str())),
                "{clause}"
            );
        }
    }

    /// The unaccounted line says the run concluded nothing, in the same word
    /// the rest of the report uses for that state, and says what that resource
    /// is waiting on rather than blaming the build as a whole.
    #[test]
    fn the_unaccounted_line_says_nothing_was_concluded() {
        let line = unaccounted(ResourceKind::PeakMem).unwrap_or_default();
        assert!(line.contains("inconclusive"), "{line}");
        assert!(line.contains("no bound was derived"), "{line}");
        assert!(line.contains("semiring"), "{line}");
        assert!(
            unaccounted(ResourceKind::Queries).is_none(),
            "a resource that derives a number is never unaccounted for"
        );
    }

    /// The summary clause says, per resource, whether a bound was derived.
    ///
    /// The rest of the summary counts units, and a selected resource is not a
    /// unit, so it contributes to none of those counts. `0 finding(s), ...,
    /// 0 inconclusive` beside exit `1` reads as a defect in the tool unless the
    /// same line says why the run could not conclude - and beside exit `0` it
    /// reads as a resource nobody asked about unless the line says a number was
    /// produced.
    #[test]
    fn the_summary_clause_says_whether_a_bound_was_derived() {
        for kind in ResourceKind::ALL {
            let clause = selected(*kind);
            if derives(*kind) {
                assert!(clause.contains(super::BOUND_DERIVED), "{clause}");
                assert!(!clause.contains(super::NO_BOUND), "{clause}");
            } else {
                assert!(clause.contains(super::NO_BOUND), "{clause}");
            }
        }
    }
}
