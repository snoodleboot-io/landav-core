//! [`Coverage`] - how much of a program became an integer transition system,
//! and what stopped the rest.

use std::cmp::Reverse;

use crate::{construct::Construct, its::Its, lowering_error::LoweringError};

/// The accumulated result of offering many units to [`crate::lower`].
///
/// # What this type is for
///
/// `LAN-67` built the vocabulary ([`Construct`]), one refusal
/// ([`crate::Unsupported`]) and one unit's ledger ([`crate::Refusals`]), and
/// stopped there. This is the other half of `LAN-68`: the *report*, the
/// accumulation **across** units, and the coverage number.
///
/// # Why a partial analysis must be impossible to mistake for a whole one
///
/// A refused construct produces no transition. A system missing a transition
/// admits fewer executions than the program has, so a bound derived from it
/// can be exceeded - the analysis is unsound *by omission*, and it looks
/// exactly like a clean result. [`crate::lower`] closes that hole for one unit
/// by refusing outright rather than emitting a partial system; this type
/// closes it for a *run*, whose natural failure mode is that four functions
/// out of five refused and the report mentioned only the fifth.
///
/// Every accessor below exists to make that visible: [`Coverage::units`] is
/// the denominator, [`Coverage::is_complete`] is the question a driver must
/// ask before claiming anything, and [`Coverage::percent`] is deliberately
/// unable to reach `100` unless [`Coverage::is_complete`] is true.
///
/// # The denominator is units, not statements
///
/// Refusal is all-or-nothing per unit - see [`LoweringError`] - so "90% of the
/// statements lowered" would describe a function that produced **no**
/// transitions at all as nearly analysed. The honest ratio is units lowered
/// over units attempted, and it is the one reported.
///
/// [`Construct::all`] supplies a second, different denominator: how much of
/// the refusal *vocabulary* a codebase ran into. That is a fact about which
/// constructs block this analysis, not about how much code was covered, and
/// [`Coverage::not_encountered`] keeps the two apart.
///
/// # Language-neutral
///
/// Non-negotiable 4. Nothing here mentions a source language: units are named
/// by whatever [`crate::SourceProgram::name`] the frontend supplied, positions
/// are opaque [`landav_bound::Origin`] strings, and the vocabulary is
/// [`Construct`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    /// Units that produced an [`Its`].
    lowered: usize,
    /// Units that did not, in the order they were offered.
    failures: Vec<LoweringError>,
}

