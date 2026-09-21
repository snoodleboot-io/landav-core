//! [`Shadowed`] - a row one pack overrode in another.

use crate::{pack_origin::PackOrigin, signature::Signature};

/// A row that a later pack replaced, kept whole.
///
/// # Why an override is recorded rather than merely performed
///
/// Within one pack, two rows for one callee are a [`crate::PackError::Duplicate`]
/// and the pack is refused: two rows for one name means somebody appended
/// instead of editing, and the two `why` fields are exactly the disagreement a
/// silent last-wins would hide.
///
/// That argument does not carry *across* packs. Overriding the builtin is the
/// whole point of supplying a pack — a deployment that knows its own `foo` is
/// constant should be able to say so, and a refusal would mean it could not.
/// So an override is allowed.
///
/// What does carry across is the reason the duplicate rule exists: the
/// disagreement must not vanish. A row that loosened `sorted` from
/// `loglinear` to `constant` would make every sort inside a loop report a
/// bound the program exceeds, and that is not a thing anybody should be able
/// to do invisibly. So the losing row is kept **whole**, `why` included, and
/// the pair of origins says who overrode whom. A driver reports these for the
/// same reason it reports a suppression that suppressed nothing: a claim
/// nobody can see is a claim nobody can review.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Shadowed {
    /// The callee both rows claim.
    pub callee: String,
    /// Where the row that lost came from.
    pub overridden: PackOrigin,
    /// Where the row that won came from.
    pub overridden_by: PackOrigin,
    /// The losing row, entire — including the `why` it argued.
    pub replaced: Signature,
}

impl Shadowed {
    /// Whether this override replaced a row of the shipped pack.
    ///
    /// The case worth a reader's attention: a deployment disagreeing with the
    /// builtin is a deliberate act, and one the shipped pack's `why` argued
    /// against in writing.
    #[must_use]
    pub const fn overrode_the_builtin(&self) -> bool {
        self.overridden.is_builtin()
    }
}
