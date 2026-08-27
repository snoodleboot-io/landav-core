//! [`SignaturePack`] - a table of [`Signature`] rows, and how one is read.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{pack_error::PackError, signature::Signature};

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
    rows: BTreeMap<String, Signature>,
}

/// The on-disk shape: `[[signature]]` tables and nothing else.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    #[serde(default)]
    signature: Vec<Signature>,
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
        Self::parse(BUILTIN)
    }

    /// Reads a pack from TOML.
    ///
    /// # Errors
    ///
    /// [`PackError::Malformed`] if the text is not a pack,
    /// [`PackError::Duplicate`] if two rows claim one callee, and
    /// [`PackError::UnexplainedRow`] if a row's `why` is empty.
    pub fn parse(text: &str) -> Result<Self, PackError> {
        let file: PackFile = toml::from_str(text).map_err(|error| PackError::Malformed {
            reason: error.to_string(),
        })?;
        let mut rows: BTreeMap<String, Signature> = BTreeMap::new();
        for row in file.signature {
            if row.why.trim().is_empty() {
                return Err(PackError::UnexplainedRow { callee: row.callee });
            }
            if rows.contains_key(&row.callee) {
                return Err(PackError::Duplicate { callee: row.callee });
            }
            rows.insert(row.callee.clone(), row);
        }
        Ok(Self { rows })
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
        let row = self.rows.get(callee)?;
        if !row.is_resolvable() || bound_names(callee) {
            return None;
        }
        Some(row)
    }

    /// The row for `callee`, resolvable or not.
    ///
    /// For a reader of the table - a report, a test - rather than for a
    /// frontend deciding what to do with a call site.
    #[must_use]
    pub fn row(&self, callee: &str) -> Option<&Signature> {
        self.rows.get(callee)
    }

    /// Every row, in callee order.
    pub fn rows(&self) -> impl Iterator<Item = &Signature> {
        self.rows.values()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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
            SignaturePack::parse(text),
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
            SignaturePack::parse(text),
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
            SignaturePack::parse(text),
            Err(PackError::Malformed { .. })
        ));
    }
}
