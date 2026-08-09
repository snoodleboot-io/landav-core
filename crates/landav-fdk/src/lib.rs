//! The Landav Frontend Development Kit — **the boundary crate**.
//!
//! # Scope
//!
//! Component `C-33`. Features [`F-043`] (interface) and [`F-044`] (conformance
//! suite), release R3, milestone M2.
//!
//! # This crate is the OSS/EE seam
//!
//! Everything here is `Edition/Boundary` in Linear: it ships in `landav-core`
//! under Apache-2.0, but it is also the interface `landav-ee` plugs into.
//! Changes are breaking-change reviewed against **both** repositories.
//!
//! Two families of extension use the same mechanism, deliberately:
//!
//! | Extension | Implemented by | Edition |
//! |---|---|---|
//! | Language frontend | `landav-python`, TypeScript (R4), JVM (R6) | OSS |
//! | Signature pack | stdlib pack (OSS); pandas/numpy pack (EE, F-032) | mixed |
//! | Calibration profile | self-calibrated (OSS); curated matrix (EE, E-003) | mixed |
//! | Size envelope provider | declaration/schema/config (OSS); telemetry (EE, E-004) | mixed |
//! | Policy resolver | local file (OSS); org governance (EE, E-001) | mixed |
//!
//! Designing one plugin boundary rather than two is the point. If the EE
//! extensions ever need a second mechanism, this crate is wrong.
//!
//! # Hard constraint: no entitlement logic here
//!
//! `landav-core` contains **no licence checks and no entitlement logic** — not
//! stubbed, not feature-gated, absent. An Apache-2.0 repository with a licence
//! check in it invites exactly one kind of fork, and the check is trivially
//! patchable anyway, so it buys nothing and costs credibility.
//!
//! This crate knows how to *load a pack*. It does not know, and must never
//! learn, whether something decided the user was allowed to have it. See
//! [`E-002`].
//!
//! # Packs are data, not code
//!
//! Signature packs and calibration profiles are runtime-discovered data, never
//! compiled in. That decision is what keeps the OSS/EE split reversible — see
//! the flag on [`F-032`], which may yet move from EE to a baseline-in-OSS split.
//!
//! [`F-043`]: https://linear.app/snoodleboot/issue/LAN-25
//! [`F-044`]: https://linear.app/snoodleboot/issue/LAN-29
//! [`F-032`]: https://linear.app/snoodleboot/issue/LAN-36
//! [`E-002`]: https://linear.app/snoodleboot/issue/LAN-51

#![doc(html_root_url = "https://docs.rs/landav-fdk")]

// TODO(LAN-25): The six trait boundaries a language plugin implements — parse,
// resolve types, lower to Core IR, supply a signature pack, supply a
// calibration profile, declare framework entry points — plus the plugin
// registry and the versioning policy.
//
// TODO(LAN-29): The language-agnostic conformance corpus, expressed as
// source-plus-expected-bound pairs, that any frontend must pass to be
// considered supported.
//
// The traits below are scheduled for R3, but the *shape* is fixed now because
// R0-R2 code will be written against it and retrofitting is what this whole
// feature exists to avoid.

/// Placeholder so the workspace builds before `LAN-25` lands.
///
/// Replace with the six FDK traits in the first story of F-043.
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
