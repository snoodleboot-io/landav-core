//! Integer transition system exporter.
//!
//! # Scope
//!
//! Component `C-07`. Feature [`F-006`], release R0, milestone M0.5.
//!
//! # Turning the problem into a lowering
//!
//! Lowers the numeric fragment — integer variables, loops, conditionals, no
//! containers — into KoAT's integer transition system format. This is what
//! turns milestone one from "build a complexity analyser" into "build a
//! lowering", following the Pico precedent of a domain frontend onto KoAT.
//!
//! The ITS export cannot represent containers or heap effects. That is what the
//! Landav IR (`F-009`, R1) is for; this crate deliberately handles only the
//! fragment KoAT can already reason about, which is how R0 produces real bounds
//! within weeks.
//!
//! # Edition
//!
//! OSS — Apache-2.0, ships in `landav-core`.
//!
//! [`F-006`]: https://linear.app/snoodleboot/issue/LAN-6

#![doc(html_root_url = "https://docs.rs/landav-its")]

// TODO(LAN-6): Lower the annotated numeric fragment to KoAT ITS format, with
// the R0 exit criterion being an end-to-end run over 20 hand-written numeric
// functions.

/// Placeholder so the workspace builds before `LAN-6` lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Unimplemented;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_builds() {
        assert_eq!(Unimplemented, Unimplemented);
    }
}
