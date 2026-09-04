//! [`UnsupportedNode`] - one refusal node found by scanning a program's
//! arenas, with the handle that names it.

use landav_bound::{Origin, Symbol};

use crate::{
    construct::Construct, declared_effect::DeclaredEffect, node_id::NodeId,
    unsupported::Unsupported, writes::Writes,
};

/// An `Unsupported` node in a [`crate::SourceProgram`]: what it refuses, where
/// it is, and **which node it is**.
///
/// # Why this is not [`Unsupported`]
///
/// [`Unsupported`] is a *diagnostic* record: it is sorted and deduplicated into
/// [`crate::Refusals`], because a report wants one line per distinct refusal.
/// That is exactly the wrong shape for a consumer that has to account for every
/// node individually - two calls on the same line dedup into one record and one
/// of them silently stops needing to be charged.
///
/// So this carries the [`NodeId`] and nothing deduplicates. [`Self::refusal`]
/// converts to the diagnostic form when that is what is wanted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnsupportedNode {
    id: NodeId,
    construct: Construct,
    origin: Origin,
    detail: Option<Symbol>,
    declared: Option<DeclaredEffect>,
    writes: Writes,
}

impl UnsupportedNode {
    /// A record of the node `id`, refusing `construct` at `origin`.
    #[must_use]
    pub const fn new(
        id: NodeId,
        construct: Construct,
        origin: Origin,
        detail: Option<Symbol>,
    ) -> Self {
        Self {
            id,
            construct,
            origin,
            detail,
            declared: None,
            writes: Writes::unstated(),
        }
    }

    /// The same record, carrying what the frontend declared about the node.
    ///
    /// A node with a declaration is **not** a refusal - see [`DeclaredEffect`].
    /// It is yielded by the scan all the same, because a consumer that charges
    /// every node it finds must still find this one: dropping it here would let
    /// a declared node go uncharged and unreconciled, which is the same silent
    /// omission the scan exists to make impossible.
    #[must_use]
    pub fn declaring(mut self, declared: Option<DeclaredEffect>) -> Self {
        self.declared = declared;
        self
    }

    /// What the frontend declared about this node, if anything.
    #[must_use]
    pub const fn declared(&self) -> Option<DeclaredEffect> {
        self.declared
    }

    /// The same record, carrying which locals the node may rebind.
    ///
    /// Carried for the same reason the declaration is: a frontend that
    /// translates into a scratch program and hoists the refusals across has to
    /// be able to hoist this too, or every refused binding would arrive at the
    /// engine having forgotten that it binds one name.
    #[must_use]
    pub fn writing(mut self, writes: Writes) -> Self {
        self.writes = writes;
        self
    }

    /// Which locals of its frame this node may rebind. See [`Writes`].
    #[must_use]
    pub const fn writes(&self) -> &Writes {
        &self.writes
    }

    /// Which node this is.
    #[must_use]
    pub const fn id(&self) -> NodeId {
        self.id
    }

    /// What was refused.
    #[must_use]
    pub const fn construct(&self) -> Construct {
        self.construct
    }

    /// Where it is, as the frontend spelled the position.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The frontend-supplied specifics, if any.
    #[must_use]
    pub const fn detail(&self) -> Option<&Symbol> {
        self.detail.as_ref()
    }

    /// This node as a diagnostic record.
    #[must_use]
    pub fn refusal(&self) -> Unsupported {
        match &self.detail {
            Some(detail) => {
                Unsupported::with_detail(self.construct, self.origin.clone(), detail.clone())
            }
            None => Unsupported::new(self.construct, self.origin.clone()),
        }
    }
}
