//! **Failure must carry blame**, mechanised.
//!
//! Two decisions are pinned here, and both were defects found before
//! implementation:
//!
//! * **`omega` with no blame is a tool error, never a clean report.**
//!   `omega`-freeness is not a sound proxy for "nothing was unaccounted for" -
//!   provenance flows through the ledger, not by scanning the final term - so
//!   an unbounded result with an empty ledger is refused rather than
//!   published.
//! * **`Bottom` is `Unreachable`, not `Proved(0)`.** `Bottom` is the fixpoint
//!   seed for every not-yet-visited node, so a `Verdict` without this arm
//!   would publish "cost 0, proved, no blame" for every unvisited region.

use std::collections::BTreeMap;

use landav_bound::{
    Assumption, Blame, Blames, Bound, BoundError, ExitCode, FiniteBound, Lifted, Nat, Origin,
    PartialBound, TotalValuation, Valuation, VarId, Verdict,
};
use proptest::prelude::*;

use crate::support::{BoundSpec, arb_spec, build, ref_nat};

/// A blame record built from an index, so the generators can produce ledgers
/// without inventing frontend vocabulary.
fn blame(index: usize) -> Blame {
    let assumption = match index % 4 {
        0 => Assumption::TerminationNotProved,
        1 => Assumption::RecursionNotRanked,
        2 => Assumption::SizeNotBounded {
            var: VarId::new(format!("v{index}")),
        },
        _ => Assumption::CalleeCostUnknown {
            callee: format!("callee{index}").into(),
        },
    };
    Blame {
        unaccounted: format!("term{index}").into(),
        assumption,
        origin: Origin::new(format!("file.rs:{index}")),
    }
}

/// A ledger containing exactly one record.
fn one_blame() -> Blames {
    Blames::new(blame(0))
}

