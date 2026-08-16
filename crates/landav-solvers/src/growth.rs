//! [`Growth`] - the asymptotic class an answer belongs to.

/// The asymptotic class of a runtime, ordered by growth.
///
/// # Why a class and not just a bound
///
/// KoAT prints a symbolic expression *and* a class: `Arg_2^2+5*Arg_2+3
/// {O(n^2)}`. LoAT prints a class and nothing else: `WORST_CASE(Omega(n^2),?)`.
/// The class is therefore the only vocabulary the two share, and comparing an
/// upper answer with a lower one is only possible here.
///
/// It has a second job on the KoAT side, and it is the more important one. The
/// class KoAT announces is an independent statement about the same bound, so
/// the parsed expression can be checked against it for free. A polynomial read
/// one degree short of the class its own solver announced is the signature of
/// a dropped factor - which is an upper bound smaller than the one that was
/// proved. [`crate::koat_answer::parse`] refuses on a mismatch rather than
/// publishing either half.
///
/// # The order is the asymptotic order
///
/// `Ord` is derived, and the variant order below is load bearing: it is what
/// [`crate::Analysis::agreement`] compares to decide whether a lower bound
/// contradicts an upper one. `Constant < Logarithmic < Polynomial(1) <
/// Polynomial(2) < ... < Exponential < Unbounded`.
///
/// `Polynomial(0)` is deliberately unreachable through
/// [`Growth::polynomial`] - `O(n^0)` is `O(1)` - because a degree-zero
/// polynomial would sort *above* `Logarithmic` while denoting something below
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Growth {
    /// `O(1)`.
    Constant,
    /// `O(log n)`.
    Logarithmic,
    /// `O(n^k)` for the carried `k >= 1`.
    Polynomial(u32),
    /// Growth faster than every polynomial, but bounded.
    Exponential,
    /// No bound was established, or the runtime was proved unbounded. The top
    /// of the lattice, and what `omega` denotes.
    Unbounded,
}

impl Growth {
    /// The polynomial class of `degree`, normalising degree zero to
    /// [`Growth::Constant`].
    #[must_use]
    pub const fn polynomial(degree: u32) -> Self {
        if degree == 0 {
            Self::Constant
        } else {
            Self::Polynomial(degree)
        }
    }

    /// The polynomial degree, if this class is a polynomial one.
    ///
    /// [`Growth::Constant`] is degree zero; the sub-polynomial and
    /// super-polynomial classes have no degree.
    #[must_use]
    pub const fn degree(self) -> Option<u32> {
        match self {
            Self::Constant => Some(0),
            Self::Polynomial(degree) => Some(degree),
            Self::Logarithmic | Self::Exponential | Self::Unbounded => None,
        }
    }
}

impl core::fmt::Display for Growth {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Written without an `O(...)` or `Omega(...)` wrapper: which one it is
        // depends on the `Direction` the class arrived from, and this type
        // does not carry one.
        match self {
            Self::Constant => f.write_str("1"),
            Self::Logarithmic => f.write_str("log n"),
            Self::Polynomial(1) => f.write_str("n"),
            Self::Polynomial(degree) => write!(f, "n^{degree}"),
            Self::Exponential => f.write_str("EXP"),
            Self::Unbounded => f.write_str("INF"),
        }
    }
}
