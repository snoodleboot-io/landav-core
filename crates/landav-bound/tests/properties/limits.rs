//! The four budgets, asserted **at their boundaries**.
//!
//! `landav-bound` carries four hard limits - nesting depth, n-ary arity, DAG
//! node count, and the tree size a wire document materialises - and each is a
//! single `>` against a constant. A comparison is only pinned by a test that
//! stands on both sides of it: "some large term is refused" is satisfied by a
//! guard that fires one step early, and "some small term is accepted" by one
//! that fires one step late. Both mistakes are invisible to every property in
//! this suite and both matter - an off-by-one at `MAX_DEPTH` makes a term this
//! crate built unreadable on the way back in, and an off-by-one at
//! `MAX_NODES` silently changes what the hosted platform will ingest.
//!
//! Mutation testing measured the gap precisely: every `>` in this crate's
//! guards survived being rewritten to `>=` or `==`, because the suite only
//! ever exercised one side of each.
//!
//! So every test here asserts a **pair**: the largest value inside the budget
//! is accepted, and the smallest value outside it is refused with the right
//! error carrying the right numbers.
//!
//! Standing on both sides of a `2^20` boundary costs nothing when the term is
//! chosen well. The tree-size test below reaches `MAX_NODES` exactly with a
//! forty-one node DAG, because a shared ladder's tree doubles per level and
//! `log` adds one node at a time - so the two are enough to hit any target.

use landav_bound::{Base, Bound, BoundError, BoundWire};

/// The tallest `Trans` chain that fits, plus one.
///
/// Each `log` of a non-constant argument adds exactly one level, so this walks
/// the depth guard in `Bound::transcendental` up to `MAX_DEPTH` and one step
/// past it.
#[test]
fn the_depth_limit_is_exact_at_the_transcendental_constructor() {
    let limit = landav_bound::MAX_DEPTH;
    let mut tower = Bound::var("x");
    assert_eq!(tower.depth(), 1, "a leaf is depth 1");

    for level in 2..=limit {
        let taller = Bound::log_checked(Base::TWO, tower.clone());
        assert!(
            taller.is_ok(),
            "depth {level} is inside the limit of {limit}, but log_checked refused: {taller:?}"
        );
        tower = taller.unwrap_or(tower);
        assert_eq!(
            tower.depth(),
            level,
            "each log of a symbolic argument adds exactly one level"
        );
    }
    assert_eq!(
        tower.depth(),
        limit,
        "a term of depth exactly MAX_DEPTH must be constructible"
    );

    // One past the limit.
    let over = Bound::log_checked(Base::TWO, tower.clone());
    assert!(
        matches!(&over, Err(BoundError::DepthExceeded { limit: named }) if *named == limit),
        "depth {} must be refused as DepthExceeded({limit}), not {over:?}",
        limit + 1
    );
    // The total constructor has no channel to report on, so it widens instead.
    assert_eq!(
        Bound::log(Base::TWO, tower),
        Bound::omega(),
        "the total constructor widens to omega where the checked one refuses"
    );
}

/// The same boundary at the n-ary constructors, whose depth guard is a
/// separate `>` in a separate function.
///
/// The operator alternates so that nothing flattens: a `Prod` under a `Sum`
/// and a `Sum` under a `Prod` both survive, and the depth really does grow by
/// one per level. The step past the limit uses the *same* alternation -
/// repeating the operator would flatten rather than nest, and the depth would
/// not grow at all.
#[test]
fn the_depth_limit_is_exact_at_the_n_ary_constructors() {
    let limit = landav_bound::MAX_DEPTH;
    let mut tower = Bound::var("x");

    let grow = |base: &Bound, level: u16| -> Result<Bound, BoundError> {
        let fresh = Bound::var(format!("y{level}"));
        if level.is_multiple_of(2) {
            Bound::prod_checked([base.clone(), fresh])
        } else {
            Bound::sum_checked([base.clone(), fresh])
        }
    };

    for level in 2..=limit {
        let taller = grow(&tower, level);
        assert!(
            taller.is_ok(),
            "depth {level} is inside the limit of {limit}, but the constructor refused: {taller:?}"
        );
        tower = taller.unwrap_or(tower);
        assert_eq!(tower.depth(), level, "each level must add exactly one");
    }
    assert_eq!(tower.depth(), limit);

    let over = grow(&tower, limit + 1);
    assert!(
        matches!(&over, Err(BoundError::DepthExceeded { limit: named }) if *named == limit),
        "depth {} must be refused as DepthExceeded({limit}), not {over:?}",
        limit + 1
    );
}

