//! [`Unsupported`] - one construct that was refused, and where it was.

use landav_bound::{Assumption, Blame, Origin, Symbol};

use crate::construct::Construct;

/// One refused construct: what it was, where it was, and any specifics.
///
/// # Why this is not an error string
///
/// `LAN-68` builds a coverage report over these, and a report needs to group
/// by construct and count. A formatted string cannot be grouped without
/// parsing it back, so the [`Construct`] stays a value and the free text is
/// confined to [`Unsupported::detail`], which no consumer is expected to
/// interpret.
///
/// `Ord` is derived and content derived throughout, so [`crate::Refusals`] can
/// keep itself sorted and two runs over identical input produce byte-identical
/// diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unsupported {
    construct: Construct,
    origin: Origin,
    detail: Option<Symbol>,
}

impl Unsupported {
    /// A refusal naming a construct and a position.
    ///
    /// Both arguments are required and neither has a default: a refusal that
    /// does not say *where* sends a reader to grep, which is the failure mode
    /// non-negotiable 3 exists to prevent.
    #[must_use]
    pub fn new(construct: Construct, origin: Origin) -> Self {
        Self {
            construct,
            origin,
            detail: None,
        }
    }

    /// The same refusal, carrying frontend-supplied specifics.
    ///
    /// For the part a [`Construct`] cannot say: which operator, which callee,
    /// which annotation was missing.
    #[must_use]
    pub fn with_detail(construct: Construct, origin: Origin, detail: impl Into<Symbol>) -> Self {
        Self {
            construct,
            origin,
            detail: Some(detail.into()),
        }
    }

    /// What was refused.
    #[must_use]
    pub const fn construct(&self) -> Construct {
        self.construct
    }

    /// Where it was, as the frontend spelled the position.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The frontend-supplied specifics, if any.
    #[must_use]
    pub fn detail(&self) -> Option<&Symbol> {
        self.detail.as_ref()
    }

    /// This refusal as a [`Blame`] record.
    ///
    /// # The `LAN-68` and `F-015` seam
    ///
    /// `F-015` reports a *partial bound*: a sound over-approximation together
    /// with a non-empty ledger of what was not accounted for. This method is
    /// the join between the two vocabularies, and it exists here so that
    /// `F-015` is an extension rather than a retrofit - a caller holding
    /// [`crate::Refusals`] can build a [`landav_bound::Blames`] without this
    /// crate learning what a bound is.
    ///
    /// The assumption is always [`Assumption::ResourceNotModelled`]: a refused
    /// construct is precisely one the cost model has no rule for. It is
    /// deliberately not [`Assumption::TerminationNotProved`], which is a
    /// stronger and different claim - the loop may well terminate, we simply
    /// declined to look.
    #[must_use]
    pub fn blame(&self) -> Blame {
        Blame {
            unaccounted: Symbol::from(self.construct.tag()),
            assumption: Assumption::ResourceNotModelled {
                detail: self
                    .detail
                    .clone()
                    .unwrap_or_else(|| Symbol::from(self.construct.describe())),
            },
            origin: self.origin.clone(),
        }
    }
}

impl core::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}: {} [{}]",
            self.origin,
            self.construct.describe(),
            self.construct.tag()
        )?;
        if let Some(detail) = &self.detail {
            write!(f, ": {detail}")?;
        }
        Ok(())
    }
}
