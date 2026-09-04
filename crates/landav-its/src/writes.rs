//! [`Writes`] - what a refused node may change in the frame it stands in.

use std::collections::BTreeSet;

use crate::{construct::Construct, var_name::VarName};

/// Which locals of its frame a refused node may rebind.
///
/// The locals half of [`Writes`]; see there for the argument.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Locals {
    /// The frontend said nothing, and the construct answers for its kind.
    #[default]
    Unstated,
    /// Any local of the frame, whatever the construct says.
    Any,
    /// At most these locals, and no other. Empty means none.
    AtMost(BTreeSet<VarName>),
}

/// What a refused node may change in the frame it stands in: which local
/// names, and whether any object.
///
/// # Why a node says this and not only its [`Construct`]
///
/// [`Construct::may_rebind_locals`] answers per *kind*, and a kind has to
/// answer for its worst member. `NonIntegerValue` is the kind of `total = 0`
/// when `total` is later condemned, and it is also the kind of a name read in
/// an f-string with a walrus inside it; the first rebinds exactly `total` and
/// the second may rebind anything, so the kind says "anything" and the engine
/// forgets every readable value at `total = 0`. Measured on the typed corpus:
/// 75 of the 91 loops that iterate a sized parameter lose their trip count to
/// a refusal earlier in the same function, and most of those refusals are a
/// binding of one known name. That is `LAN-100`.
///
/// The frontend saw the node and knows which case it is, so it says so here,
/// per node, and the kind's answer becomes the default a node with nothing to
/// say inherits. That is the same shape [`crate::DeclaredEffect`] has for a
/// call the signature pack accounts for, and it is deliberately a *separate*
/// field: a declaration turns the node into bounded work that is no longer a
/// refusal, whereas this leaves the node exactly as refused as it was - still a
/// hole, still a reason the program does not lower - and narrows only what the
/// hole is allowed to have changed.
///
/// # The two questions, and why both are here
///
/// A region ends what the engine knows in two different ways. It may **rebind
/// a local**, after which the name denotes a different value; and it may
/// **mutate an object**, after which a name standing for a property of that
/// object - a collection parameter's length on entry, see
/// [`crate::SourceProgram::is_volatile`] - is wrong without any local having
/// changed. `x.y` does the second and not the first; `total = 0` does neither
/// beyond `total` itself. A claim that answered only the first question would
/// leave the engine dropping every length at every refused binding, which is
/// precisely the loss this type exists to stop.
///
/// # What each answer licenses
///
/// | locals | the engine forgets |
/// |---|---|
/// | [`Locals::Unstated`] | whatever [`Construct::may_rebind_locals`] says for the kind |
/// | [`Locals::Any`] | every readable value, whatever the kind says |
/// | [`Locals::AtMost`]`(names)` | exactly `names`, which may be empty |
///
/// and, when the locals answer keeps anything at all, `mutates_objects` decides
/// whether the volatile names go too.
///
/// Every narrowing here is an over-approximation of what the node does and
/// must be one: a frontend that lists fewer names than the node can rebind, or
/// says "no object" of a node that runs a `property`, has licensed the engine
/// to read a value the program changed, and the bound derived from that value
/// is one the program can exceed. [`Locals::Any`] exists for the converse: a
/// kind whose default is "nothing" - an attribute access - can still hide a
/// walrus in an operand the frontend did not translate, and the frontend that
/// saw it needs a way to say so that outranks the kind.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Writes {
    locals: Locals,
    mutates_objects: bool,
}

impl Default for Writes {
    fn default() -> Self {
        Self::unstated()
    }
}

impl Writes {
    /// The frontend said nothing: the construct answers for the locals, and
    /// any object may have changed.
    #[must_use]
    pub const fn unstated() -> Self {
        Self {
            locals: Locals::Unstated,
            mutates_objects: true,
        }
    }

    /// Any local and any object: the frontend saw something it cannot narrow.
    #[must_use]
    pub const fn frame() -> Self {
        Self {
            locals: Locals::Any,
            mutates_objects: true,
        }
    }

    /// Nothing at all. A name read, a literal: the node is a value with no
    /// effect.
    #[must_use]
    pub const fn nothing() -> Self {
        Self {
            locals: Locals::AtMost(BTreeSet::new()),
            mutates_objects: false,
        }
    }

    /// Exactly these locals and no object. The shape of a refused binding:
    /// `total = <unreadable>` rebinds `total` and touches nothing else.
    #[must_use]
    pub fn only(names: impl IntoIterator<Item = VarName>) -> Self {
        Self {
            locals: Locals::AtMost(names.into_iter().collect()),
            mutates_objects: false,
        }
    }

    /// At most these locals, and any object. `x.y` rebinds nothing and runs a
    /// `property`; `a, b = pair` rebinds two names and runs an `__iter__`.
    #[must_use]
    pub fn at_most(names: impl IntoIterator<Item = VarName>) -> Self {
        Self {
            locals: Locals::AtMost(names.into_iter().collect()),
            mutates_objects: true,
        }
    }

    /// The locals half of the answer.
    #[must_use]
    pub const fn locals(&self) -> &Locals {
        &self.locals
    }

    /// Whether evaluating the node may mutate an object reachable from the
    /// frame. `true` unless the frontend said otherwise.
    #[must_use]
    pub const fn mutates_objects(&self) -> bool {
        self.mutates_objects
    }

    /// The names this node may rebind, or `None` if it may rebind any.
    ///
    /// This is the one place the per-node answer and the per-kind answer are
    /// combined, so that `landav-engine` asks one question at a region and
    /// cannot consult one of the two and forget the other.
    #[must_use]
    pub fn rebound(&self, construct: Construct) -> Option<&BTreeSet<VarName>> {
        match &self.locals {
            Locals::Unstated if construct.may_rebind_locals() => None,
            Locals::Unstated => Some(EMPTY.get_or_init(BTreeSet::new)),
            Locals::Any => None,
            Locals::AtMost(names) => Some(names),
        }
    }
}

/// The empty write set, for the kinds that rebind nothing by default.
static EMPTY: std::sync::OnceLock<BTreeSet<VarName>> = std::sync::OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unstated_defers_to_the_construct() {
        assert!(Writes::unstated().rebound(Construct::Call).is_none());
        assert_eq!(
            Writes::unstated().rebound(Construct::Attribute),
            Some(&BTreeSet::new())
        );
        assert!(Writes::unstated().mutates_objects());
    }

    #[test]
    fn frame_outranks_a_construct_that_rebinds_nothing() {
        assert!(Writes::frame().rebound(Construct::Attribute).is_none());
    }

    #[test]
    fn a_stated_set_outranks_a_construct_that_rebinds_anything() {
        let name = VarName::new("total");
        assert_eq!(
            Writes::only([name.clone()]).rebound(Construct::NonIntegerValue),
            Some(&BTreeSet::from([name.clone()]))
        );
        assert_eq!(
            Writes::at_most([name.clone()]).rebound(Construct::NonIntegerValue),
            Some(&BTreeSet::from([name]))
        );
        assert_eq!(
            Writes::nothing().rebound(Construct::NonIntegerValue),
            Some(&BTreeSet::new())
        );
    }

    #[test]
    fn only_and_nothing_touch_no_object_and_at_most_may() {
        assert!(!Writes::nothing().mutates_objects());
        assert!(!Writes::only([VarName::new("x")]).mutates_objects());
        assert!(Writes::at_most([]).mutates_objects());
    }
}