/// `b_{i+1} = (b_i * b_i) + 1`, whose *tree* is `4 * 2^i - 3` nodes while its
/// DAG is `2i + 2`.
fn shared_ladder(levels: u32) -> Bound {
    let mut bound = Bound::var("x");
    for _ in 0..levels {
        let squared = Bound::prod([bound.clone(), bound.clone()]);
        bound = Bound::sum([squared, Bound::constant(1)]);
    }
    bound
}

/// `pad` nested `log`s on top of `inner`, each adding exactly one tree node.
fn padded(inner: Bound, pad: u32) -> Bound {
    let mut bound = inner;
    for _ in 0..pad {
        bound = Bound::log(Base::TWO, bound);
    }
    bound
}

/// `Bound::try_from_wire` measures the **tree** a document materialises, not
/// the node table that carries it, and refuses above `MAX_NODES`. This walks
/// that guard across its boundary.
///
/// Eighteen ladder levels is a tree of `4 * 2^18 - 3 = 1 048 573` nodes over a
/// 38-node DAG, and each `log` on top adds exactly one tree node - so three
/// pads land on `MAX_NODES` exactly and four land one past it.
///
/// The refusal's `got` field is asserted as well as its variant, which pins
/// the tree-size arithmetic itself rather than only the comparison: a
/// `tree_size_of` that returns a constant leaves the guard working and makes
/// every document acceptable.
#[test]
fn the_tree_size_limit_is_exact_on_ingest() {
    let limit = landav_bound::MAX_NODES;
    let ladder = shared_ladder(18);

    // At the limit exactly: accepted, and the document itself is tiny.
    let at_limit = padded(ladder.clone(), 3);
    let document = at_limit.to_wire();
    assert!(
        matches!(&document, Ok(wire) if wire.nodes.len() < 64),
        "a 41-node DAG must serialise to a few dozen nodes: {document:?}"
    );
    if let Ok(wire) = &document {
        let rebuilt = Bound::try_from_wire(wire);
        assert_eq!(
            rebuilt.ok().as_ref(),
            Some(&at_limit),
            "a tree of exactly MAX_NODES ({limit}) must be accepted and round-trip"
        );
    }

    // One past it: refused, and the refusal names the size it measured.
    let over_limit = padded(ladder, 4);
    let document = over_limit.to_wire();
    assert!(
        document.is_ok(),
        "to_wire refused a 42-node DAG: {document:?}"
    );
    if let Ok(wire) = &document {
        let rebuilt = Bound::try_from_wire(wire);
        assert!(
            matches!(
                &rebuilt,
                Err(BoundError::TreeSizeExceeded { got, limit: named })
                    if *got == u64::from(limit) + 1 && *named == limit
            ),
            "a tree of {} nodes must be refused as TreeSizeExceeded, not {rebuilt:?}",
            u64::from(limit) + 1
        );
    }
}