proptest! {
    /// The classification table, exhaustively, over arbitrary bounds and both
    /// ledger states.
    #[test]
    fn classification_follows_the_frozen_table(
        spec in arb_spec(),
        with_blame in any::<bool>(),
        unreachable in any::<bool>(),
    ) {
        let bound = build(&spec);
        let at = Origin::new("classify.rs:1");
        let ledger = if with_blame { Some(one_blame()) } else { None };
        let cost = if unreachable {
            Lifted::Bottom
        } else {
            Lifted::Elem(bound.clone())
        };

        let got = Verdict::classify(cost, at.clone(), ledger);

        match (unreachable, with_blame) {
            (true, false) => {
                prop_assert_eq!(got, Ok(Verdict::Unreachable(at)));
            }
            (true, true) => {
                // Reachability was not established *and* something was
                // unaccounted for: a partial over omega, never Unreachable.
                match got {
                    Ok(Verdict::Partial(partial)) => {
                        prop_assert_eq!(partial.bound(), &Bound::omega());
                        prop_assert_eq!(partial.blames(), &one_blame());
                    }
                    other => prop_assert!(
                        false,
                        "Bottom with blame must be Partial over omega, got {other:?}"
                    ),
                }
            }
            (false, true) => {
                prop_assert_eq!(
                    got,
                    Ok(Verdict::Partial(PartialBound::new(bound, one_blame())))
                );
            }
            (false, false) => {
                if bound.is_finite() {
                    match FiniteBound::try_new(bound.clone()) {
                        Ok(finite) => prop_assert_eq!(got, Ok(Verdict::Proved(finite))),
                        Err(_) => prop_assert!(
                            false,
                            "is_finite() said yes but FiniteBound::try_new refused {bound}"
                        ),
                    }
                } else {
                    prop_assert_eq!(
                        got,
                        Err(BoundError::UnblamedOmega),
                        "an unblamed omega-bearing bound must be a tool error"
                    );
                }
            }
        }
    }

    /// A published verdict never carries an unblamed `omega`, and never
    /// carries an empty ledger with a `Partial`.
    #[test]
    fn no_verdict_publishes_an_unblamed_omega(
        spec in arb_spec(),
        with_blame in any::<bool>(),
    ) {
        let bound = build(&spec);
        let ledger = if with_blame { Some(one_blame()) } else { None };
        let Ok(verdict) = Verdict::classify(
            Lifted::Elem(bound),
            Origin::new("classify.rs:2"),
            ledger,
        ) else {
            // Refusal is the correct outcome for an unblamed omega.
            return Ok(());
        };

        match &verdict {
            Verdict::Proved(finite) => {
                prop_assert!(finite.get().is_finite(), "Proved must be omega-free");
                prop_assert_eq!(verdict.blames(), None);
                prop_assert_eq!(verdict.exit_code(false), ExitCode::Clean);
                prop_assert_eq!(verdict.exit_code(true), ExitCode::Clean);
            }
            Verdict::Partial(_) => {
                match verdict.blames() {
                    Some(blames) => {
                        prop_assert!(!blames.is_empty());
                        prop_assert_eq!(blames.len(), blames.as_slice().len());
                    }
                    None => prop_assert!(false, "Partial must carry blame"),
                }
            }
            Verdict::Unreachable(_) => {
                prop_assert_eq!(verdict.bound(), None, "Unreachable has no cost to report");
            }
        }
    }

    /// The blame ledger's order is **content determined**, not push
    /// determined: the same records inserted in any order produce the same
    /// slice, sorted and deduplicated.
    #[test]
    fn blame_order_is_content_determined(
        indices in proptest::collection::vec(0usize..6, 1..8),
    ) {
        let mut forwards = Blames::new(blame(indices[0]));
        for i in indices.iter().skip(1) {
            forwards.insert(blame(*i));
        }

        let mut backwards = Blames::new(blame(indices[indices.len() - 1]));
        for i in indices.iter().rev().skip(1) {
            backwards.insert(blame(*i));
        }

        prop_assert_eq!(forwards.as_slice(), backwards.as_slice());

        let records = forwards.as_slice();
        prop_assert!(!forwards.is_empty());
        prop_assert_eq!(forwards.len(), records.len());
        for pair in records.windows(2) {
            prop_assert!(
                pair[0] < pair[1],
                "blames are not sorted and deduplicated: {records:?}"
            );
        }
    }

    /// Merging is the same operation as repeated insertion.
    #[test]
    fn merging_ledgers_agrees_with_inserting(
        left in proptest::collection::vec(0usize..6, 1..5),
        right in proptest::collection::vec(0usize..6, 1..5),
    ) {
        let mut merged = Blames::new(blame(left[0]));
        for i in left.iter().skip(1) {
            merged.insert(blame(*i));
        }
        let mut other = Blames::new(blame(right[0]));
        for i in right.iter().skip(1) {
            other.insert(blame(*i));
        }
        merged.merge(other);

        let mut inserted = Blames::new(blame(left[0]));
        for i in left.iter().skip(1).chain(right.iter()) {
            inserted.insert(blame(*i));
        }

        prop_assert_eq!(merged.as_slice(), inserted.as_slice());
    }

    /// `FiniteBound::try_new` accepts exactly the `omega`-free bounds, and
    /// hands the bound back unchanged when it refuses.
    #[test]
    fn finite_bound_accepts_exactly_the_omega_free_terms(spec in arb_spec()) {
        let bound = build(&spec);
        match FiniteBound::try_new(bound.clone()) {
            Ok(finite) => {
                prop_assert!(bound.is_finite());
                prop_assert_eq!(finite.get(), &bound);
            }
            Err(returned) => {
                prop_assert!(!bound.is_finite());
                prop_assert_eq!(returned, bound, "the rejected bound must come back unchanged");
            }
        }
    }

    /// `TotalValuation::saturating` sends every absent variable to `omega`,
    /// which is the only sound policy for analysis.
    #[test]
    fn saturating_valuations_send_absent_variables_to_omega(
        known_value in 0u64..1000,
    ) {
        let mut known = BTreeMap::new();
        known.insert(VarId::new("present"), Nat::Fin(known_value));
        let valuation = TotalValuation::saturating(known);

        prop_assert_eq!(valuation.value_of(&VarId::new("present")), Nat::Fin(known_value));
        prop_assert_eq!(valuation.value_of(&VarId::new("absent")), Nat::OMEGA);
    }

    /// `with_default` is the property-test constructor: a chosen point on a
    /// grid rather than an over-approximation of an unknown.
    #[test]
    fn with_default_uses_the_supplied_default(default in crate::support::arb_ref()) {
        let valuation = TotalValuation::with_default(BTreeMap::new(), ref_nat(default));
        prop_assert_eq!(valuation.value_of(&VarId::new("anything")), ref_nat(default));
    }
}

/// **`Bottom` is `Unreachable`, not `Proved(0)`.**
#[test]
fn bottom_with_no_blame_is_unreachable_never_a_proved_zero() {
    let at = Origin::new("fixpoint.rs:1");
    let got = Verdict::classify(Lifted::Bottom, at.clone(), None);
    assert_eq!(got, Ok(Verdict::Unreachable(at)));

    if let Ok(verdict) = got {
        assert_eq!(verdict.bound(), None, "Unreachable reports no cost, not 0");
        assert_eq!(verdict.blames(), None);
        assert_eq!(verdict.exit_code(false), ExitCode::Clean);
        assert_eq!(verdict.exit_code(true), ExitCode::Clean);
    }
}

