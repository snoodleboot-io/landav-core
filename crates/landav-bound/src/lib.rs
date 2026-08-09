//! `landav-bound` - the frozen bound algebra and cost-semiring layer.
//!
//! This crate is the one genuinely irreversible decision in M0. Everything
//! downstream (LAN-57 substitution, LAN-58 normalisation, LAN-60 `--resource`,
//! LAN-61 exit codes, the hosted platform's JSON ingest, and the F-008
//! incremental cache) is typed against the surface declared here.
//!
//! # The three structural decisions
//!
//! 1. **Both semiring carriers are [`Lifted<Bound>`].** `zero` is
//!    [`Lifted::Bottom`] - "no execution reaches here" - for *both* [`B`] and
//!    [`MaxPlus`]. `Bound::Const(0)` means one thing only: "proved to cost
//!    nothing". Because the annihilator is `Bottom` rather than `Const(0)`,
//!    the annihilation law no longer forces `0 * omega = 0`; this crate
//!    defines `0 * omega = omega` (omega absorbs unconditionally).
//!
//! 2. **[`Bound`] does not implement [`Ord`] or [`PartialOrd`].** A `<` on a
//!    symbolic bound reads as "tighter", which is semantic domination
//!    (F-018), which this crate does not decide. The total deterministic order
//!    that canonicalisation needs lives on the [`Canonical`] trait under a
//!    name that cannot be misread, and is reached only through
//!    [`Canonical::canonical_cmp`].
//!
//! 3. **[`Bound`] is an opaque handle over a private node.** Every node
//!    carries a constructor-computed depth and [`VarSet`]; the matchable
//!    [`BoundKind`] is an *observation* type that cannot be lifted back into a
//!    `Bound`. This is what makes [`MAX_DEPTH`] and the substitution
//!    fast-path unbypassable in safe code.
//!
//! # Non-negotiables this crate encodes
//!
//! * **Soundness has a zero target.** Every operator over-approximates
//!   upwards or is exact; `omega` is the top and saturation always goes there.
//! * **Never panic.** Every fallible path is a [`Result<_, BoundError>`];
//!   every operator on `N u {omega}` is total.
//! * **Failure carries blame.** [`Verdict::classify`] *refuses* to publish an
//!   `omega`-bearing bound with an empty blame ledger - that is
//!   [`BoundError::UnblamedOmega`], a tool error, not a clean report.
//! * **No frontend assumptions.** [`Origin`] and [`Symbol`] are opaque
//!   frontend-supplied handles; this crate attaches no meaning to either.
//!
//! # Determinism contract
//!
//! The canonical order, the canonical byte form and the normal form are one
//! versioned artefact, pinned by [`NORMAL_FORM_VERSION`]. Rust *declaration*
//! order is deliberately not load bearing: [`BoundShape::canonical_tag`]
//! assigns the tags explicitly, and the wire form pins its own names with
//! `#[serde(rename)]`. See [`NormaliserBudget`] for the configuration LAN-58
//! must use to keep e-graph extraction reproducible.

#![forbid(unsafe_code)]
// SKELETON ONLY. Every body in this crate is `todo!()`, so every parameter and
// every private field is unused by construction. These two allows exist to
// prove the *signatures* compile clean under the workspace lint table; they
// must be deleted by the first commit that fills in a body.
#![allow(unused_variables, dead_code)]

pub mod assumption;
pub mod b;
pub mod base;
pub mod blame;
pub mod blames;
pub mod bound;
pub mod bound_error;
pub mod bound_kind;
pub mod bound_shape;
pub mod bound_wire;
pub mod cache_key_material;
pub mod canonical;
pub mod canonical_bytes;
pub mod dioid;
#[cfg(any(test, feature = "laws"))]
pub mod dioid_laws;
pub mod exit_code;
pub mod finite_bound;
#[cfg(any(test, feature = "laws"))]
pub mod law;
#[cfg(any(test, feature = "laws"))]
pub mod law_failure;
pub mod lifted;
pub mod max_plus;
pub mod max_terms;
pub mod nat;
pub mod normaliser_budget;
pub mod origin;
pub mod partial_bound;
pub mod registry;
pub mod resource_descriptor;
pub mod resource_id;
pub mod semiring_id;
pub mod symbol;
pub mod terms;
pub mod total_valuation;
pub mod trans_kind;
pub mod valuation;
pub mod var_id;
pub mod var_set;
pub mod verdict;
pub mod wire_node;

pub use crate::{
    assumption::Assumption,
    b::B,
    base::Base,
    blame::Blame,
    blames::Blames,
    bound::Bound,
    bound_error::BoundError,
    bound_kind::BoundKind,
    bound_shape::BoundShape,
    bound_wire::BoundWire,
    cache_key_material::CacheKeyMaterial,
    canonical::Canonical,
    canonical_bytes::CanonicalBytes,
    dioid::Dioid,
    exit_code::ExitCode,
    finite_bound::FiniteBound,
    lifted::Lifted,
    max_plus::MaxPlus,
    max_terms::MaxTerms,
    nat::Nat,
    normaliser_budget::NormaliserBudget,
    origin::Origin,
    partial_bound::PartialBound,
    registry::{ResourceKind, registered},
    resource_descriptor::ResourceDescriptor,
    resource_id::ResourceId,
    semiring_id::SemiringId,
    symbol::Symbol,
    terms::Terms,
    total_valuation::TotalValuation,
    trans_kind::TransKind,
    valuation::Valuation,
    var_id::VarId,
    var_set::VarSet,
    verdict::Verdict,
    wire_node::WireNode,
};

#[cfg(any(test, feature = "laws"))]
pub use crate::{dioid_laws::DioidLaws, law::Law, law_failure::LawFailure};

/// The maximum nesting depth of a [`Bound`].
///
/// Enforced by the smart constructors, which are the only way to obtain a
/// `Bound`, so **every** value of the type satisfies it. Two consequences
/// follow, and both are load bearing:
///
/// * the recursive `PartialEq`/`Hash`/`Display` implementations cannot
///   overflow the stack, because no inhabitant is deeper than this;
/// * deserialisation cannot smuggle in a deep term, because the wire form is a
///   flat node table rebuilt through the same constructors.
///
/// Exceeding it widens to `omega` (sound, monotone) in the total constructors,
/// or raises [`BoundError::DepthExceeded`] in the `_checked` constructors, for
/// callers that want to attach blame rather than silently lose tightness.
pub const MAX_DEPTH: u16 = 512;

/// The maximum number of distinct nodes in a [`Bound`]'s DAG.
///
/// Bounds the serialised size, which `MAX_DEPTH` alone does not: a 30-level
/// chain of `b = b * b` is 31 shared nodes and would be `2^30` nodes if the
/// wire form were a tree. The wire form is a DAG (see [`BoundWire`]) and this
/// constant caps it.
pub const MAX_NODES: u32 = 1 << 20;

/// The version of the canonical order, the canonical byte form, the rewrite
/// set and the extraction cost function, taken together.
///
/// **Bump this whenever any of those four changes.** It prefixes
/// [`CacheKeyMaterial`], so bumping it invalidates every persisted F-008 cache
/// entry - which is the correct and only sound response to a normal-form
/// change.
pub const NORMAL_FORM_VERSION: u32 = 1;

/// The version tag carried by [`BoundWire`].
///
/// Independent of [`NORMAL_FORM_VERSION`]: the wire *shape* can be stable
/// across a normal-form change, and a normal-form change must not silently
/// look like a wire change to the hosted platform.
pub const WIRE_VERSION: u16 = 1;
