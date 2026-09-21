//! [`SignaturePack`] - a table of [`Signature`] rows, and how one is read.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    pack_error::PackError, pack_origin::PackOrigin, result_length::ResultLength,
    shadowed::Shadowed, signature::Signature,
};

/// The OSS builtin pack, as data.
///
/// # This is a file, not a `match`
///
/// The crate-level docs say packs are data and never compiled in, and this
/// `include_str!` is the one place that claim has to be read carefully. What is
/// embedded is the *bytes of a TOML file*, parsed at runtime by the same
/// [`SignaturePack::parse`] any other pack goes through. There is no Rust arm
/// per callee anywhere in this workspace, and adding a row is editing a data
/// file that a reviewer reads as a table.
///
/// It is embedded rather than discovered from an install path because the
/// builtin pack is the *default* - a landav that cannot find its own defaults
/// is a landav that silently analyses Python without knowing what `isinstance`
/// is - and because a default that depends on where the binary was installed is
/// a default that differs between a developer's checkout and a container. A
/// user-supplied or `landav-ee` pack is discovered at runtime and goes through
/// `parse`; nothing about that path is different.
const BUILTIN: &str = include_str!("builtin_signatures.toml");

/// The newest pack format this build reads.
///
/// # Why a pack carries a version at all
///
/// [`PackFile`] and [`Signature`] are both `deny_unknown_fields`, and that is
/// deliberate: a row that misspells `rebinds_locals` must not be accepted with
/// the field defaulted, because the default is `false` and a row silently
/// claiming it cannot rebind the caller's locals is the one lie this analysis
/// cannot survive. Strictness there is a soundness property, not tidiness.
///
/// The cost of that strictness is that *any* field added later — this one
/// included — makes a newer pack unreadable by an older binary. That is
/// unavoidable. What is avoidable is the older binary saying `unknown field
/// format`, which reads as "your pack is broken" and sends somebody to edit a
/// file that is correct.
///
/// So the version is read in its own permissive pass before the strict one,
/// and a pack from the future is refused by number. Note what this does *not*
/// do: it cannot help a binary built before this change, which has no probe
/// and will still report `unknown field`. It buys forward compatibility only
/// from here on, which is why it lands while every crate is `0.0.0` and no
/// pack exists that somebody else wrote. The same key added after a release
/// would be worth much less.
///
/// A pack with no `format` key is format 1, so nothing already written needs
/// editing.
pub const PACK_FORMAT: u32 = 1;

/// The rows of a pack, keyed by callee.
///
/// # Matching is by name, and that is unsound in general
///
/// `isinstance` is a name, not an identity. A module is free to write
/// `def isinstance(x, t): ...` or `from mymod import isinstance`, and a pack
/// keyed by name would then resolve a call to something it knows nothing about
/// - which is the one direction this analysis must never go.
///
/// So this type does not decide on its own. [`Self::signature`] takes the set
/// of names the module and the enclosing function **bind**, and a callee whose
/// name is bound anywhere in that set gets no signature at all. The frontend
/// supplies the set, because binding is a language question and this crate is
/// language-agnostic.
///
/// The residual risk is a name bound by a wildcard import: `from mymod import *`
/// can rebind `isinstance` with nothing at the call site or in the module's
/// binding set to show for it. That risk is *recorded* rather than removed - a
/// frontend that wants it removed must resolve the wildcard, which needs the
/// imported module's contents and is `LAN-87`'s slice 2. The same trust is
/// already extended to the `int` annotation and to `len` for a collection
/// parameter, both of which a module can lie about in exactly the same way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignaturePack {
    rows: BTreeMap<String, Row>,
    shadowed: Vec<Shadowed>,
}

/// One row and where it came from.
///
/// Origin is kept beside the row rather than inside [`Signature`] because a
/// pack author does not declare it — see [`PackOrigin`] for why a pack that
/// could name its own origin is a pack that could name someone else's.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    signature: Signature,
    origin: PackOrigin,
}