/// **`omega` with no blame is a tool error.** Not `Proved`, and not an
/// unblamed `Partial`.
#[test]
fn an_unblamed_omega_is_refused() {
    assert_eq!(
        Verdict::classify(
            Lifted::Elem(Bound::omega()),
            Origin::new("derive.rs:1"),
            None
        ),
        Err(BoundError::UnblamedOmega)
    );

    // Also when the `omega` is buried inside a larger term rather than at the
    // root: the check is on the ledger, but the term must not slip through.
    let buried = Bound::sum([Bound::var("n"), Bound::omega()]);
    assert_eq!(
        Verdict::classify(Lifted::Elem(buried), Origin::new("derive.rs:2"), None),
        Err(BoundError::UnblamedOmega)
    );

    // With blame, the same bound is publishable.
    let blamed = Verdict::classify(
        Lifted::Elem(Bound::omega()),
        Origin::new("derive.rs:3"),
        Some(one_blame()),
    );
    assert_eq!(
        blamed,
        Ok(Verdict::Partial(PartialBound::new(
            Bound::omega(),
            one_blame()
        )))
    );
}

/// An empty blame list - a bare "unknown" - has no representation. The only
/// constructor takes a first `Blame` by value.
#[test]
fn a_ledger_can_never_be_empty() {
    let ledger = Blames::new(blame(0));
    assert_eq!(ledger.len(), 1);
    assert!(!ledger.is_empty());
    assert_eq!(ledger.as_slice().len(), 1);

    // Inserting the same record twice does not grow it.
    let mut deduplicated = Blames::new(blame(1));
    deduplicated.insert(blame(1));
    assert_eq!(deduplicated.len(), 1);
}

/// `--fail-on-partial` must exist, and the default must be documented rather
/// than assumed: without the flag, a file where every function came back
/// `Partial` **with blame** reports clean.
///
/// `ExitCode::Findings` is unreachable in M0 (it needs semantic domination,
/// which is F-018), so the only code left for "we could not look" is
/// `ToolError`.
#[test]
fn partial_reports_clean_unless_fail_on_partial_is_set() -> Result<(), BoundError> {
    let verdict = Verdict::classify(
        Lifted::Elem(Bound::omega()),
        Origin::new("cli.rs:1"),
        Some(one_blame()),
    )?;

    assert_eq!(verdict.exit_code(false), ExitCode::Clean);
    assert_ne!(verdict.exit_code(true), ExitCode::Clean);
    assert_ne!(
        verdict.exit_code(true),
        ExitCode::Findings,
        "Findings is unreachable until F-018 lands"
    );
    assert_eq!(verdict.exit_code(true), ExitCode::ToolError);

    assert_eq!(ExitCode::Clean.as_i32(), 0);
    assert_eq!(ExitCode::Findings.as_i32(), 1);
    assert_eq!(ExitCode::ToolError.as_i32(), 2);
    Ok(())
}

/// `require_total` names the **least** absent variable in `VarId` order.
/// "Least", not "first": a hash-ordered "first" differs between two runs of
/// the same binary, and this message reaches a CI log diff.
#[test]
fn require_total_names_the_least_absent_variable() {
    let valuation = TotalValuation::with_default(BTreeMap::new(), Nat::OMEGA);
    let asked_in_one_order = valuation.clone().require_total([
        VarId::new("zeta"),
        VarId::new("alpha"),
        VarId::new("mu"),
    ]);
    let asked_in_another = valuation.clone().require_total([
        VarId::new("mu"),
        VarId::new("zeta"),
        VarId::new("alpha"),
    ]);

    let expected = Some(BoundError::UnboundVariable {
        var: VarId::new("alpha"),
    });
    assert_eq!(asked_in_one_order.err(), expected);
    assert_eq!(asked_in_another.err(), expected);

    // With every variable known, it succeeds.
    let mut known = BTreeMap::new();
    known.insert(VarId::new("alpha"), Nat::Fin(1));
    let complete = TotalValuation::with_default(known, Nat::OMEGA);
    assert!(complete.require_total([VarId::new("alpha")]).is_ok());
}

/// `BoundError` is deliberately separate from blame: caller misuse ends in a
/// message, blame ends in a partial bound. Collapsing them puts "we could not
/// size `n`" on the `?` path, and the `?` path does not end in a bound.
#[test]
fn caller_misuse_and_blame_are_different_channels() {
    // Caller misuse.
    assert_eq!(
        landav_bound::Base::new(1).err(),
        Some(BoundError::BaseTooSmall { got: 1 })
    );

    // Blame.
    let partial = PartialBound::new(Bound::omega(), one_blame());
    assert_eq!(partial.blames().len(), 1);
    assert_eq!(partial.bound(), &Bound::omega());
}

