//! The `landav` command-line interface.
//!
//! # Scope
//!
//! Component `C-26`. Features [`F-004`] (skeleton, R0/M0) and [`F-041`]
//! (documentation and adoption guide, R2/M1.5).
//!
//! # The primary surface through R2
//!
//! Subcommands `check`, `explain`, `calibrate` and `version`; configuration
//! discovery via `pyproject.toml`; consistent exit codes. Everything else in
//! the product is reachable through this binary.
//!
//! # Free forever
//!
//! GTM P3 pricing is explicitly "Free CLI, paid team features" and "Free for
//! OSS". The CLI, the analysis it drives and the CI integrations that wrap it
//! all ship Apache-2.0 in `landav-core`. What is paid is org-level governance
//! and the hosted platform — never the verdict itself.
//!
//! # Naming
//!
//! The delivery workbook says `pycost`, the pre-rename working title. The
//! binary ships as `landav`.
//!
//! [`F-004`]: https://linear.app/snoodleboot/issue/LAN-3
//! [`F-041`]: https://linear.app/snoodleboot/issue/LAN-24

// TODO(LAN-3): clap-based subcommands, pyproject.toml discovery, and the exit
// code contract below.

/// Exit codes. These are a public interface — CI integrations and the EE
/// platform both branch on them, so changing one is a breaking change.
// Fixed here rather than alongside the dispatch so that R0 code written against
// the CLI has a stable contract to target. Wired up by LAN-3.
#[allow(dead_code, reason = "contract fixed ahead of the dispatch in LAN-3")]
mod exit {
    /// Analysis ran and every checked bound held.
    pub const OK: i32 = 0;
    /// Analysis ran and at least one bound or contract was violated.
    pub const VIOLATION: i32 = 1;
    /// Analysis could not run: bad configuration, unreadable source, missing
    /// solver. Distinct from `VIOLATION` so CI can tell "we found a problem"
    /// from "we could not look".
    pub const ERROR: i32 = 2;
}

fn main() {
    // TODO(LAN-3): replace with the real dispatch.
    println!("landav {} (scaffold)", env!("CARGO_PKG_VERSION"));
    std::process::exit(exit::OK);
}