/// The on-disk shape: a `format`, `[[signature]]` tables, and nothing else.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    #[serde(default)]
    signature: Vec<Signature>,
    /// Ignored here — [`FormatProbe`] has already checked it. Declared so the
    /// strict pass accepts the key rather than refusing the pack that carries
    /// it.
    #[serde(default, rename = "format")]
    _format: Option<u32>,
}

/// The first pass: the pack's `format` and nothing else.
///
/// Permissive on purpose, and the only place in this module that is. It must
/// read the version out of a pack whose *other* fields this build has never
/// heard of, which is precisely the pack the strict pass cannot get through.
#[derive(Debug, Deserialize)]
struct FormatProbe {
    #[serde(default)]
    format: Option<u32>,
}

impl SignaturePack {
    /// The OSS builtin pack.
    ///
    /// # Errors
    ///
    /// Only if the shipped file stops parsing, which a test in this crate
    /// prevents from reaching a release. It is still a `Result` rather than a
    /// panic: this is library code, and the workspace forbids a library that
    /// aborts the process over its own data.
    pub fn builtin() -> Result<Self, PackError> {
        Self::parse(&PackOrigin::Builtin, BUILTIN)
    }

    /// Reads a pack from TOML, attributing every row to `origin`.
    ///
    /// # Errors
    ///
    /// [`PackError::UnsupportedFormat`] if the pack declares a format newer
    /// than [`PACK_FORMAT`] — checked first, so a pack from a newer landav is
    /// refused by version rather than by whichever unknown field serde reached
    /// on the way past. Then [`PackError::Malformed`] if the text is not a
    /// pack, [`PackError::Duplicate`] if two rows claim one callee, and
    /// [`PackError::UnexplainedRow`] if a row's `why` is empty.
    pub fn parse(origin: &PackOrigin, text: &str) -> Result<Self, PackError> {
        // Version first. A probe that cannot read the text is not reported
        // here: the text is not TOML at all, and the strict pass below says so
        // in the terms it already used.
        if let Ok(probe) = toml::from_str::<FormatProbe>(text) {
            let found = probe.format.unwrap_or(PACK_FORMAT);
            if found > PACK_FORMAT {
                return Err(PackError::UnsupportedFormat {
                    found,
                    supported: PACK_FORMAT,
                });
            }
        }
        let file: PackFile = toml::from_str(text).map_err(|error| PackError::Malformed {
            reason: error.to_string(),
        })?;
        let mut rows: BTreeMap<String, Row> = BTreeMap::new();
        for row in file.signature {
            if row.why.trim().is_empty() {
                return Err(PackError::UnexplainedRow { callee: row.callee });
            }
            if rows.contains_key(&row.callee) {
                return Err(PackError::Duplicate { callee: row.callee });
            }
            rows.insert(
                row.callee.clone(),
                Row {
                    signature: row,
                    origin: origin.clone(),
                },
            );
        }
        Ok(Self {
            rows,
            shadowed: Vec::new(),
        })
    }

    /// Lays `other`'s rows over this pack's, recording what they replaced.
    ///
    /// # Overriding is allowed; overriding invisibly is not
    ///
    /// [`PackError::Duplicate`] refuses two rows for one callee *within* a
    /// pack. This is the other case and it answers differently, because
    /// overriding the builtin is the reason to supply a pack at all — see
    /// [`Shadowed`], which is where the argument and the losing row are kept.
    ///
    /// Overlaying is last-wins by row and not by pack: a pack that speaks about
    /// one callee replaces that one row and leaves the rest of the builtin
    /// standing. Replacing the pack wholesale would mean a deployment
    /// correcting a single row had to restate every row it agreed with, and the
    /// rows it forgot would become holes rather than errors.
    ///
    /// `other`'s own shadow records come along, so overlaying three packs in
    /// sequence reports every override rather than only the last one.
    pub fn overlay(&mut self, other: Self) {
        self.shadowed.extend(other.shadowed);
        for (callee, row) in other.rows {
            if let Some(previous) = self.rows.insert(callee.clone(), row.clone()) {
                self.shadowed.push(Shadowed {
                    callee,
                    overridden: previous.origin,
                    overridden_by: row.origin,
                    replaced: previous.signature,
                });
            }
        }
    }

