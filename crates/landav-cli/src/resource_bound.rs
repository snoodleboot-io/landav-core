//! [`ResourceBound`] - what a run may report for the selected resource, per
//! function.
//!
//! # What is derived, and from what
//!
//! `queries` is "external calls issued". Since LAN-87 a call is not a refusal
//! that discards the function; it is a [`landav_engine::Hole`] charged **at its
//! position in the control structure**, carrying `construct() == "call"` and an
//! origin. So the engine has already done the hard part: the cost bound for
//! `for i in range(10): fetch(i)` is `1 + 10 * (2 + #hole0)`, in which the hole
//! variable is *multiplied by the trip count*.
//!
//! The query count is that multiplier. This module reads the **coefficient of
//! the call holes** out of the cost bound the engine derived, which is why it
//! is a projection of an existing answer rather than a second analysis:
//!
//! * a hole variable standing for a `call` region contributes `1`;
//! * a hole variable standing for anything else - a `while`, a `collection` -
//!   contributes *itself*, because how many calls that region issues is exactly
//!   what was not derived;
//! * everything else - constants, program variables - contributes `0`, because
//!   a step is not a call.
//!
//! Sum, max and product are then interpreted structurally, so the resource
//! count inherits the engine's control-flow rules instead of re-deriving them.
//!
//! # The three counting rules, and why each is the only sound one
//!
//! **A call in a loop counts once per iteration.** The obvious implementation
//! counts entries in [`landav_engine::TripCount::holes`], and that ledger holds
//! one entry per *syntactic region*: `for i in range(10): fetch(i)` would report
//! `1` where ten calls are issued. Under-counting is the one direction a
//! resource bound may never move - a loose bound is unhelpful, a low one is a
//! gate waving through a function that issues ten times its budget, with a
//! number that looks entirely reasonable on the way past. Reading the
//! coefficient gives `10` because that is what the engine already charged.
//!
//! **A call in a branch is the worse arm, not the sum.** `Bound::max_of` is how
//! the engine joins branches ([`landav_engine::TripCount::branching`]), and
//! interpreting `Max` as a maximum here is what keeps the two numbers - printed
//! side by side in the same JSON - from using different control-flow rules and
//! drifting apart on every nested branch. Summing the arms is sound but charges
//! for calls no execution issues.
//!
//! **A region the engine did not read never reports a confident zero.** A
//! `while` body may call out on every iteration; the run has no evidence either
//! way. So a non-call hole survives into the query bound as a variable, the
//! result is reported as `partial`, and no number is offered. A zero is the most
//! dangerous answer available here, because every gate passes it.
//!
//! # What the number now guarantees, and what it costs
//!
//! **A confident count has descended into every refused container.** `exact`
//! and `upper` are offered only where the frontend accounted for every call in
//! the function; where one was left inside a refused node it did not translate,
//! the result is `partial` and no number is offered. `LAN-97`.
//!
//! That is a change of *direction* rather than of precision. A call written in
//! a refused container - a callee, a refused binary operator's operand, a
//! subscript index - exists in no arena, so it is in no ledger and in no bound,
//! and a projection reading coefficients cannot see that it is missing.
//! `lookup(n).method()` is one `call` hole whose coefficient reads as `1`, for
//! a statement issuing two. Measured over `/usr/lib/python3.12` and a typed
//! corpus, both deduplicated by real path, 30.1% and 11.3% of confident results
//! were below an `ast`-derived truth and labelled `exact`.
//!
//! Closing each container is an ordinary precision improvement - it moves
//! functions from `partial` back to a number - and each carries its own blame
//! cost, which is why `LAN-96` did one and stopped. What could not wait is the
//! *label*: an unhelpful `partial` costs a reader nothing, and a confident 10
//! for a function issuing 78 costs them an incident. So the count fails closed,
//! and the containers are widened on their own schedule underneath it.
//!
//! The trade is visible and was measured rather than assumed: the count of
//! confident results falls, and that number is recorded beside the under-count
//! rate in `LAN-97` so a reader can see what honesty cost.
//!
//! The frontend answers the question - see
//! [`landav_its::SourceProgram::conceals_a_call`] - because whether a container
//! was descended into is a fact about the translation, not about the bound.
//! **Nothing here reads that flag for the cost bound**, and nothing should: the
//! enclosing region dominates the concealed call, so the cost is as sound as it
//! ever was, and only a projection that counts *individual* calls is defeated.

