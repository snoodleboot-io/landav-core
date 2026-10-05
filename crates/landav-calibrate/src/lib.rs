//! Calibration harness, profile format and profile loader.
//!
//! # Scope
//!
//! Component `C-03`. Features [`F-003`] (harness, R0/M0.5) and [`F-017`]
//! (concrete cost estimation, R2/M1.5).
//!
//! # Why calibration comes first
//!
//! A tick is not a fixed unit any more. The runtime a number is calibrated
//! against varies more than `major.minor` can name: under free-threading
//! `PyObject` grew from 16 bytes to roughly 32, and the JIT, the GC and the
//! interpreter loop are all build- and patch-level choices (`LAN-74`). A
//! bytecode-op count was never a uniform unit — one `numpy` call is one op and a
//! billion floating-point operations — but it is now non-uniform *dynamically*
//! as well as statically.
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
//! # A profile's identity is a tuple, not a version (`LAN-74`)
//!
//! The build plan's CPython 3.14 claims were checked against primary sources and
//! several were wrong, all in the same direction — the runtime is *less* uniform
//! than assumed. The JIT is opt-in (PEP 744 is Draft, configure default `no`)
//! and in 3.13/3.14 was often *slower* than the interpreter; the circulating
//! 5–12% wins are 3.15. Free-threading's single-threaded overhead has three
//! different official numbers and is platform-dependent, not a constant. The
//! incremental GC shipped in 3.14.0–3.14.4 and was **reverted in 3.14.5**, so
//! two machines both running "3.14" can differ in GC behaviour by patch version
//! alone — which the peak-memory semiring feels directly.
//!
//! So a profile is keyed on a tuple and the loader **hard-refuses a mismatch**
//! rather than near-matching: implementation; **exact patch version**, not
//! `major.minor` (the GC revert is the proof); platform and architecture; GIL
//! build *and* runtime state, sampled *after* imports (a C extension can
//! re-enable the GIL); JIT available *and* enabled; and a build-configuration
//! hash to catch what has no runtime probe (the tail-call interpreter is
//! invisible in `sys.version_info`). The hash cannot say *which* flag differs,
//! but it refuses to pretend two builds are the same. Refuse rather than warn:
//! a silently near-matched profile is how a calibrated estimate becomes a
//! confidently wrong number (`LAN-63` reports the mismatch). The full record,
//! with sources, is `LAN-74`.
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
//! [`LAN-74`]: https://linear.app/snoodleboot/issue/LAN-74
//! [`LAN-63`]: https://linear.app/snoodleboot/issue/LAN-63

#![doc(html_root_url = "https://docs.rs/landav-calibrate")]

// TODO(LAN-5): Benchmark suite over interpreter ops, allocation and a library
// operation panel; signed, versioned profile emission keyed by the `LAN-74`
// identity tuple (implementation, exact patch version, platform/arch, GIL build
// and runtime state, JIT available and enabled, build-config hash) — not by
// `major.minor`; profile format and loader as a public, versioned interface
// whose loader hard-refuses a mismatched tuple.
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