    /// Every row an overlay replaced, in the order the overlays happened.
    ///
    /// Empty for a pack that was never overlaid. A driver reports these for
    /// the reason it reports a suppression that suppressed nothing: an
    /// override nobody can see is an override nobody can review.
    #[must_use]
    pub fn shadowed(&self) -> &[Shadowed] {
        &self.shadowed
    }

    /// Where this pack's row for `callee` came from, if it has one.
    ///
    /// Answers for any row the pack holds, resolvable or not — the question
    /// "who said this" is as worth asking about a refusal as about an
    /// admission.
    #[must_use]
    pub fn origin_of(&self, callee: &str) -> Option<&PackOrigin> {
        self.rows.get(callee).map(|row| &row.origin)
    }

    /// The signature for `callee`, if this pack has one a frontend may resolve
    /// and the name is not bound in the source being analysed.
    ///
    /// `bound_names` is every name the module and the enclosing function bind:
    /// a `def`, a `class`, an `import`, a parameter, an assignment. A callee
    /// found there is a callee this pack knows nothing about, whatever the row
    /// says. See the type's own note for what that does and does not cover.
    ///
    /// A row that is present but not resolvable - `join`, `sorted` - answers
    /// `None` here exactly as an absent one does. The difference between them is
    /// visible to a reader of the pack and to [`Self::row`], and deliberately
    /// invisible to a caller deciding whether to hole a call.
    #[must_use]
    pub fn signature<F>(&self, callee: &str, bound_names: F) -> Option<&Signature>
    where
        F: FnOnce(&str) -> bool,
    {
        let row = &self.rows.get(callee)?.signature;
        if !row.is_resolvable() || bound_names(callee) {
            return None;
        }
        Some(row)
    }

    /// What `callee`'s result's length is, if this pack declares it and the
    /// name is not bound in the source being analysed.
    ///
    /// # Two accessors, one shadowing gate, two admissibility gates
    ///
    /// This is deliberately **not** [`Self::signature`] with a different field
    /// read off the end. The two answer different questions and admit different
    /// rows, and `sorted` is the row that proves it: `signature` refuses it,
    /// because its cost is `n log n` and no frontend may treat the call as
    /// discharged; this one admits it, because its result holds exactly as many
    /// values as its argument. A frontend that asked one question and used the
    /// other answer would publish a complete bound for a program that sorts.
    ///
    /// What the two share is the shadowing rule, and they share it because it
    /// is about the *name* rather than about the claim: a module that writes
    /// `def sorted(x)` is not calling the builtin, and neither the cost nor the
    /// length of a callee this pack has never seen is anything to declare. See
    /// the type's own note for what that rule does and does not cover.
    #[must_use]
    pub fn length_relation<F>(&self, callee: &str, bound_names: F) -> Option<ResultLength>
    where
        F: FnOnce(&str) -> bool,
    {
        let row = &self.rows.get(callee)?.signature;
        if !row.result_length.is_declared() || bound_names(callee) {
            return None;
        }
        Some(row.result_length)
    }

    /// The row for `callee`, resolvable or not.
    ///
    /// For a reader of the table - a report, a test - rather than for a
    /// frontend deciding what to do with a call site.
    #[must_use]
    pub fn row(&self, callee: &str) -> Option<&Signature> {
        self.rows.get(callee).map(|row| &row.signature)
    }