use std::collections::BTreeSet;

use landav_bound::{Bound, BoundKind, Nat, TotalValuation, VarId};
use landav_engine::{Hole, TripCount};
use landav_its::SourceProgram;

/// What the run may report for the selected resource, for one function.
///
/// Every field can be absent, and absence is never zero - the same rule
/// [`crate::machine::Function`] states for the cost bound, and it matters more
/// here: a consumer that reads a missing query count as "issues no queries" has
/// inverted the tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceBound {
    /// The derived count, rendered the way the cost bound already is.
    ///
    /// Symbolic where the count is symbolic: `for i in range(n): fetch(i)`
    /// issues `n` queries, and rounding that to a number would be a fabrication.
    pub bound: Option<String>,
    /// `"exact"`, `"upper"`, `"partial"`, or `None` when nothing was derived.
    pub kind: Option<&'static str>,
    /// The same quantity as a number, when the expression is a closed constant.
    ///
    /// `None` when it is symbolic or mentions a region that was not derived, so
    /// that a gate can threshold on this field without parsing algebra and
    /// without ever thresholding on something that is not a total.
    pub value: Option<u64>,
}

impl ResourceBound {
    /// Nothing derived: three absences, and not a zero anywhere.
    const fn undetermined() -> Self {
        Self {
            bound: None,
            kind: None,
            value: None,
        }
    }

    /// The queries `cost` charges, or [`Self::undetermined`].
    ///
    /// `cost` is the engine's own answer for this function, already filtered by
    /// [`crate::derived::cost_of`] - so a function the toolchain refused for a
    /// reason that left no node in the arena has no cost here and therefore no
    /// query count either, rather than a plausible-looking one.
    #[must_use]
    pub fn queries_of(cost: &TripCount, program: &SourceProgram) -> Self {
        let Some(bound) = cost.bound() else {
            return Self::undetermined();
        };
        let calls: BTreeSet<VarId> = cost
            .holes()
            .iter()
            .filter(|hole| hole.construct() == landav_its::Construct::Call.tag())
            .map(Hole::var)
            .collect();
        let Some(derived) = charge_calls(bound, &calls) else {
            // The bound has a shape this projection cannot read - a hole under
            // an exponent, or two hole-bearing factors in one product. Neither
            // is reachable from the engine as it stands, and if one becomes
            // reachable the honest answer is that nothing was derived, not a
            // number obtained by guessing which factor was the trip count.
            return Self::undetermined();
        };
        // A call the frontend could not account for is a call no coefficient
        // in this bound stands for, so no arithmetic here can notice it is
        // missing. `LAN-97`: the claim fails closed rather than naming a number
        // below the truth. See `SourceProgram::conceals_a_call`.
        let unread = derived.vars().iter().any(Hole::is_hole) || program.conceals_a_call();
        Self {
            kind: Some(if unread {
                // The count mentions a region nobody read, and an unfilled hole
                // denotes omega. `exact` and `upper` are complete claims a
                // consumer may compare against a budget; neither is available.
                "partial"
            } else if cost.exact_outside_holes() {
                // Every coefficient in the bound came out of arithmetic the
                // engine performed exactly; relaxing anywhere would have cleared
                // this flag, and it is conjunctive through `TripCount::combine`.
                "exact"
            } else {
                "upper"
            }),
            // Withheld for a concealed call as well as for an unread region:
            // this is the field a gate thresholds on, and it is the one place
            // a low number does the damage.
            value: (!program.conceals_a_call())
                .then(|| closed_value(&derived))
                .flatten(),
            bound: Some(derived.to_string()),
        }
    }
}