/// A derivation that reaches `omega` through a *zero-trip loop with an
/// unanalysed body* is exactly the case `0 * omega = 0` used to launder into
/// `Proved(0)`. End to end, through `Verdict`.
#[test]
fn a_zero_trip_loop_with_an_unanalysed_body_is_not_proved_zero() {
    let trip_count = Bound::var("trips");
    let body_cost = Bound::var("body");
    let loop_cost = Bound::prod([trip_count, body_cost]);

    let mut known = BTreeMap::new();
    known.insert(VarId::new("trips"), Nat::ZERO);
    let valuation = TotalValuation::saturating(known);

    let value = loop_cost.eval(&valuation);
    assert_eq!(value, Nat::OMEGA);

    let verdict = Verdict::classify(
        Lifted::Elem(Bound::magnitude(value)),
        Origin::new("loop.rs:7"),
        None,
    );
    assert_eq!(verdict, Err(BoundError::UnblamedOmega));

    // The `BoundSpec` route agrees, so the generators cover this shape too.
    let spec = BoundSpec::Prod(vec![BoundSpec::Const(Some(0)), BoundSpec::Const(None)]);
    assert_eq!(build(&spec), Bound::omega());
}

/// The two publishable arms **report the bound they were classified from**.
///
/// `Verdict::bound` is the accessor a caller prints, and the only assertion on
/// it was that `Unreachable` reports `None` - which a `bound()` returning
/// `None` unconditionally also satisfies. A verdict whose cost cannot be read
/// back is a report with no number in it.
#[test]
fn a_publishable_verdict_reports_the_bound_it_was_classified_from() {
    let finite = Bound::sum([Bound::var("n"), Bound::constant(4)]);
    let at = Origin::new("publish.rs:1");

    let proved = Verdict::classify(Lifted::Elem(finite.clone()), at.clone(), None);
    assert!(
        proved.is_ok(),
        "an omega-free bound with no blame must be Proved: {proved:?}"
    );
    if let Ok(verdict) = &proved {
        assert_eq!(
            verdict.bound(),
            Some(&finite),
            "Proved must report the bound it proved"
        );
        assert_eq!(verdict.blames(), None);
        assert_eq!(verdict.exit_code(true), ExitCode::Clean);
    }

    let loose = Bound::max_of([Bound::var("n"), Bound::omega()]);
    let partial = Verdict::classify(Lifted::Elem(loose.clone()), at.clone(), Some(one_blame()));
    assert!(
        partial.is_ok(),
        "a blamed omega must be publishable as Partial: {partial:?}"
    );
    if let Ok(verdict) = &partial {
        assert_eq!(
            verdict.bound(),
            Some(&loose),
            "Partial must report the bound it over-approximated with"
        );
        assert_eq!(verdict.blames(), Some(&one_blame()));
        assert_eq!(verdict.exit_code(false), ExitCode::Clean);
        assert_eq!(verdict.exit_code(true), ExitCode::ToolError);
    }

    // `Bottom` with blame is a `Partial` over `omega`, so it reports a cost
    // too - the only arm that does not is `Unreachable`.
    let from_bottom = Verdict::classify(Lifted::Bottom, at, Some(one_blame()));
    assert_eq!(
        from_bottom.as_ref().ok().and_then(Verdict::bound),
        Some(&Bound::omega()),
        "Bottom with blame must be a Partial over omega: {from_bottom:?}"
    );
}

/// [`Origin`] is an opaque frontend-supplied location: core attaches no
/// meaning to it, and its entire contract is that whatever went in comes back
/// out unchanged, through the accessor and through `Display` alike.
///
/// That is a low bar and it is the whole point - `Origin` is what a user reads
/// to find the line the blame is about, so an accessor returning a constant
/// sends every report to the same place.
#[test]
fn an_origin_carries_the_frontend_string_back_out_unchanged() {
    for location in ["main.c:12", "", "a/b/c.rs:3:14", "no line number at all"] {
        let origin = Origin::new(location);
        assert_eq!(origin.as_str(), location, "Origin altered its payload");
        assert_eq!(
            origin.to_string(),
            location,
            "Display and as_str must agree"
        );
    }

    // Distinct locations stay distinct, and the order is content derived -
    // `Blames` sorts by it, and that order reaches the report text.
    let (first, second) = (Origin::new("a.rs:1"), Origin::new("a.rs:2"));
    assert_ne!(first, second);
    assert!(first < second);
    assert_eq!(first, Origin::new("a.rs:1"));

    // And the blame ledger really is non-empty and reports its own size.
    let mut ledger = one_blame();
    assert!(!ledger.is_empty());
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.as_slice().len(), ledger.len());
    ledger.insert(blame(1));
    assert_eq!(ledger.len(), 2, "a distinct record must be added");
    assert_eq!(ledger.as_slice().len(), ledger.len());
    ledger.insert(blame(1));
    assert_eq!(ledger.len(), 2, "a duplicate record must not be added");
}
