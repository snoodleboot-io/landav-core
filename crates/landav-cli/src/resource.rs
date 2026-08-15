//! `--resource` — which resource a run is about.
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
//! # What the flag does at this milestone, and what it does not
//!
//! It parses, it is validated against the registry, and it is reported. It
//! does **not** produce a bound, because nothing in this build derives one:
//! `landav-its` and `landav-solvers` are empty, and the `landav-python`
//! pattern rules are not resource-parameterised.
//!
//! The honest consequence is that a run which was asked for a bound and has
//! none to give is [`crate::outcome::Outcome::Inconclusive`], not
//! [`crate::outcome::Outcome::Clean`]. Exit `0` is the claim "analysis ran and
//! every bound held"; making it for a resource nothing propagates through
//! would be a fabrication, and a green build nobody checked is worse than a red
//! one. [`unaccounted`] is the line that says so, and [`detail`] is `--help`
//! saying it in advance, so that the first person to pass `--resource` reads
//! the result as the tool's limitation rather than as a bug.

use landav_bound::{ResourceKind, registered};

/// The M0 caveat, in the words the run summary uses.
///
/// Every statement this crate makes about the milestone limitation is in this
/// module — this constant, [`detail`]'s closing paragraph and [`unaccounted`] —
/// so that the lane which lands bound inference has one file to revisit rather
/// than a phrase to hunt for. A caveat left behind in a corner of the report
/// after it has stopped being true is a lie the tool tells about itself.
const NO_BOUND: &str = "no bound derived";

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

/// The `--help` long form: one line per registered resource, then the caveat.
///
/// Each line carries the descriptor's own id, unit and summary, so registering
/// a resource extends `--help` with no edit here. The caveat is not optional
/// garnish: without it the flag reads as a promise to report a number, and this
/// build reports none.
pub fn detail() -> String {
    let mut text = String::from("Which resource to bound.\n\nRegistered resources:\n");
    for descriptor in registered() {
        text.push_str(&format!(
            "  {}: {}, in {} (`{}` semiring)\n",
            descriptor.id(),
            descriptor.summary(),
            descriptor.unit(),
            descriptor.semiring().as_str()
        ));
    }
    text.push_str(
        "\nNo bound is derived for any resource in this build.\n\
         The analysis that would derive one is not implemented at this\n\
         milestone, so selecting a resource reports the run as\n\
         inconclusive rather than clean, and never reports a number.\n\
         The selection is parsed, checked against the registry above\n\
         and reported; it does not yet change what is analysed.",
    );
    text
}

/// The report line for a run that was asked for a bound it cannot derive.
///
/// Names the resource first and the algebra second, and says why there is no
/// number rather than printing one. "inconclusive" is the same word the
/// unreadable-source diagnostic uses, because it is the same fact about the
/// run: something was asked and nothing was concluded.
pub fn unaccounted(kind: ResourceKind) -> String {
    let descriptor = kind.descriptor();
    format!(
        "landav: inconclusive: no bound was derived for `{}` ({}); landav would \
         bound it in the `{}` semiring, but this build derives no bound for any \
         resource, so nothing is claimed about the code analysed here",
        descriptor.id(),
        descriptor.unit(),
        descriptor.semiring().as_str()
    )
}

/// The run summary's clause naming the question the run was asked.
///
/// Printed on every run that selected a resource, including the ones that found
/// nothing to analyse. A run whose summary does not say which resource it was
/// about is a run whose output cannot be filed against the invocation that
/// produced it.
///
/// It ends with the caveat because the summary is one line and is often the
/// only line that gets read. The rest of that line counts *units* — findings,
/// waivers, files nothing could be concluded about — and a selected resource is
/// not a unit, so it contributes to none of them. A summary reading
/// `0 finding(s), ..., 0 inconclusive` beside exit `1` looks like a defect in
/// the tool unless the same line says why.
pub fn selected(kind: ResourceKind) -> String {
    let descriptor = kind.descriptor();
    format!(
        "`{}` ({}), `{}` semiring, {}",
        descriptor.id(),
        descriptor.unit(),
        descriptor.semiring().as_str(),
        NO_BOUND
    )
}

#[cfg(test)]
mod tests {
    use super::{detail, selected, summary, unaccounted};
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

    /// `--help` must not promise a number this build cannot produce. The first
    /// caller to pass `--resource` reads this text and nothing else.
    #[test]
    fn the_long_help_does_not_promise_a_bound() {
        assert!(detail().contains("No bound is derived for any resource"));
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
    #[test]
    fn every_rendering_names_the_resource_and_its_algebra() {
        for kind in ResourceKind::ALL {
            let descriptor = kind.descriptor();
            for line in [unaccounted(*kind), selected(*kind)] {
                assert!(
                    line.contains(&format!("`{}` ({})", descriptor.id(), descriptor.unit())),
                    "{line}"
                );
                assert!(
                    line.contains(&format!("`{}` semiring", descriptor.semiring().as_str())),
                    "{line}"
                );
            }
        }
    }

    /// The unaccounted line says the run concluded nothing, in the same word
    /// the rest of the report uses for that state.
    #[test]
    fn the_unaccounted_line_says_nothing_was_concluded() {
        let line = unaccounted(ResourceKind::PeakMem);
        assert!(line.contains("inconclusive"), "{line}");
        assert!(line.contains("no bound was derived"), "{line}");
    }

    /// The summary clause carries the caveat too.
    ///
    /// The rest of the summary counts units, and a selected resource is not a
    /// unit, so it contributes to none of those counts. `0 finding(s), ...,
    /// 0 inconclusive` beside exit `1` reads as a defect in the tool unless the
    /// same line says why the run could not conclude.
    #[test]
    fn the_summary_clause_says_why_the_run_could_not_conclude() {
        for kind in ResourceKind::ALL {
            let clause = selected(*kind);
            assert!(clause.contains(super::NO_BOUND), "{clause}");
        }
    }
}
