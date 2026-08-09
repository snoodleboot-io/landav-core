//! External solver bridge — KoAT (upper bounds) and LoAT (lower bounds).
//!
//! # Scope
//!
//! Component `C-13`. Features [`F-007`] (bridge, R0/M0.5) and [`F-040`]
//! (evaluation corpus and benchmark harness, R1/M1).
//!
//! # What this buys
//!
//! Real upper *and* lower bounds within weeks of project start. The LoAT
//! pairing is not incidental: it is the source of the lower bounds the
//! differential engine needs at R3, and reporting tightness when the two meet
//! is the honest version of a tightness claim.
//!
//! # ⚠️ Licence diligence required before the design is fixed
//!
//! KoAT and LoAT are third-party solvers being invoked from an Apache-2.0
//! repository. Confirm their licences permit the intended distribution model
//! **before** deciding whether they are vendored, bundled or discovered on
//! `PATH`. Shelling out to a separately-installed binary is the safest default;
//! bundling is the one that needs the legal answer first. See [`F-007`].
//!
//! # The ceiling this sits under
//!
//! KoAT — state of the art, from a specialist group, after a decade — solves
//! 548 of 838 curated *integer-only* benchmarks. Roughly 65%, on the easiest
//! possible input class. That number is why contract *checking* (`F-014`) ships
//! alongside the first inference engine rather than after it: a product resting
//! solely on inference coverage would be waiting on a number that does not move
//! quickly.
//!
//! F-023 (ranking function synthesis, R2) begins reducing this dependence.
//!
//! # Edition
//!
//! OSS — Apache-2.0, ships in `landav-core`. The benchmark numbers must be
//! publicly reproducible or they are worth nothing.
//!
//! [`F-007`]: https://linear.app/snoodleboot/issue/LAN-7
//! [`F-040`]: https://linear.app/snoodleboot/issue/LAN-16

#![doc(html_root_url = "https://docs.rs/landav-solvers")]

// TODO(LAN-7): Invoke KoAT and LoAT over an exported ITS, parse output back
// into the bound algebra, report tightness when upper and lower bounds meet.
//
// TODO(LAN-16): Curated corpora with a repeatable harness reporting coverage,
// soundness and tightness as three SEPARATE metrics. Soundness target is zero;
// a reported bound the code can exceed stops the line.

/// Placeholder so the workspace builds before `LAN-7` lands.
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