/// The coefficient of the call holes in `bound`, as a bound.
///
/// `None` where the shape is one this projection cannot read; see
/// [`ResourceBound::queries_of`] for why that is an absence rather than a
/// fallback.
///
/// # Why the interpretation is structural
///
/// Every rule below is the resource reading of the operator the engine used, so
/// the query count and the cost bound cannot come to different conclusions about
/// the same control flow. `Max` is a maximum because a branch runs one arm;
/// `Sum` adds because statements run in sequence; a `Prod` multiplies the
/// hole-bearing factor by the rest, which is the trip count, because the region
/// runs once per iteration.
fn charge_calls(bound: &Bound, calls: &BTreeSet<VarId>) -> Option<Bound> {
    match bound.kind() {
        // A step is not a call.
        BoundKind::Const(_) => Some(Bound::zero()),
        BoundKind::Var(var) => Some(if calls.contains(var) {
            // One entry into a call region is one call issued.
            Bound::one()
        } else if Hole::is_hole(var) {
            // A region nobody read. How many calls it issues is precisely what
            // was not derived, so the variable stands for that too, and the
            // result will be reported as partial rather than as a number.
            bound.clone()
        } else {
            // A program variable is a size, not a call.
            Bound::zero()
        }),
        BoundKind::Sum(terms) => terms
            .as_slice()
            .iter()
            .map(|term| charge_calls(term, calls))
            .collect::<Option<Vec<Bound>>>()
            .map(Bound::sum),
        BoundKind::Max(terms) => terms
            .as_slice()
            .iter()
            .map(|term| charge_calls(term, calls))
            .collect::<Option<Vec<Bound>>>()
            .map(Bound::max_of),
        BoundKind::Prod(terms) => {
            let factors = terms.as_slice();
            let mut charged: Option<Bound> = None;
            let mut rest: Vec<Bound> = Vec::with_capacity(factors.len());
            for factor in factors {
                if !mentions_hole(factor) {
                    rest.push(factor.clone());
                    continue;
                }
                if charged.is_some() {
                    // Two hole-bearing factors: the product of two unread
                    // regions, which the engine does not build - a trip count is
                    // read before its body is walked and never carries a hole.
                    // There is no way to say which factor is the count, and
                    // guessing would put a region's cost where its multiplicity
                    // belongs.
                    return None;
                }
                charged = Some(charge_calls(factor, calls)?);
            }
            charged.map_or_else(
                || Some(Bound::zero()),
                |inner| {
                    rest.push(inner);
                    Some(Bound::prod(rest))
                },
            )
        }
        // `base ^ arg` and `ceil(log_base arg)`. Both are hole-free in every
        // bound the engine builds - holes reach the algebra only through
        // addition and multiplication - and neither has a linear coefficient to
        // read off if one ever appears.
        BoundKind::Trans { arg, .. } => (!mentions_hole(arg)).then(Bound::zero),
    }
}

/// Whether `bound` mentions a region the engine could not derive.
fn mentions_hole(bound: &Bound) -> bool {
    bound.vars().iter().any(Hole::is_hole)
}

