//! [`Hole`] - a region of a program the engine could not analyse.

use landav_bound::{Bound, Origin, Symbol, VarId};

/// The prefix every hole variable carries.
///
/// A `#` cannot appear in a Python identifier, so a hole can never collide with
/// a variable the user wrote. That is asserted rather than assumed - see the
/// tests - because a collision would silently equate a program variable with an
/// unanalysed region, and the substitution that fills the hole would then
/// rewrite the user's variable too.
const PREFIX: &str = "#hole";

/// A named stand-in for the cost of a region the engine could not derive.
///
/// # Why a variable rather than an absence
///
/// Before holes, an unanalysable statement made the whole function's bound
/// `Unknown` - `then` propagated it, so one `while` erased every exact answer
/// around it. The counted loop above it was derived correctly and then thrown
/// away.
///
/// A hole is a *variable*, so the arithmetic keeps working. `for` loop, then a
/// `while`, gives `2n + #hole1` instead of nothing at all. The engine reports
/// what it established and names what it could not.
///
/// # Filling one
///
/// [`Bound::subst`] replaces the hole with a bound obtained elsewhere - the
/// external solver today, a ranking-function engine later. That substitution is
/// sound with no extra argument: [`Bound`] is weakly monotone by construction,
/// so replacing a variable with something that dominates the true cost can only
/// raise the result.
///
/// An **unfilled** hole denotes `omega`. That is the honest reading - nothing is
/// known about the region - and it is why a bound carrying a hole is never
/// reported as a plain equality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hole {
    var: Symbol,
    origin: Origin,
    construct: &'static str,
}

impl Hole {
    /// A hole for `construct` at `origin`, distinguished by `index`.
    ///
    /// The index makes holes within one function distinct. Two `while` loops on
    /// different lines are different regions and must not share a variable, or
    /// filling one would silently fill the other.
    #[must_use]
    pub fn new(index: usize, construct: &'static str, origin: Origin) -> Self {
        Self {
            var: Symbol::from(format!("{PREFIX}{index}")),
            origin,
            construct,
        }
    }

    /// The variable standing for this region's cost.
    #[must_use]
    pub fn var(&self) -> VarId {
        VarId::new(self.var.clone())
    }

    /// This hole as a bound.
    #[must_use]
    pub fn as_bound(&self) -> Bound {
        Bound::var(self.var.clone())
    }

    /// Where the region is.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// What the region was - `while`, a refused construct, and so on.
    ///
    /// This is the blame. "No bound" tells a user nothing they can act on;
    /// "the `while` at line 42" tells them what to change.
    #[must_use]
    pub const fn construct(&self) -> &'static str {
        self.construct
    }

    /// Whether `var` names a hole rather than something the user wrote.
    ///
    /// Consumers that walk a bound's variables need this: a hole is not a
    /// parameter and must not be reported as one, nor supplied a value by a
    /// caller who has no idea it exists.
    #[must_use]
    pub fn is_hole(var: &VarId) -> bool {
        var.symbol().as_str().starts_with(PREFIX)
    }
}

impl core::fmt::Display for Hole {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} ({} at {})", self.var, self.construct, self.origin)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use landav_bound::{Origin, Symbol, VarId};

    use super::{Hole, PREFIX};

    fn at() -> Origin {
        Origin::new("probe.py:42")
    }

    /// The collision that would be silent and wrong.
    ///
    /// A hole variable sharing a name with a program variable would make the
    /// substitution that fills the hole rewrite the user's variable as well.
    /// `#` is not legal in a Python identifier, which is the whole defence, so
    /// it is worth asserting rather than trusting.
    #[test]
    fn a_hole_cannot_be_named_like_a_program_variable() {
        let hole = Hole::new(0, "while", at());
        let name = hole.var().symbol().as_str().to_owned();
        assert!(
            name.starts_with(PREFIX),
            "a hole must live in the reserved namespace, got {name:?}"
        );
        assert!(
            name.contains('#'),
            "the reserved namespace must use a character no identifier can \
             contain, or a program variable could collide with it: {name:?}"
        );
    }

    #[test]
    fn holes_are_told_apart_by_index() {
        let first = Hole::new(0, "while", at());
        let second = Hole::new(1, "while", at());
        assert_ne!(
            first.var(),
            second.var(),
            "two regions must not share a variable, or filling one fills both"
        );
    }

    #[test]
    fn a_hole_is_recognised_and_a_program_variable_is_not() {
        assert!(Hole::is_hole(&Hole::new(3, "call", at()).var()));
        for ordinary in ["n", "i", "hole", "hole1", "x_hole"] {
            assert!(
                !Hole::is_hole(&VarId::new(Symbol::from(ordinary))),
                "`{ordinary}` is a program variable and must not read as a hole"
            );
        }
    }

    /// The blame is the point: a user can act on "the `while` at line 42" and
    /// cannot act on "no bound".
    #[test]
    fn a_hole_carries_what_it_was_and_where() {
        let hole = Hole::new(0, "while", at());
        assert_eq!(hole.construct(), "while");
        assert_eq!(hole.origin().as_str(), "probe.py:42");
        let shown = hole.to_string();
        assert!(shown.contains("while"), "{shown}");
        assert!(shown.contains("probe.py:42"), "{shown}");
    }
}