/// Every term this suite builds is orders of magnitude inside the node budget,
/// so `to_wire` must accept all of them - and the version it stamps is the one
/// the hosted platform reads.
///
/// Stated separately from the round-trip property because a guard rewritten to
/// fire on *small* documents is caught here by a single term, without needing
/// the generator to produce one.
#[test]
fn to_wire_accepts_a_term_far_inside_the_node_budget() {
    let term = Bound::sum([
        Bound::var("x0"),
        Bound::prod([Bound::var("x1"), Bound::constant(3)]),
        Bound::log(Base::TWO, Bound::var("x2")),
    ]);
    let document = term.to_wire();
    assert!(
        document.is_ok(),
        "to_wire refused a {}-node term against a budget of {}: {document:?}",
        term.wire_node_count(),
        landav_bound::MAX_NODES
    );
    if let Ok(wire) = &document {
        assert_eq!(wire.version, landav_bound::WIRE_VERSION);
        assert_eq!(
            u32::try_from(wire.nodes.len()).unwrap_or(u32::MAX),
            term.wire_node_count(),
            "wire_node_count must report what to_wire emitted"
        );
        assert!(
            u32::try_from(wire.nodes.len()).unwrap_or(u32::MAX) < landav_bound::MAX_NODES,
            "the term is far inside the budget"
        );
    }
}

/// The version guard is exact: the supported version round-trips and its
/// neighbours are refused, naming both the version they carried and the one
/// this build reads.
#[test]
fn the_wire_version_guard_is_exact() {
    let term = Bound::sum([Bound::var("x0"), Bound::var("x1")]);
    let document = term.to_wire();
    assert!(document.is_ok(), "to_wire refused a two-operand sum");
    let Ok(wire) = document else { return };

    for offset in [1u16, 2] {
        let wrong = BoundWire {
            version: landav_bound::WIRE_VERSION.wrapping_add(offset),
            nodes: wire.nodes.clone(),
            root: wire.root,
        };
        let rebuilt = Bound::try_from_wire(&wrong);
        assert!(
            matches!(
                &rebuilt,
                Err(BoundError::WireVersionUnsupported { got, supported })
                    if *got == wrong.version && *supported == landav_bound::WIRE_VERSION
            ),
            "wire version {} must be refused as WireVersionUnsupported, not {rebuilt:?}",
            wrong.version
        );
    }

    assert_eq!(
        Bound::try_from_wire(&wire).ok(),
        Some(term),
        "the supported version must round-trip"
    );
}

/// The node budget on **ingest**, at its boundary.
///
/// `try_from_wire` refuses a document declaring more than `MAX_NODES` entries,
/// and that guard is the outermost thing standing between an untrusted
/// document and this crate's allocator: it runs before a single node is
/// rebuilt. Both sides of it are user-visible - one step early and the hosted
/// platform silently cannot send what the constant says it may, one step late
/// and the limit does not exist.
///
/// This is the one test in the suite that pays real time (about two seconds)
/// for its boundary, because `MAX_NODES` counts entries in a `Vec` and there
/// is no arithmetic trick that reaches `2^20` of them with fewer. The nodes
/// are `Const`s rather than `Var`s so that rebuilding them allocates no
/// strings, which is most of the difference between two seconds and four.
#[test]
fn the_ingest_node_budget_is_exact() {
    let limit = landav_bound::MAX_NODES;
    let document = |count: u32| BoundWire {
        version: landav_bound::WIRE_VERSION,
        // Distinct literals, so nothing about the outcome can come from the
        // table being uniform.
        nodes: (0..count)
            .map(|i| landav_bound::WireNode::Const {
                fin: Some(u64::from(i)),
            })
            .collect(),
        root: count.saturating_sub(1),
    };

    // Exactly at the budget: accepted, and the root really is the node the
    // document pointed at.
    let accepted = Bound::try_from_wire(&document(limit));
    assert_eq!(
        accepted.ok(),
        Some(Bound::constant(u64::from(limit) - 1)),
        "a document of exactly MAX_NODES ({limit}) entries must be accepted"
    );

    // One past it: refused, before anything is rebuilt.
    let refused = Bound::try_from_wire(&document(limit + 1));
    assert!(
        matches!(&refused, Err(BoundError::NodeBudgetExceeded { limit: named }) if *named == limit),
        "a document of {} entries must be refused as NodeBudgetExceeded({limit}), not {refused:?}",
        u64::from(limit) + 1
    );
}
