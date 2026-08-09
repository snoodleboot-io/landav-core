//! Calibration harness, profile format and profile loader.
//!
//! # Scope
//!
//! Component `C-03`. Features [`F-003`] (harness, R0/M0.5) and [`F-017`]
//! (concrete cost estimation, R2/M1.5).
//!
//! # Why calibration comes first
//!
//! A tick is not a fixed unit any more. In CPython 3.14 the JIT is the default
//! code path on supported platforms, and under free-threading `PyObject` grew
//! from 16 bytes to roughly 32. A bytecode-op count was never a uniform unit —
//! one `numpy` call is one op and a billion floating-point operations — but it
//! is now non-uniform *dynamically* as well as statically.
//!
//! Every concrete number the product reports depends on a calibration. Building
//! the analysis first and calibrating later would mean months of numbers nobody
//! should have believed.
//!
//! **Every estimate names the calibration id it used.** That is a hard
//! requirement: it is what makes the number auditable and what lets a stale
//! profile be detected rather than silently trusted.
//!
//! # ⚠️ Verify before designing
//!
//! The CPython 3.14 JIT and free-threading characteristics above come from 2026
//! secondary reporting cited in the build plan. Confirm them against CPython
//! release notes before the harness is designed — the whole calibration
//! argument rests on them being accurate.
//!
//! # Edition
//!
//! **Boundary.** The harness, the profile *format* and the *loader* are all
//! OSS: an OSS user must be able to calibrate their own machine and get honest
//! numbers, or R2's concrete-estimate promise is hollow.
//!
//! What is EE is the *distribution* of a curated, maintained profile matrix
//! plus its freshness SLA — see [`E-003`]. So the profile format is a
//! published, versioned interface from day one, not an internal detail.
//!
//! [`F-003`]: https://linear.app/snoodleboot/issue/LAN-5
//! [`F-017`]: https://linear.app/snoodleboot/issue/LAN-18
//! [`E-003`]: https://linear.app/snoodleboot/issue/LAN-52

#![doc(html_root_url = "https://docs.rs/landav-calibrate")]

// TODO(LAN-5): Benchmark suite over interpreter ops, allocation and a library
// operation panel; signed, versioned profile emission keyed by (CPython
// version, build flags, platform); profile format and loader as a public,
// versioned interface.
//
// TODO(LAN-18): Combine a derived bound, a size envelope and a named
// calibration into a concrete time and memory estimate with an explicit
// uncertainty band, naming every input it depended on.

/// Placeholder so the workspace builds before `LAN-5` lands.
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