/// `bound` as a number, when it is a closed constant.
///
/// `None` for anything symbolic, including `omega`: a gate thresholding on this
/// field must never receive a value it can compare successfully against a budget
/// unless the run established a total.
fn closed_value(bound: &Bound) -> Option<u64> {
    if !bound.vars().is_empty() {
        return None;
    }
    match bound.eval(&TotalValuation::saturating(
        std::collections::BTreeMap::new(),
    )) {
        Nat::Fin(count) => Some(count),
        Nat::Omega => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a failed unwrap is a failed test")]

    use super::{ResourceBound, charge_calls};
    use landav_bound::{Bound, Origin, VarId};
    use landav_engine::{Hole, TripCount};
    use landav_its::{SourceProgram, SourceProgramBuilder};
    use std::collections::BTreeSet;

    fn call_hole(index: usize) -> Hole {
        Hole::new(index, "call", Origin::new("probe.py:1:1"))
    }

    fn while_hole(index: usize) -> Hole {
        Hole::new(index, "while", Origin::new("probe.py:1:1"))
    }

    fn calls(holes: &[Hole]) -> BTreeSet<VarId> {
        holes.iter().map(Hole::var).collect()
    }

    /// A step is not a call. The constants the engine charges for statements
    /// must contribute nothing, or every function would report its step count
    /// as its query count.
    #[test]
    fn statements_contribute_no_queries() {
        let charged = charge_calls(&Bound::constant(17), &BTreeSet::new()).unwrap();
        assert_eq!(charged, Bound::zero());
    }

    /// Two calls in sequence are two queries: `Sum` adds.
    #[test]
    fn calls_in_sequence_add() {
        let (first, second) = (call_hole(0), call_hole(1));
        let bound = Bound::sum([Bound::constant(3), first.as_bound(), second.as_bound()]);
        let charged = charge_calls(&bound, &calls(&[first, second])).unwrap();
        assert_eq!(charged, Bound::constant(2));
    }

    /// A call multiplied by a trip count is that many queries. This is the
    /// assertion the ledger-counting implementation fails: it would say one.
    #[test]
    fn a_call_under_a_trip_count_counts_once_per_iteration() {
        let hole = call_hole(0);
        let body = Bound::sum([Bound::constant(2), hole.as_bound()]);
        let bound = Bound::sum([Bound::one(), Bound::prod([Bound::constant(10), body])]);
        let charged = charge_calls(&bound, &calls(&[hole])).unwrap();
        assert_eq!(charged, Bound::constant(10));
    }

    /// A branch takes the worse arm and not the sum, matching how the engine
    /// already joins branches.
    /// A program with nothing in it, which therefore conceals no call.
    ///
    /// Every projection test below is about the *arithmetic*, so it is handed a
    /// program that makes no claim of its own. The two tests at the end are
    /// about the flag itself.
    fn accounted() -> SourceProgram {
        SourceProgramBuilder::new("f", Origin::new("q.py:1:1"), Vec::new()).build(Vec::new())
    }

    /// The same, having left a call inside something it did not translate.
    fn concealing() -> SourceProgram {
        let mut builder = SourceProgramBuilder::new("f", Origin::new("q.py:1:1"), Vec::new());
        builder.mark_concealed_call();
        builder.build(Vec::new())
    }

    #[test]
    fn a_branch_takes_the_worse_arm() {
        let (left, right, only) = (call_hole(0), call_hole(1), call_hole(2));
        let bound = Bound::max_of([
            Bound::sum([Bound::constant(1), only.as_bound()]),
            Bound::sum([Bound::constant(2), left.as_bound(), right.as_bound()]),
        ]);
        let charged = charge_calls(&bound, &calls(&[left, right, only])).unwrap();
        assert_eq!(charged, Bound::constant(2));
    }

    /// A region the engine did not read keeps its variable, so the count can
    /// never be reported as a confident zero.
    #[test]
    fn an_unread_region_survives_into_the_count() {
        let opaque = while_hole(0);
        let bound = Bound::sum([Bound::one(), opaque.as_bound()]);
        let charged = charge_calls(&bound, &BTreeSet::new()).unwrap();
        assert_eq!(charged, opaque.as_bound());
        assert_ne!(charged, Bound::zero());
    }

    /// The whole projection, over the shape a `while` produces: partial, and
    /// with no number offered.
    #[test]
    fn an_unread_region_is_partial_and_offers_no_number() {
        let opaque = while_hole(0);
        let derived = ResourceBound::queries_of(
            &TripCount::Partial {
                bound: Bound::sum([Bound::one(), opaque.as_bound()]),
                holes: vec![opaque],
                exact_elsewhere: true,
            },
            &accounted(),
        );
        assert_eq!(derived.kind, Some("partial"));
        assert_eq!(
            derived.value, None,
            "a count over an unanalysed region is not a total, and a gate must \
             not be handed one it can compare"
        );
    }

    /// A function the engine derived exactly, with no call in it, reports zero
    /// *as a number*. A known zero withheld is as wrong as an unknown one
    /// printed.
    #[test]
    fn a_fully_derived_function_with_no_call_reports_zero_exactly() {
        let derived =
            ResourceBound::queries_of(&TripCount::Exact(Bound::constant(4)), &accounted());
        assert_eq!(derived.value, Some(0));
        assert_eq!(derived.kind, Some("exact"));
    }

    /// An over-approximated bound reports an upper query count, never an exact
    /// one: the coefficient came out of arithmetic that was relaxed.
    #[test]
    fn an_approximated_bound_yields_an_upper_count() {
        let hole = call_hole(0);
        let derived = ResourceBound::queries_of(
            &TripCount::Partial {
                bound: Bound::sum([Bound::one(), hole.as_bound()]),
                holes: vec![hole],
                exact_elsewhere: false,
            },
            &accounted(),
        );
        assert_eq!(derived.kind, Some("upper"));
        assert_eq!(derived.value, Some(1));
    }

    /// **A concealed call withholds the number and the confident label.**
    ///
    /// The arithmetic is the same in both calls below - one call hole over a
    /// fully derived cost, which reads as a confident `1`. What differs is
    /// whether the frontend accounted for every call in the function. Where it
    /// did not, there is a call this bound has no coefficient for, so no
    /// arithmetic here could notice the `1` is low. `LAN-97`.
    #[test]
    fn a_concealed_call_is_partial_and_offers_no_number() {
        let shape = || {
            let hole = call_hole(0);
            TripCount::Partial {
                bound: Bound::sum([Bound::one(), hole.as_bound()]),
                holes: vec![hole],
                exact_elsewhere: true,
            }
        };
        let accounted = ResourceBound::queries_of(&shape(), &accounted());
        assert_eq!(accounted.kind, Some("exact"));
        assert_eq!(accounted.value, Some(1));

        let concealed = ResourceBound::queries_of(&shape(), &concealing());
        assert_eq!(
            concealed.kind,
            Some("partial"),
            "a count that cannot see one of the calls is not a complete claim"
        );
        assert_eq!(
            concealed.value, None,
            "this is the field a gate thresholds on, and a low number here is \
             what waves a function past its budget"
        );
    }

    /// **A concealed call does not withhold the count of a function with no
    /// call in it at all.** It still reports `partial` - the flag says a call
    /// was left unaccounted for, and this projection has no way to know the
    /// hidden one is somewhere else - but the direction is checked: the number
    /// is withheld, never invented.
    #[test]
    fn a_concealed_call_never_raises_a_count() {
        let concealed =
            ResourceBound::queries_of(&TripCount::Exact(Bound::constant(4)), &concealing());
        assert_eq!(concealed.kind, Some("partial"));
        assert_eq!(concealed.value, None);
        assert_eq!(
            concealed.bound.as_deref(),
            Some("0"),
            "the expression is still what the arithmetic found; it is the \
             *label* that stops calling it complete"
        );
    }

    /// Nothing derived is three absences, and never a zero.
    #[test]
    fn an_underived_function_reports_no_number() {
        let derived = ResourceBound::queries_of(&TripCount::Unknown, &accounted());
        assert_eq!(derived.value, None);
        assert_eq!(derived.kind, None);
        assert_eq!(derived.bound, None);
    }

    /// A symbolic count is reported as an expression and not rounded to a
    /// number: `for i in range(n): fetch(i)` issues `n` queries.
    #[test]
    fn a_symbolic_count_is_reported_symbolically() {
        let hole = call_hole(0);
        let body = Bound::sum([Bound::constant(2), hole.as_bound()]);
        let derived = ResourceBound::queries_of(
            &TripCount::Partial {
                bound: Bound::prod([Bound::var("n"), body]),
                holes: vec![hole],
                exact_elsewhere: true,
            },
            &accounted(),
        );
        assert_eq!(derived.bound.as_deref(), Some("n"));
        assert_eq!(derived.value, None);
        assert_eq!(derived.kind, Some("exact"));
    }
}
