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
//! the product is reachable through this binary. At M0 only `check` exists.
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
//! binary ships as `landav`, and so does the configuration section.
//!
//! # The exit code is decided in exactly one place
//!
//! [`crate::outcome::Outcome::exit_code`] is the only function in this crate
//! that produces an [`ExitCode`], and this is the only place it is converted
//! into a process status. Nothing calls `std::process::exit`, so every path
//! out of the program runs the same conversion and no path can invent a fourth
//! code. See `crate::outcome` for why the mapping is an exhaustive match.
//!
//! [`F-004`]: https://linear.app/snoodleboot/issue/LAN-3
//! [`F-041`]: https://linear.app/snoodleboot/issue/LAN-24

mod check;
mod cli;
mod config;
mod diagnostic;
mod outcome;
mod resource;
mod sources;

use landav_bound::ExitCode;

/// The status byte for "the tool could not complete", and the largest code the
/// contract permits.
///
/// It is named rather than written inline so that the three arms of
/// [`as_status`] read as the table they are. There is deliberately **no**
/// wildcard arm falling back to it: [`as_status`] matches every [`ExitCode`]
/// variant explicitly, so a fourth variant is a compile error until somebody
/// decides which status byte it earns. A `_ => TOOL_ERROR_STATUS` would turn
/// that compile error into a green build — the same argument `crate::outcome`
/// makes one level up, where the wrong default would be `0` instead of `2`.
const TOOL_ERROR_STATUS: u8 = 2;

fn main() -> std::process::ExitCode {
    let code = cli::dispatch().exit_code();
    std::process::ExitCode::from(as_status(code))
}

/// The process status byte for `code`.
///
/// `main` returns [`std::process::ExitCode`] rather than calling
/// `std::process::exit`, so that buffered output is flushed before the process
/// ends. A report that never reached stderr is the same as no report.
const fn as_status(code: ExitCode) -> u8 {
    match code {
        ExitCode::Clean => 0,
        ExitCode::Findings => 1,
        ExitCode::ToolError => TOOL_ERROR_STATUS,
    }
}

#[cfg(test)]
mod tests {
    use super::as_status;
    use landav_bound::ExitCode;

    /// The frozen contract, restated against the process status byte. These
    /// three integers are a public interface: CI integrations and the EE
    /// platform both branch on them, so changing one is a breaking change.
    #[test]
    fn the_status_bytes_are_the_frozen_contract() {
        assert_eq!(as_status(ExitCode::Clean), 0);
        assert_eq!(as_status(ExitCode::Findings), 1);
        assert_eq!(as_status(ExitCode::ToolError), 2);
    }

    /// The discriminants on the shared enum must not drift away from the
    /// status bytes this binary hands back.
    #[test]
    fn the_status_bytes_agree_with_the_shared_enum() {
        for code in [ExitCode::Clean, ExitCode::Findings, ExitCode::ToolError] {
            assert_eq!(i32::from(as_status(code)), code.as_i32(), "{code:?}");
        }
    }
}