impl Coverage {
    /// A report over nothing.
    ///
    /// Not "a clean report": [`Coverage::percent`] answers `None` here, and a
    /// driver that prints `100%` for a run that lowered nothing has invented a
    /// claim.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            lowered: 0,
            failures: Vec::new(),
        }
    }

    /// Records one unit's result.
    ///
    /// Takes exactly what a caller holds after `lower(&program).as_ref()`, so
    /// that recording is not a separate decision from lowering. There is no
    /// spelling of this call that records a unit *without* saying whether it
    /// lowered, which is the accounting equivalent of non-negotiable 3.
    pub fn record(&mut self, outcome: Result<&Its, &LoweringError>) {
        match outcome {
            Ok(_) => self.lowered += 1,
            Err(error) => self.failures.push(error.clone()),
        }
    }

    /// Folds another report into this one.
    ///
    /// The cross-file half of the accumulation: one report per file, merged
    /// into one report per run. A construct refused once in one function is a
    /// footnote; the same construct refused four hundred times across a tree
    /// is the headline, and only a merged report can say so.
    pub fn merge(&mut self, other: &Self) {
        self.lowered += other.lowered;
        self.failures.extend(other.failures.iter().cloned());
    }

    /// How many units were offered. The denominator.
    #[must_use]
    pub const fn units(&self) -> usize {
        self.lowered + self.failures.len()
    }

    /// How many units produced an [`Its`].
    #[must_use]
    pub const fn lowered(&self) -> usize {
        self.lowered
    }

    /// How many units refused because of a construct outside the fragment.
    #[must_use]
    pub fn refused(&self) -> usize {
        self.failures
            .iter()
            .filter(|error| error.refusals().is_some())
            .count()
    }

    /// How many units were rejected as malformed.
    ///
    /// Counted apart from [`Coverage::refused`] and excluded from every
    /// per-construct total, for the reason [`LoweringError::Malformed`] gives:
    /// a malformed program is a **frontend defect**, and a coverage report that
    /// filed it under an unsupported language construct would send somebody to
    /// write a lowering rule for a construct that does not exist.
    #[must_use]
    pub fn malformed(&self) -> usize {
        self.failures
            .iter()
            .filter(|error| error.refusals().is_none())
            .count()
    }

    /// How many individual refusals were recorded, across every unit.
    ///
    /// Always at least [`Coverage::refused`], because a refusing unit reports
    /// every construct it met rather than only the first.
    #[must_use]
    pub fn refusals(&self) -> usize {
        self.failures
            .iter()
            .filter_map(LoweringError::refusals)
            .map(crate::refusals::Refusals::len)
            .sum()
    }

    /// How many refusals across the whole run name `construct`.
    ///
    /// The aggregation `LAN-68` criterion 2 is about.
    #[must_use]
    pub fn count_of(&self, construct: Construct) -> usize {
        self.failures
            .iter()
            .filter_map(LoweringError::refusals)
            .map(|refusals| refusals.count_of(construct))
            .sum()
    }

    /// Every construct this run actually met, in canonical order.
    #[must_use]
    pub fn constructs(&self) -> Vec<Construct> {
        Construct::all()
            .iter()
            .copied()
            .filter(|construct| self.count_of(*construct) > 0)
            .collect()
    }

    /// Every construct in the vocabulary this run did **not** meet, in
    /// canonical order.
    ///
    /// The half of a coverage report that carries information, and the reason
    /// [`Construct::all`] is public. Together with [`Coverage::constructs`]
    /// this partitions the vocabulary exactly: a reader can tell "we never met
    /// a comprehension" from "we do not have a name for comprehensions".
    #[must_use]
    pub fn not_encountered(&self) -> Vec<Construct> {
        Construct::all()
            .iter()
            .copied()
            .filter(|construct| self.count_of(*construct) == 0)
            .collect()
    }

    /// Every construct met, with its count, most frequent first.
    ///
    /// Ties break on [`Construct::tag`] rather than on the enum's declaration
    /// order, so that reordering the variants - a readability refactor that
    /// breaks no build - cannot reorder a report a baseline has pinned.
    #[must_use]
    pub fn ranked(&self) -> Vec<(Construct, usize)> {
        let mut counts: Vec<(Construct, usize)> = self
            .constructs()
            .into_iter()
            .map(|construct| (construct, self.count_of(construct)))
            .collect();
        counts.sort_by_key(|(construct, count)| (Reverse(*count), construct.tag()));
        counts
    }

    /// The construct that blocked this run most often, if any did.
    #[must_use]
    pub fn dominant(&self) -> Option<(Construct, usize)> {
        self.ranked().first().copied()
    }

    /// Every unit that did not lower, in the order they were offered.
    ///
    /// Each carries its own function name and, for a refusal, a position per
    /// construct.
    #[must_use]
    pub fn failures(&self) -> &[LoweringError] {
        &self.failures
    }

    /// Whether every unit offered produced an [`Its`].
    ///
    /// The question a driver must ask before claiming anything about the
    /// program. A run over no units is vacuously complete - there is nothing
    /// it failed to cover - which is why [`Coverage::units`] has to be
    /// consulted as well before the answer means "we analysed this".
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.failures.is_empty()
    }

    /// The percentage of units that lowered, or `None` if none were offered.
    ///
    /// **Floors, and never rounds up.** 999 units of 1000 is `99`, not `100`:
    /// `100` is a claim of completeness, and a report that reaches it by
    /// rounding claims to have covered a function it refused. `None` rather
    /// than `0` for an empty run, because a ratio with no denominator is not a
    /// number and printing one would be an invention.
    #[must_use]
    pub fn percent(&self) -> Option<u32> {
        let units = self.units();
        if units == 0 {
            return None;
        }
        let scaled = (self.lowered.saturating_mul(100) / units).min(100);
        u32::try_from(scaled).ok()
    }

    /// The one-line form, for a run summary.
    ///
    /// Always names both halves of the ratio, on a complete run as well as a
    /// partial one, so that a number a CI job watches is always there to be
    /// watched. Names the dominant construct when there is one, because the
    /// aggregate is the actionable half: one refusal is a footnote, four
    /// hundred of the same one is the next thing to implement.
    #[must_use]
    pub fn summary(&self) -> String {
        let Some(percent) = self.percent() else {
            return "coverage: no function was offered for lowering".to_owned();
        };
        let mut line = format!(
            "coverage: {} of {} function(s) lowered ({percent}%)",
            self.lowered,
            self.units()
        );
        if self.is_complete() {
            return line;
        }
        line.push_str(&format!(", {} construct(s) out of scope", self.refusals()));
        if let Some((construct, count)) = self.dominant() {
            line.push_str(&format!(" (most frequent: {construct} ×{count})"));
        }
        if self.malformed() > 0 {
            line.push_str(&format!(
                ", {} malformed source program(s)",
                self.malformed()
            ));
        }
        line
    }

    /// The full report: what was out of scope, where, and what it means.
    ///
    /// Ends every line it can with a [`Construct::tag`] and a
    /// [`Construct::describe`], so no line of it is a bare "unknown".
    /// Deterministic: two runs over identical input produce byte-identical
    /// text, because [`Coverage::ranked`] sorts on the stable tag and
    /// [`crate::Refusals`] holds itself sorted.
    #[must_use]
    pub fn report(&self) -> String {
        let mut text = self.summary();
        text.push('\n');
        if self.units() == 0 {
            return text;
        }
        if self.is_complete() {
            text.push_str(
                "  every construct met was inside the numeric fragment, so the whole \
                 program is covered\n",
            );
        } else {
            text.push_str(&format!(
                "  {} of {} function(s) produced no transition system at all, so nothing \
                 is derived from them and no bound reported for this program covers them\n",
                self.units() - self.lowered,
                self.units()
            ));
        }
        self.write_ranking(&mut text);
        self.write_positions(&mut text);
        self.write_malformed(&mut text);
        self.write_vocabulary(&mut text);
        text
    }

    /// The per-construct aggregate, most frequent first.
    fn write_ranking(&self, text: &mut String) {
        let ranked = self.ranked();
        if ranked.is_empty() {
            return;
        }
        text.push_str("\nout of scope, by construct (most frequent first):\n");
        let width = ranked
            .iter()
            .map(|(construct, _)| construct.tag().len())
            .max()
            .unwrap_or_default();
        for (construct, count) in ranked {
            text.push_str(&format!(
                "  {:<width$}  ×{count}  {}\n",
                construct.tag(),
                construct.describe()
            ));
        }
    }

    /// One line per refusal, naming the position the frontend gave it.
    fn write_positions(&self, text: &mut String) {
        let mut any = false;
        for error in &self.failures {
            let Some(refusals) = error.refusals() else {
                continue;
            };
            if !any {
                text.push_str("\nwhere:\n");
                any = true;
            }
            for record in refusals.as_slice() {
                text.push_str(&format!("  {}: {record}\n", error.function()));
            }
        }
    }

    /// The frontend defects, kept apart from the language constructs.
    fn write_malformed(&self, text: &mut String) {
        let malformed: Vec<&LoweringError> = self
            .failures
            .iter()
            .filter(|error| error.refusals().is_none())
            .collect();
        if malformed.is_empty() {
            return;
        }
        text.push_str(
            "\nmalformed source program(s) - a frontend defect, not a language construct:\n",
        );
        for error in malformed {
            text.push_str(&format!("  {error}\n"));
        }
    }

    /// The constructs the run never met, and the size of the vocabulary.
    fn write_vocabulary(&self, text: &mut String) {
        let unmet = self.not_encountered();
        text.push_str(&format!(
            "\nnever met, {} of the {} named constructs:\n",
            unmet.len(),
            Construct::all().len()
        ));
        if unmet.is_empty() {
            text.push_str("  (every construct in the vocabulary was met)\n");
            return;
        }
        let names: Vec<&str> = unmet.iter().map(|construct| construct.tag()).collect();
        text.push_str(&format!("  {}\n", names.join(", ")));
    }
}

impl core::fmt::Display for Coverage {
    /// The one-line form. See [`Coverage::summary`].
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.summary())
    }
}
