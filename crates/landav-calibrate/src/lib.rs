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
//! # What is waiting on this crate, and what is not
//!
//! Exactly one registered `--resource` is: **`ops`**. It is the only one of the
//! four that is a *scaling* of a number the engine already derives, so it is
//! the only one whose blocker is a measurement rather than an analysis. The CLI
//! says so in its own words - `landav check --help` names this component under
//! `--resource ops`, and a run that selects `ops` reports that it is waiting on
//! the profile this crate emits. That sentence is generated from
//! `landav-cli`'s `resource::awaiting`, which is the single place any statement
//! about a resource's status is made.
//!
//! The other three are not waiting on a profile and must not be told they are:
//! `alloc` and `peak-mem` need an IR that can represent data - nothing in an
//! integer-scalar fragment allocates - and `queries` already derives, because a
//! call became a named hole rather than a refusal. A blanket "calibration is
//! not implemented" would be true of one resource and misleading about three,
//! which is the drift LAN-86 was filed to end.
//!
//! # The shape of the edge, so it is not mistaken for a stub
//!
//! `landav-cli` declares this crate as a dependency and calls nothing in it
//! yet. That edge is real and deliberate rather than decorative: it is where
//! the profile *loader* is consumed the moment `LAN-5` lands, and the CLI
//! already carries the sentence saying what it is for. What arrives here is a
//! benchmark harness, a versioned profile format, and a loader - and the
//! contract that **every concrete estimate names the calibration id it used**,
//! so that a stale profile is detectable in the report rather than silently
//! trusted.
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
