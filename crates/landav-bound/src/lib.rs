//! The Landav bound algebra and cost semiring registry.
//!
//! # Scope
//!
//! Components `C-01` (Bound Algebra) and `C-02` (Cost Semiring Registry).
//! Features [`F-001`] and [`F-002`], release R0, milestone M0.
//!
//! # Why this crate exists first
//!
//! Every analysis tier emits into this vocabulary. The algebra is closed and
//! total: naturals with omega, variables, and the closure operators `+`, `max`,
//! `*`, `p^b` and `log_k`. Weak monotonicity is enforced *by construction*, so
//! that composition-by-substitution is always sound.
//!
//! Getting this wrong invalidates everything downstream, which is why it is the
//! first thing built and the reason M0 is called "Vocabulary".
//!
//! # Edition
//!
//! OSS — Apache-2.0, ships in `landav-core`.
//!
//! [`F-001`]: https://linear.app/snoodleboot/issue/LAN-1
//! [`F-002`]: https://linear.app/snoodleboot/issue/LAN-2

#![doc(html_root_url = "https://docs.rs/landav-bound")]

// TODO(LAN-1): Bound enum with the six constructors, smart constructors that
// preserve weak monotonicity, evaluation at a state, substitution and
// composition, and egg-based normalisation.
//
// TODO(LAN-2): Dioid trait (zero, one, plus, times, star) with the propagation
// engine generic over it. Additive resources instantiate (+, *); peak live
// memory instantiates (max, +).

/// Placeholder so the workspace builds before `LAN-1` lands.
///
/// Replace with the real `Bound` enum in the first story of F-001.
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
