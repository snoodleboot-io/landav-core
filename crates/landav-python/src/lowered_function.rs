//! [`LoweredFunction`] - one Python function, translated into the numeric
//! fragment.

use landav_its::SourceProgram;

use crate::location::Location;

/// One `def`, translated into the language-neutral numeric fragment.
///
/// Holding the [`SourceProgram`] rather than an
/// [`landav_its::Its`] is deliberate. Translation and lowering are different
/// steps that fail differently: translation fails only if the *file* cannot be
/// read, whereas lowering fails whenever the function uses something outside
/// the fragment. Fusing them would make "this file has a syntax error" and
/// "this function calls `sorted`" the same kind of failure, and they need
/// different responses.
///
/// So this crate answers "what does this Python function look like in the
/// numeric fragment", and [`landav_its::lower`] answers "can that be turned
/// into a transition system". A caller runs the second on the first.
#[derive(Debug, Clone)]
pub struct LoweredFunction {
    name: String,
    location: Location,
    program: SourceProgram,
}

impl LoweredFunction {
    /// Pairs a name and position with the translated program.
    #[must_use]
    pub const fn new(name: String, location: Location, program: SourceProgram) -> Self {
        Self {
            name,
            location,
            program,
        }
    }

    /// The function's name, as written.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where the `def` is.
    #[must_use]
    pub const fn location(&self) -> &Location {
        &self.location
    }

    /// The translated program, ready for [`landav_its::lower`].
    #[must_use]
    pub const fn program(&self) -> &SourceProgram {
        &self.program
    }

    /// The translated program, by value.
    #[must_use]
    pub fn into_program(self) -> SourceProgram {
        self.program
    }
}
