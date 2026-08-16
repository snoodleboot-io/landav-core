//! [`Direction`] - which side of the runtime a solver bounds.

/// Which side of a program's runtime a solver's answer lies on.
///
/// A property of the **solver**, never of its output. Nothing KoAT or LoAT
/// prints says which direction it bounds in: `Arg_0+2` and `Omega(n^1)` are
/// shapes, not promises. So the direction is declared once per solver in
/// [`crate::Solver::direction`] and read nowhere else.
///
/// The reason it is a type rather than a `bool` is what happens when the two
/// are confused. A lower bound reported as an upper bound is, by construction,
/// a bound the program exceeds - the single failure class with a zero target.
/// [`crate::Analysis::new`] refuses a report filed under the wrong direction
/// rather than trusting call sites to keep them straight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    /// The solver bounds the runtime from above. KoAT.
    Upper,
    /// The solver bounds the runtime from below. LoAT.
    Lower,
}

impl Direction {
    /// The direction as a word for a report.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Upper => "upper",
            Self::Lower => "lower",
        }
    }
}

impl core::fmt::Display for Direction {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