    /// Every row, in callee order.
    pub fn rows(&self) -> impl Iterator<Item = &Signature> {
        self.rows.values().map(|row| &row.signature)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::path::PathBuf;

    use super::*;
    use crate::cost_class::CostClass;

    #[test]
    fn the_builtin_pack_parses() {
        let pack = SignaturePack::builtin().expect("the shipped pack must parse");
        assert!(pack.rows().count() > 0, "the shipped pack must have rows");
    }

    #[test]
    fn every_builtin_row_explains_itself() {
        let pack = SignaturePack::builtin().expect("the shipped pack must parse");
        for row in pack.rows() {
            assert!(
                row.why.trim().len() > 20,
                "the row for `{}` explains nothing a reader could argue with: {:?}",
                row.callee,
                row.why
            );
        }
    }

    #[test]
    fn a_row_that_is_not_constant_is_not_resolvable() {
        let pack = SignaturePack::builtin().expect("the shipped pack must parse");
        for callee in ["join", "sorted", "list", "print", "append", "decode"] {
            let row = pack
                .row(callee)
                .unwrap_or_else(|| panic!("`{callee}` must be recorded as a refusal, not omitted"));
            assert_ne!(
                row.cost,
                CostClass::Constant,
                "`{callee}`'s cost is a function of its argument's size"
            );
            assert!(
                pack.signature(callee, |_| false).is_none(),
                "`{callee}` must not be resolvable"
            );
        }
    }

    #[test]
    fn a_length_relation_is_a_separate_claim_from_a_cost() {
        let pack = SignaturePack::builtin().expect("the shipped pack must parse");
        assert!(
            pack.signature("sorted", |_| false).is_none(),
            "`sorted` costs n log n and no frontend may treat the call as discharged"
        );
        assert_eq!(
            pack.length_relation("sorted", |_| false),
            Some(ResultLength::ExactlyArgument0),
            "and its result still holds exactly as many values as its argument"
        );
    }

    #[test]
    fn a_set_declares_an_upper_bound_and_never_an_equality() {
        let pack = SignaturePack::builtin().expect("the shipped pack must parse");
        for callee in ["set", "frozenset"] {
            assert_eq!(
                pack.length_relation(callee, |_| false),
                Some(ResultLength::AtMostArgument0),
                "`{callee}` collapses equal elements, so its length is an inequality"
            );
        }
    }

    #[test]
    fn a_row_declaring_no_length_confers_none() {
        let pack = SignaturePack::builtin().expect("the shipped pack must parse");
        for callee in ["isinstance", "join", "print", "append"] {
            assert_eq!(
                pack.length_relation(callee, |_| false),
                None,
                "`{callee}` says nothing about its result's length, so it confers none"
            );
        }
        assert_eq!(pack.length_relation("mystery", |_| false), None);
    }

    #[test]
    fn a_shadowed_name_gets_no_length_relation() {
        let pack = SignaturePack::builtin().expect("the shipped pack must parse");
        assert!(pack.length_relation("sorted", |_| false).is_some());
        assert!(
            pack.length_relation("sorted", |name| name == "sorted")
                .is_none(),
            "a module that binds `sorted` is not calling this one"
        );
    }

    #[test]
    fn a_shadowed_name_gets_no_signature() {
        let pack = SignaturePack::builtin().expect("the shipped pack must parse");
        assert!(pack.signature("isinstance", |_| false).is_some());
        assert!(
            pack.signature("isinstance", |name| name == "isinstance")
                .is_none(),
            "a module that binds `isinstance` is not calling this one"
        );
    }

    #[test]
    fn a_row_without_a_reason_is_refused() {
        let text = "\
[[signature]]
callee = \"isinstance\"
cost = \"constant\"
rebinds_locals = false
mutates_arguments = true
why = \"\"
";
        assert_eq!(
            SignaturePack::parse(&PackOrigin::Builtin, text),
            Err(PackError::UnexplainedRow {
                callee: "isinstance".to_owned()
            })
        );
    }

    #[test]
    fn two_rows_for_one_callee_are_refused() {
        let text = "\
[[signature]]
callee = \"isinstance\"
cost = \"constant\"
rebinds_locals = false
mutates_arguments = true
why = \"a class hierarchy walk, fixed at import time\"

[[signature]]
callee = \"isinstance\"
cost = \"unbounded\"
rebinds_locals = true
mutates_arguments = true
why = \"appended rather than argued with\"
";
        assert_eq!(
            SignaturePack::parse(&PackOrigin::Builtin, text),
            Err(PackError::Duplicate {
                callee: "isinstance".to_owned()
            })
        );
    }

    #[test]
    fn an_unknown_field_is_refused() {
        let text = "\
[[signature]]
callee = \"isinstance\"
cost = \"constant\"
rebinds_locals = false
mutates_arguments = true
why = \"a class hierarchy walk, fixed at import time\"
blocks_forever = false
";
        assert!(matches!(
            SignaturePack::parse(&PackOrigin::Builtin, text),
            Err(PackError::Malformed { .. })
        ));
    }

    /// One row, valid, used by the format tests below.
    fn one_row() -> String {
        "\
[[signature]]
callee = \"isinstance\"
cost = \"constant\"
rebinds_locals = false
mutates_arguments = true
why = \"a class hierarchy walk, fixed at import time\"
"
        .to_owned()
    }

    #[test]
    fn a_pack_with_no_format_key_is_the_current_format() {
        assert!(SignaturePack::parse(&PackOrigin::Builtin, &one_row()).is_ok());
    }

    #[test]
    fn a_pack_declaring_the_current_format_parses() {
        let text = format!("format = {PACK_FORMAT}\n{}", one_row());
        assert!(SignaturePack::parse(&PackOrigin::Builtin, &text).is_ok());
    }

    #[test]
    fn a_pack_from_a_newer_landav_is_refused_by_number() {
        let text = format!("format = {}\n{}", PACK_FORMAT + 1, one_row());
        assert_eq!(
            SignaturePack::parse(&PackOrigin::Builtin, &text),
            Err(PackError::UnsupportedFormat {
                found: PACK_FORMAT + 1,
                supported: PACK_FORMAT,
            })
        );
    }

    /// **The reason the version is read in its own pass.**
    ///
    /// A pack from a newer landav does not merely carry a bigger number — it
    /// carries the fields that number was raised for. Deserialising it
    /// strictly fails on one of those fields, and `unknown field
    /// blocks_forever` tells the reader their pack is broken when what is
    /// actually true is that their landav is old. This asserts the version
    /// wins, which only holds while the probe runs first.
    #[test]
    fn a_future_pack_is_refused_by_version_and_not_by_its_unknown_fields() {
        let text = format!(
            "format = {}\n{}blocks_forever = false\n",
            PACK_FORMAT + 1,
            one_row()
        );
        assert_eq!(
            SignaturePack::parse(&PackOrigin::Builtin, &text),
            Err(PackError::UnsupportedFormat {
                found: PACK_FORMAT + 1,
                supported: PACK_FORMAT,
            }),
            "the unknown field was reported instead of the version, so the \
             operator is told to edit a pack that is correct"
        );
    }

    /// The shipped pack states its format rather than relying on the default.
    ///
    /// The builtin is the worked example every other pack is written from, and
    /// a key that the one pack in the repository never uses is a key nobody
    /// copies.
    #[test]
    fn the_builtin_pack_declares_its_format() {
        let probe: FormatProbe = toml::from_str(BUILTIN).expect("the builtin pack is TOML");
        assert_eq!(probe.format, Some(PACK_FORMAT));
    }

    // -----------------------------------------------------------------------
    // provenance and overlay
    // -----------------------------------------------------------------------

    /// A pack read from `name`, with one row for `callee` at `cost`.
    fn pack_from(name: &str, callee: &str, cost: &str, why: &str) -> SignaturePack {
        let text = format!(
            "[[signature]]\ncallee = \"{callee}\"\ncost = \"{cost}\"\n\
             rebinds_locals = false\nmutates_arguments = true\nwhy = \"{why}\"\n"
        );
        SignaturePack::parse(&PackOrigin::File(PathBuf::from(name)), &text)
            .expect("the test pack parses")
    }

    #[test]
    fn a_row_knows_which_pack_it_came_from() {
        let pack = pack_from("team.toml", "frobnicate", "constant", "measured here");
        assert_eq!(
            pack.origin_of("frobnicate"),
            Some(&PackOrigin::File(PathBuf::from("team.toml")))
        );
        assert_eq!(pack.origin_of("never_declared"), None);
        assert!(
            !pack
                .origin_of("frobnicate")
                .expect("the row is present")
                .is_builtin(),
            "a row somebody supplied must not read as a row landav shipped"
        );
    }

    #[test]
    fn a_pack_that_conflicts_with_nothing_shadows_nothing() {
        let mut pack = SignaturePack::builtin().expect("the builtin pack parses");
        pack.overlay(pack_from("team.toml", "frobnicate", "constant", "ours"));
        assert!(pack.shadowed().is_empty());
        assert!(pack.row("frobnicate").is_some(), "the new row is usable");
        assert!(
            pack.row("isinstance").is_some(),
            "an overlay is by row, not by pack: the builtin rows are still here"
        );
    }

    /// **An override is allowed, and the row it replaced is kept whole.**
    ///
    /// The loosening direction on purpose, because it is the dangerous one:
    /// `sorted` is `loglinear` and unresolvable in the builtin, and a
    /// deployment declaring it constant would make every sort inside a loop
    /// report a bound the program exceeds. That is permitted — overriding is
    /// the reason to supply a pack — and it is permitted *because* it cannot
    /// be done quietly. The losing row survives with the argument it made.
    #[test]
    fn an_overridden_row_is_recorded_with_the_argument_it_lost() {
        let mut pack = SignaturePack::builtin().expect("the builtin pack parses");
        let before = pack.row("sorted").expect("the builtin declares sorted").clone();

        pack.overlay(pack_from("team.toml", "sorted", "constant", "we sort small lists"));

        assert_eq!(
            pack.row("sorted").map(|row| row.cost),
            Some(CostClass::Constant),
            "the later row is the one in force"
        );
        assert_eq!(
            pack.origin_of("sorted"),
            Some(&PackOrigin::File(PathBuf::from("team.toml"))),
            "and the pack knows it is no longer answering for the builtin"
        );

        let records = pack.shadowed();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].callee, "sorted");
        assert!(records[0].overrode_the_builtin());
        assert_eq!(
            records[0].replaced, before,
            "the row that lost is kept entire, `why` included, or the \
             disagreement the duplicate rule exists to preserve is gone"
        );
    }

    #[test]
    fn every_override_in_a_chain_is_reported_and_not_only_the_last() {
        let mut pack = SignaturePack::builtin().expect("the builtin pack parses");
        pack.overlay(pack_from("team.toml", "sorted", "constant", "first word"));
        pack.overlay(pack_from("local.toml", "sorted", "unbounded", "last word"));

        let callees: Vec<&str> = pack
            .shadowed()
            .iter()
            .map(|record| record.callee.as_str())
            .collect();
        assert_eq!(
            callees,
            vec!["sorted", "sorted"],
            "the middle override vanished, so a reader sees the final row \
             disagreeing with the builtin and not the pack that came between"
        );
        assert!(pack.shadowed()[0].overrode_the_builtin());
        assert!(!pack.shadowed()[1].overrode_the_builtin());
    }

    /// An overlaid pack's own records come along with it.
    ///
    /// Otherwise a driver that composes packs before handing one over reports
    /// only the overrides it performed itself.
    #[test]
    fn an_overlays_own_records_survive_being_overlaid() {
        let mut supplied = SignaturePack::builtin().expect("the builtin pack parses");
        supplied.overlay(pack_from("team.toml", "sorted", "constant", "first"));
        assert_eq!(supplied.shadowed().len(), 1);

        let mut pack = SignaturePack::default();
        pack.overlay(supplied);
        assert_eq!(
            pack.shadowed().len(),
            1,
            "the record made before the merge was dropped by the merge"
        );
    }
}
