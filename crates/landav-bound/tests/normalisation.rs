//! LAN-58 acceptance criteria, as executable checks.
//!
//! | LAN-58 AC | asserted by |
//! |---|---|
//! | 1. egg rewrite rules for `+`, `*`, `max` | [`the_frozen_rewrite_set_is_pinned`] |
//! | 2. idempotence of `max`, distribution of `*` over `+` | [`provably_equal_bounds_converge_on_one_normal_form`] |
//! | 3. deterministic normal form, across machines and under load | [`the_normal_form_is_identical_in_one_process`], [`the_normal_form_is_identical_in_a_separate_process`], [`a_run_longer_than_five_seconds_still_stops_on_a_count`] |
//! | 4. golden tests for printed output | [`the_printed_normal_form_is_golden`], [`the_cache_key_material_is_golden`] |
//! | 5. explicit iteration/node limit, no wall clock | [`the_frozen_budget_is_counts_only`], [`every_reachable_stop_reason_is_count_based`], [`a_run_longer_than_five_seconds_still_stops_on_a_count`] |
//! | 6. integer `Ord` cost with a unique tie-break | `normalise::tests` (in the crate), [`the_normal_form_is_identical_in_one_process`] |
//! | 7. no `egg::Symbol` in any type reaching extraction | [`the_normal_form_is_identical_in_a_separate_process`], [`interner_priming_does_not_move_the_normal_form`] |
//!
//! # Why two determinism tests and not one
//!
//! The two failure modes this lane exists to prevent are invisible to each
//! other. A process-global interner index (`egg::Symbol`) produces a *stable*
//! answer within one process and a different one in the next, so only the
//! cross-process diff sees it. A wall-clock stop produces a different answer
//! on the *same* machine depending on load, so only a run long enough to trip
//! the five-second default sees it. Neither test subsumes the other and
//! neither is optional.
//!
//! # Three different guarantees about the cache key, and why all three
//!
//! The determinism gates prove the cache-key material is the same as it was a
//! millisecond ago, in this process and in the next. They say nothing about
//! whether it is the same as it was *yesterday*, and F-008 depends on that:
//! a rewrite-set or cost-function change that shifts every key is exactly what
//! [`landav_bound::NORMAL_FORM_VERSION`] exists to catch, and a change to it
//! must invalidate a persisted cache rather than silently mis-serve it.
//! [`CACHE_KEY_GOLDENS`] pins the third guarantee.
//!
//! Both halves of that were checked against the implementation rather than
//! assumed: bumping `NORMAL_FORM_VERSION` moves all seventeen digests (it is
//! the prefix of `canonical_bytes`), and deleting a single rewrite rule moves
//! the affected ones. If either stops being true, this table has quietly
//! become decoration.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{process::Command, time::Duration, time::Instant};

use landav_bound::{
    Base, Bound, NormaliserBudget, NormaliserStop, normalise, normalise_with, rewrite_rule_names,
};

// ---------------------------------------------------------------------------
// the corpus
// ---------------------------------------------------------------------------

/// The environment variable that puts the in-process determinism test into
/// "emit" mode, so a child process can be asked for its digest.
const EMIT: &str = "LANDAV_NORMALISE_EMIT";

/// The name of the test a child process is asked to run.
const EMIT_TEST: &str = "the_normal_form_is_identical_in_one_process";

/// The prefix of a machine-readable digest line.
const MARKER: &str = "LANDAV-NORMAL";

/// The fixed corpus, with the printed form of the input and of its normal
/// form.
///
/// These are the golden values of acceptance criterion 4. Changing any of them
/// changes what a user reads *and* what an F-008 cache key hashes, so a change
/// here must be a deliberate normal-form change accompanied by a bump of
/// `NORMAL_FORM_VERSION`.
///
/// The variable names are chosen so that construction order and lexicographic
/// order disagree (`zeta` is built before `alpha`): under an interner-indexed
/// symbol the canonical order would follow construction and `max(alpha, zeta)`
/// would print the other way round.
fn corpus() -> Vec<(&'static str, Bound, &'static str, &'static str)> {
    let x0 = Bound::var("x0");
    let x1 = Bound::var("x1");
    let x2 = Bound::var("x2");
    // Built in the order that is *wrong* lexicographically, on purpose.
    let zeta = Bound::var("zeta");
    let alpha = Bound::var("alpha");

    vec![
        ("const", Bound::constant(42), "42", "42"),
        ("var", x0.clone(), "x0", "x0"),
        (
            "omega-absorbs",
            Bound::max_of([x0.clone(), Bound::omega()]),
            "omega",
            "omega",
        ),
        (
            "zero-times-var",
            Bound::prod([Bound::constant(0), x0.clone()]),
            "(0 * x0)",
            "(0 * x0)",
        ),
        (
            "factoring",
            Bound::sum([
                Bound::prod([x0.clone(), x1.clone()]),
                Bound::prod([x0.clone(), x2.clone()]),
            ]),
            "((x0 * x1) + (x0 * x2))",
            "(x0 * (x1 + x2))",
        ),
        (
            "already-factored",
            Bound::prod([Bound::constant(2), Bound::sum([x0.clone(), x1.clone()])]),
            "(2 * (x0 + x1))",
            "(2 * (x0 + x1))",
        ),
        (
            "three-way-factoring",
            Bound::sum([
                Bound::prod([x0.clone(), x0.clone()]),
                Bound::prod([x0.clone(), x1.clone()]),
                Bound::prod([x0.clone(), x2.clone()]),
            ]),
            "((x0 * x0) + (x0 * x1) + (x0 * x2))",
            "(x0 * (x0 + x1 + x2))",
        ),
        (
            "koat",
            Bound::prod([
                x1.clone(),
                Bound::sum([Bound::constant(2), Bound::log(Base::TWO, x1.clone())]),
            ]),
            "(x1 * (2 + log2(x1)))",
            "(x1 * (2 + log2(x1)))",
        ),
        (
            "koat-distributed",
            Bound::sum([
                Bound::prod([x1.clone(), Bound::constant(2)]),
                Bound::prod([x1.clone(), Bound::log(Base::TWO, x1.clone())]),
            ]),
            "((2 * x1) + (x1 * log2(x1)))",
            "(x1 * (2 + log2(x1)))",
        ),
        (
            "max-deduplicated-by-the-constructor",
            Bound::max_of([Bound::max_of([x0.clone(), x1.clone()]), x0.clone()]),
            "max(x0, x1)",
            "max(x0, x1)",
        ),
        (
            "max-idempotent-only-in-the-egraph",
            Bound::max_of([
                Bound::prod([x0.clone(), Bound::sum([x1.clone(), x2.clone()])]),
                Bound::sum([
                    Bound::prod([x0.clone(), x1.clone()]),
                    Bound::prod([x0.clone(), x2.clone()]),
                ]),
            ]),
            "max(((x0 * x1) + (x0 * x2)), (x0 * (x1 + x2)))",
            "(x0 * (x1 + x2))",
        ),
        (
            "max-of-products",
            Bound::max_of([
                Bound::prod([x0.clone(), x1.clone()]),
                Bound::prod([x0.clone(), x2.clone()]),
            ]),
            "max((x0 * x1), (x0 * x2))",
            "max((x0 * x1), (x0 * x2))",
        ),
        (
            "pow",
            Bound::pow(Base::TWO, Bound::sum([x0.clone(), x1.clone()])),
            "2^((x0 + x1))",
            "2^((x0 + x1))",
        ),
        (
            "log",
            Bound::log(Base::TEN, Bound::prod([x0.clone(), x1.clone()])),
            "log10((x0 * x1))",
            "log10((x0 * x1))",
        ),
        (
            "nested-factoring",
            Bound::sum([
                Bound::prod([
                    x0.clone(),
                    Bound::sum([x1.clone(), Bound::pow(Base::TWO, x2.clone())]),
                ]),
                Bound::prod([x0.clone(), x2.clone()]),
            ]),
            "((x0 * x2) + (x0 * (x1 + 2^(x2))))",
            "(x0 * (x1 + x2 + 2^(x2)))",
        ),
        (
            "mixed",
            Bound::sum([
                Bound::prod([x0.clone(), x1.clone(), x2.clone()]),
                Bound::prod([x0.clone(), x1.clone(), Bound::constant(3)]),
                Bound::max_of([x0.clone(), x1.clone()]),
                Bound::constant(7),
            ]),
            "(7 + max(x0, x1) + (3 * x0 * x1) + (x0 * x1 * x2))",
            "(7 + max(x0, x1) + (x0 * x1 * (3 + x2)))",
        ),
        (
            "construction-order-is-not-canonical-order",
            Bound::max_of([zeta, alpha]),
            "max(alpha, zeta)",
            "max(alpha, zeta)",
        ),
    ]
}

/// Hex, lower case, for a byte run.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// One digest line per corpus entry: the label, the printed normal form and
/// the hex of [`Bound::canonical_bytes`].
///
/// Both artefacts, not one. The printed form is what a user diffs and the
/// canonical bytes are what the F-008 cache key hashes, and it is entirely
/// possible for one to be stable while the other is not.
///
/// The result is **sorted**, so a caller may build the corpus in any order and
/// still compare the lines directly.
fn digest_lines() -> Vec<String> {
    let mut lines: Vec<String> = corpus()
        .into_iter()
        .map(|(label, bound, _, _)| {
            let form = match normalise(&bound) {
                Ok(form) => form,
                Err(error) => panic!("{label}: normalisation failed: {error}"),
            };
            assert!(
                NormaliserStop::ALL.contains(&form.stop()),
                "{label}: stop reason {} is not count based",
                form.stop()
            );
            format!(
                "{MARKER}\t{label}\t{}\t{}",
                form.bound(),
                hex(form.bound().canonical_bytes().as_bytes())
            )
        })
        .collect();
    lines.sort();
    lines
}

/// The 128-bit FNV-1a digest of a byte run, as 32 lower-case hex characters.
///
/// # Why a digest is written out here rather than pulled in
///
/// `landav-bound` has no hash dependency and
/// [`landav_bound::CacheKeyMaterial`] deliberately does **not** pick the
/// digest function, so that the choice is not frozen into the algebra's
/// dependency graph. A golden table needs *some* fixed function, and adding a
/// crate to the workspace to get one would freeze exactly what that decision
/// left open.
///
/// FNV-1a at 128 bits, with the published offset basis and prime, is the right
/// shape for the job: it is a dozen lines of `u128` arithmetic with no
/// dependency, it is bit-identical on every target, and 128 bits is the width
/// `CacheKeyMaterial`'s contract asks a real key to be.
///
/// **This is not a cryptographic hash and must not become one by accident.**
/// It is a tripwire on a fixed 17-entry table, where the only adversary is a
/// developer changing the rewrite set without noticing. A production F-008 key
/// still owes the `>= 128`-bit *cryptographic* digest that
/// `CacheKeyMaterial` requires - FNV is trivially collidable on chosen input,
/// and a persisted cache is exactly where chosen input arrives.
fn fnv1a_128_hex(bytes: &[u8]) -> String {
    // The published FNV-1a 128-bit parameters.
    const OFFSET_BASIS: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

    let mut hash = OFFSET_BASIS;
    for byte in bytes {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:032x}")
}

/// The pinned cache-key digest of each corpus entry's **normal form**.
///
/// # What this pins that the printed goldens do not
///
/// [`the_printed_normal_form_is_golden`] pins what a user reads.
/// [`the_normal_form_is_identical_in_one_process`] and its cross-process
/// sibling pin that the cache-key material is *stable between two runs*.
/// Neither pins that it is **what it was yesterday**, and F-008 depends on
/// that third guarantee: a rewrite-set or cost-function change that shifts
/// every key is precisely the event [`landav_bound::NORMAL_FORM_VERSION`]
/// exists to catch, and without this table nothing would fail.
///
/// The digest is over [`landav_bound::Bound::canonical_bytes`], which already
/// carries `NORMAL_FORM_VERSION` as its prefix - so a version bump moves every
/// line here, which is the intended and visible consequence.
///
/// **A diff in this table means the normal form moved.** That is a legitimate
/// thing to do, and it is never a legitimate thing to do *quietly*: re-record
/// these values only together with a bump of `NORMAL_FORM_VERSION`, because
/// every persisted F-008 entry keyed under the old form is now wrong.
/// Two lines here are *required* to equal two others, and
/// [`the_pinned_digests_agree_exactly_where_the_normal_forms_do`] enforces it:
/// `koat`/`koat-distributed` and
/// `factoring`/`max-idempotent-only-in-the-egraph` are each two ways of
/// writing one cost, so a digest recorded from the wrong run breaks that
/// relationship before it breaks anything else.
const CACHE_KEY_GOLDENS: &[(&str, &str)] = &[
    ("already-factored", "32e37b330b79c159c8a3a5bc1fbb01d7"),
    ("const", "c6f63d6745de6964dc73d08a260b6dfa"),
    (
        "construction-order-is-not-canonical-order",
        "9705ee59005e50b3ed9b4ddeafbe396f",
    ),
    ("factoring", "590108d6edf24368118d5f50ced83d00"),
    ("koat", "7622916edd683d0d2352a5668c43174f"),
    ("koat-distributed", "7622916edd683d0d2352a5668c43174f"),
    ("log", "9912100df9a8e127920e516a87b19a1b"),
    (
        "max-deduplicated-by-the-constructor",
        "b951207f7f002f5b25c087b047ae8843",
    ),
    (
        "max-idempotent-only-in-the-egraph",
        "590108d6edf24368118d5f50ced83d00",
    ),
    ("max-of-products", "ed1dc34d72119901b7ec03137db3509f"),
    ("mixed", "4b0f8e4bd72895d292960248a0f82ae4"),
    ("nested-factoring", "7d2e6028b5e3983eafb267845db9a8d1"),
    ("omega-absorbs", "bda3e96001ea800951c97783ee98fe27"),
    ("pow", "c7f6f3ae3debfdeffd8141dd42c8799e"),
    ("three-way-factoring", "79efa58c04b4b42160f9d666d57ad77d"),
    ("var", "1a393dcc034184342166686a4372ad15"),
    ("zero-times-var", "1f3a33281be603d3adcd6c40b054e95a"),
];

/// Interns a handful of names *through the normaliser*, in the given order.
///
/// A mirror language carrying `egg::Symbol` would order its e-class nodes by
/// interner index, which is assigned on first use. Priming with two different
/// orders and demanding the same answer is what turns that into a failure
/// rather than into a difference nobody sees until CI walks a directory in a
/// different order.
fn prime_interners(names: &[&str]) {
    for name in names {
        let probe = Bound::max_of([Bound::var(*name), Bound::var("zzzz")]);
        let _ = normalise(&probe);
    }
}

// ---------------------------------------------------------------------------
// AC1, AC5: the frozen configuration
// ---------------------------------------------------------------------------

/// The rewrite set is part of the normal form. Adding, removing or reordering
/// a rule changes what is extracted, therefore every golden and every
/// persisted cache key - so it fails here first.
#[test]
fn the_frozen_rewrite_set_is_pinned() {
    assert_eq!(
        rewrite_rule_names(),
        vec![
            "add-comm",
            "add-assoc",
            "add-assoc-rev",
            "add-zero",
            "mul-comm",
            "mul-assoc",
            "mul-assoc-rev",
            "mul-one",
            "max-comm",
            "max-assoc",
            "max-assoc-rev",
            "max-idem",
            "max-zero",
            "mul-distributes-over-add",
            "add-factors-through-mul",
        ],
        "the frozen rewrite set changed; that is a normal-form change and needs \
         a NORMAL_FORM_VERSION bump"
    );
}

/// The rule set must not contain the two rewrites the algebra does not
/// survive, and the check is behavioural rather than a reading of the table:
/// `?a * 0 -> 0` is unsound because variables range over `N u {omega}` and
/// `omega` absorbs unconditionally, so the product is `omega` at `?a = omega`.
#[test]
fn the_unsound_zero_product_rewrite_is_absent() {
    let term = Bound::prod([Bound::constant(0), Bound::var("x0")]);
    let form = normalise(&term).expect("normalisation must not fail");
    assert_ne!(
        form.bound(),
        &Bound::zero(),
        "`?a * 0 -> 0` folded a symbolic product to zero; at x0 = omega the \
         product is omega, so this launders an unblamed omega into a proved zero"
    );
    assert_eq!(form.bound(), &term, "nothing in the rule set applies here");
}

/// Both limits are counts, and they are the frozen ones. A duration here would
/// be the whole defect.
#[test]
fn the_frozen_budget_is_counts_only() {
    assert_eq!(NormaliserBudget::FROZEN.iter_limit(), 60);
    assert_eq!(NormaliserBudget::FROZEN.node_limit(), 10_000);
}

/// All three count-based stop reasons must be reachable, and none of them may
/// be an error. An unreachable stop path is where a silently less-normalised
/// bound would hide.
#[test]
fn every_reachable_stop_reason_is_count_based() {
    let vars: Vec<Bound> = (0..5).map(|i| Bound::var(format!("v{i}"))).collect();
    let mut terms = Vec::new();
    for left in &vars {
        for right in &vars {
            terms.push(Bound::prod([left.clone(), right.clone()]));
        }
    }
    let wide = Bound::sum(terms);

    let saturated = normalise(&Bound::var("q")).expect("a leaf must saturate");
    assert_eq!(saturated.stop(), NormaliserStop::Saturated);

    let capped_iterations = normalise_with(&wide, NormaliserBudget::new(1, 1_000_000))
        .expect("an iteration cap is a deterministic stop, not an error");
    assert_eq!(capped_iterations.stop(), NormaliserStop::IterationLimit);

    let capped_nodes = normalise_with(&wide, NormaliserBudget::new(60, 50))
        .expect("a node cap is a deterministic stop, not an error");
    assert_eq!(capped_nodes.stop(), NormaliserStop::NodeLimit);

    // Exhaustive over the type, so a fourth stop reason is a compile error
    // here rather than a review item.
    for stop in NormaliserStop::ALL {
        let named = match stop {
            NormaliserStop::Saturated => "saturated",
            NormaliserStop::IterationLimit => "iteration-limit",
            NormaliserStop::NodeLimit => "node-limit",
        };
        assert_eq!(stop.as_str(), named);
        assert_eq!(
            format!("{stop}"),
            named,
            "`Display` and `as_str` must agree; a report reads one and a test reads the other"
        );
    }
}

/// The run counters a caller uses to decide whether a normal form is safe to
/// persist must report the run that actually happened.
///
/// `NormaliserStop::Saturated` alone does not say how much work it took, and a
/// caller sizing a cache or a CI budget reads these. A constant here would be
/// invisible: every determinism assertion in this file would still pass.
#[test]
fn the_run_counters_describe_the_run() {
    let leaf = normalise(&Bound::var("q")).expect("a leaf must normalise");
    assert_eq!(
        leaf.iterations(),
        1,
        "a leaf saturates on the first iteration"
    );
    assert_eq!(leaf.egraph_nodes(), 1, "a leaf is one e-node");

    let x0 = Bound::var("x0");
    let x1 = Bound::var("x1");
    let x2 = Bound::var("x2");
    let factorable = Bound::sum([
        Bound::prod([x0.clone(), x0.clone()]),
        Bound::prod([x0.clone(), x1.clone()]),
        Bound::prod([x0.clone(), x2.clone()]),
    ]);
    let worked = normalise(&factorable).expect("normalisation must not fail");
    assert!(
        worked.iterations() > leaf.iterations(),
        "a term that needs rewriting must take more iterations than a leaf, got {}",
        worked.iterations()
    );
    assert!(
        worked.egraph_nodes() > leaf.egraph_nodes(),
        "a term of many nodes must leave more e-nodes than a leaf, got {}",
        worked.egraph_nodes()
    );

    // The node limit is a bound on this number, so the two must be related.
    let capped = normalise_with(&factorable, NormaliserBudget::new(60, 8))
        .expect("a node cap is a deterministic stop, not an error");
    assert_eq!(capped.stop(), NormaliserStop::NodeLimit);
    assert!(
        capped.egraph_nodes() > 8,
        "the run stopped because the e-graph outgrew the cap, so it must report more \
         than the cap, got {}",
        capped.egraph_nodes()
    );
}

/// **The wall-clock regression test.**
///
/// `egg`'s `Runner` defaults to a five-second time limit and checks it
/// *before* the node and iteration limits, so a run that takes longer than
/// that stops on `StopReason::TimeLimit` and extracts a less-normalised term.
/// This is the only test in the suite that actually spends longer than five
/// seconds inside one `normalise` call, and it is therefore the only one that
/// would notice if `.with_time_limit(Duration::MAX)` were dropped: with the
/// default in place, this call returns
/// `Err(BoundError::NonDeterministicNormalisation)`.
///
/// It is deliberately expensive - ten seconds or so in a debug build - and
/// that cost is the price of the acceptance criterion. It is also
/// **self-calibrating** rather than tuned to one machine: a term that finishes
/// inside the threshold proves nothing, so the loop grows the term until one
/// does not. A fixed term would go quietly vacuous on a faster machine, which
/// is precisely the class of silent failure this lane exists to prevent, and
/// a fixed term tuned to be slow enough *here* would be flaky rather than
/// vacuous. Neither is acceptable for the criterion that matters most.
#[test]
fn a_run_longer_than_five_seconds_still_stops_on_a_count() {
    // Comfortably clear of the five-second default, so a scheduling hiccup on
    // either side of the boundary cannot decide the outcome.
    let threshold = Duration::from_secs(6);
    // `variables^2` products in one sum. Each step up is roughly a doubling of
    // the work, so the loop reaches the threshold in one or two rounds and
    // cannot run away.
    let mut variables = 7usize;
    loop {
        let vars: Vec<Bound> = (0..variables)
            .map(|i| Bound::var(format!("w{i}")))
            .collect();
        let mut terms = Vec::new();
        for left in &vars {
            for right in &vars {
                terms.push(Bound::prod([left.clone(), right.clone()]));
            }
        }
        let wide = Bound::sum(terms);

        let started = Instant::now();
        // Deliberately NOT the frozen budget. `FROZEN`'s node limit is 10 000,
        // which caps a run at a few hundred milliseconds, so the wall clock can
        // never be the binding constraint there and this proof is unreachable at
        // it. The claim under test is about the `Runner`'s *configuration* --
        // that `.with_time_limit(Duration::MAX)` is set -- and that
        // configuration is shared by every budget. Raising the node limit here
        // is what lets the run cross five seconds so the claim can be tested at
        // all; it is the same escape hatch `NormaliserBudget::new` documents,
        // used for exactly the reason it documents.
        let form = normalise_with(&wide, NormaliserBudget::new(60, 100_000)).expect(
            "a run longer than egg's five-second default must still stop on a count; \
             an error here means the Runner still carries a wall-clock limit",
        );
        let elapsed = started.elapsed();

        assert!(
            NormaliserStop::ALL.contains(&form.stop()),
            "stop reason {} is not count based",
            form.stop()
        );
        if elapsed >= threshold {
            // The proof: this single `normalise` call spent longer inside the
            // runner than `egg`'s default time limit allows, and still stopped
            // on a count.
            return;
        }
        variables += 1;
        assert!(
            variables <= 11,
            "no term up to 121 products took longer than {threshold:?} to normalise, so \
             this test no longer exercises egg's wall-clock default at all. Raise the \
             ceiling rather than deleting the assertion."
        );
    }
}

// ---------------------------------------------------------------------------
// AC2: the laws
// ---------------------------------------------------------------------------

/// Bounds that the algebra proves equal must reach the same normal form.
///
/// This is acceptance criterion 2 stated as an observable: distribution of `*`
/// over `+` and idempotence of `max` are only worth having if two ways of
/// writing one cost converge.
#[test]
fn provably_equal_bounds_converge_on_one_normal_form() {
    let x0 = Bound::var("x0");
    let x1 = Bound::var("x1");
    let x2 = Bound::var("x2");

    let pairs: Vec<(&str, Bound, Bound)> = vec![
        (
            "distribution of * over +",
            Bound::prod([x0.clone(), Bound::sum([x1.clone(), x2.clone()])]),
            Bound::sum([
                Bound::prod([x0.clone(), x1.clone()]),
                Bound::prod([x0.clone(), x2.clone()]),
            ]),
        ),
        (
            "distribution with a literal",
            Bound::prod([Bound::constant(3), Bound::sum([x0.clone(), x1.clone()])]),
            Bound::sum([
                Bound::prod([Bound::constant(3), x0.clone()]),
                Bound::prod([Bound::constant(3), x1.clone()]),
            ]),
        ),
        (
            "distribution under a transcendental",
            Bound::prod([
                x1.clone(),
                Bound::sum([Bound::constant(2), Bound::log(Base::TWO, x1.clone())]),
            ]),
            Bound::sum([
                Bound::prod([x1.clone(), Bound::constant(2)]),
                Bound::prod([x1.clone(), Bound::log(Base::TWO, x1.clone())]),
            ]),
        ),
        (
            "idempotence of max over two provably equal operands",
            Bound::max_of([
                Bound::prod([x0.clone(), Bound::sum([x1.clone(), x2.clone()])]),
                Bound::sum([
                    Bound::prod([x0.clone(), x1.clone()]),
                    Bound::prod([x0.clone(), x2.clone()]),
                ]),
            ]),
            Bound::prod([x0.clone(), Bound::sum([x1.clone(), x2.clone()])]),
        ),
    ];

    for (label, left, right) in pairs {
        let normalised_left = normalise(&left).expect("normalisation must not fail");
        let normalised_right = normalise(&right).expect("normalisation must not fail");
        assert_eq!(
            normalised_left.stop(),
            NormaliserStop::Saturated,
            "{label}: the left side did not saturate, so the comparison is not \
             a statement about the normal form"
        );
        assert_eq!(
            normalised_right.stop(),
            NormaliserStop::Saturated,
            "{label}: the right side did not saturate"
        );
        assert_eq!(
            normalised_left.bound(),
            normalised_right.bound(),
            "{label}: `{left}` and `{right}` denote the same cost but normalised to \
             `{}` and `{}`",
            normalised_left.bound(),
            normalised_right.bound()
        );
        assert_eq!(
            normalised_left.bound().canonical_bytes().as_bytes(),
            normalised_right.bound().canonical_bytes().as_bytes(),
            "{label}: equal terms produced different cache-key material"
        );
    }
}

// ---------------------------------------------------------------------------
// AC4: goldens
// ---------------------------------------------------------------------------

/// The printed form of every corpus entry, before and after.
///
/// `Bound`'s `Display` renders in canonical operand order and there is
/// deliberately no second presentation order, so this pins the one string a
/// user will ever see.
#[test]
fn the_printed_normal_form_is_golden() {
    for (label, bound, printed_input, printed_normal_form) in corpus() {
        assert_eq!(
            format!("{bound}"),
            printed_input,
            "{label}: the *input* no longer prints as the golden expects, so the \
             golden below is pinning a different term than it was written for"
        );
        let form = normalise(&bound).expect("normalisation must not fail");
        assert_eq!(
            format!("{}", form.bound()),
            printed_normal_form,
            "{label}: normal form changed"
        );
    }
}

/// The cache-key material of every corpus entry's normal form, pinned.
///
/// See [`CACHE_KEY_GOLDENS`] for what this pins that the determinism gates do
/// not: they prove the key is the same as it was a millisecond ago, and this
/// proves it is the same as it was at the commit that recorded it.
#[test]
fn the_cache_key_material_is_golden() {
    let entries = corpus();

    // Neither table may silently grow past the other. A corpus entry with no
    // pinned digest is an unpinned normal form, and a pinned digest with no
    // corpus entry is a golden that stopped being checked - and both of those
    // are green by default, which is how a golden table rots.
    assert_eq!(
        CACHE_KEY_GOLDENS.len(),
        entries.len(),
        "every corpus entry needs exactly one pinned digest"
    );
    for (index, (label, _)) in CACHE_KEY_GOLDENS.iter().enumerate() {
        assert!(
            !CACHE_KEY_GOLDENS[..index]
                .iter()
                .any(|(seen, _)| seen == label),
            "`{label}` is pinned twice; the second entry would never be reached"
        );
    }

    for (label, bound, _, _) in entries {
        let Some((_, expected)) = CACHE_KEY_GOLDENS.iter().find(|(name, _)| *name == label) else {
            panic!(
                "corpus entry `{label}` has no pinned cache-key digest; add one to \
                 CACHE_KEY_GOLDENS rather than removing it from the corpus"
            );
        };
        let form = normalise(&bound).expect("normalisation must not fail");
        let digest = fnv1a_128_hex(form.bound().canonical_bytes().as_bytes());
        assert_eq!(
            &digest.as_str(),
            expected,
            "{label}: the cache-key material of `{}` moved. Every persisted F-008 entry \
             keyed under the old normal form is now wrong, so this is only a legitimate \
             change together with a bump of NORMAL_FORM_VERSION.",
            form.bound()
        );
    }
}

/// Two corpus entries that normalise to the same term must produce the same
/// pinned digest, and entries that do not must not.
///
/// This is what stops [`CACHE_KEY_GOLDENS`] from being seventeen unrelated
/// magic strings: `koat` and `koat-distributed` are written differently and
/// normalise to one term, as do `factoring` and
/// `max-idempotent-only-in-the-egraph`, so their digests are *required* to
/// coincide - and a digest recorded from the wrong run would break that
/// relationship before it broke anything else.
#[test]
fn the_pinned_digests_agree_exactly_where_the_normal_forms_do() {
    let normal_forms: Vec<(&'static str, String, String)> = corpus()
        .into_iter()
        .map(|(label, bound, _, _)| {
            let form = normalise(&bound).expect("normalisation must not fail");
            let digest = fnv1a_128_hex(form.bound().canonical_bytes().as_bytes());
            (label, format!("{}", form.bound()), digest)
        })
        .collect();

    for (left_label, left_form, left_digest) in &normal_forms {
        for (right_label, right_form, right_digest) in &normal_forms {
            assert_eq!(
                left_form == right_form,
                left_digest == right_digest,
                "`{left_label}` and `{right_label}` normalise to `{left_form}` and \
                 `{right_form}`, which does not match whether their digests agree"
            );
        }
    }

    // The two pairs the corpus was built to contain, named so that deleting
    // either entry is a failure rather than a quietly weaker test.
    let digest_of = |wanted: &str| -> String {
        normal_forms
            .iter()
            .find(|(label, _, _)| *label == wanted)
            .map(|(_, _, digest)| digest.clone())
            .unwrap_or_else(|| panic!("the corpus no longer contains `{wanted}`"))
    };
    assert_eq!(digest_of("koat"), digest_of("koat-distributed"));
    assert_eq!(
        digest_of("factoring"),
        digest_of("max-idempotent-only-in-the-egraph")
    );
}

/// Normalising a normal form must be the identity. Anything else means the
/// extracted term is not a fixpoint of the rewrite set, and two callers who
/// normalise a different number of times would get different cache keys.
#[test]
fn the_normal_form_is_a_fixpoint() {
    for (label, bound, _, _) in corpus() {
        let once = normalise(&bound).expect("normalisation must not fail");
        let twice = normalise(once.bound()).expect("normalisation must not fail");
        assert_eq!(
            once.bound(),
            twice.bound(),
            "{label}: normalising twice moved the term"
        );
        assert_eq!(
            once.bound().canonical_bytes().as_bytes(),
            twice.bound().canonical_bytes().as_bytes(),
            "{label}: normalising twice moved the cache key"
        );
    }
}

// ---------------------------------------------------------------------------
// AC3, AC7: determinism
// ---------------------------------------------------------------------------

/// Normalises the corpus twice in one process, with the corpus built in
/// opposite orders and the interner primed differently in between, and
/// byte-diffs both the rendered bound and the canonical bytes.
///
/// Under `LANDAV_NORMALISE_EMIT` it also prints the digest so that
/// [`the_normal_form_is_identical_in_a_separate_process`] can ask a child
/// process for it. It is a real test in both modes; the environment variable
/// only adds the printing.
#[test]
fn the_normal_form_is_identical_in_one_process() {
    prime_interners(&["x2", "x1", "x0", "zeta", "alpha"]);
    let first = digest_lines();

    // A *different* interning order, then the whole corpus rebuilt from
    // scratch. Nothing content derived can notice; an interner index would.
    prime_interners(&["alpha", "zeta", "x0", "x1", "x2"]);
    let second = digest_lines();

    assert_eq!(
        first, second,
        "the normal form moved between two runs in the same process"
    );

    if std::env::var_os(EMIT).is_some() {
        for line in &first {
            println!("{line}");
        }
    }
}

/// The same corpus, normalised in a **separate operating-system process**.
///
/// This is the only test that can see a process-global interner. `egg::Symbol`
/// is `symbol_table::GlobalSymbol`, whose `Ord` is an index assigned on first
/// use; within one process that index is stable, so an in-process diff is
/// blind to it. Because `landav check src/` walks directories in `read_dir`
/// order, two runs can intern the same two variables in the opposite order and
/// extract `max(n, m)` differently.
#[test]
fn the_normal_form_is_identical_in_a_separate_process() {
    if std::env::var_os(EMIT).is_some() {
        // A child never spawns a grandchild.
        return;
    }
    let exe = std::env::current_exe().expect("the test binary must have a path");
    let output = Command::new(&exe)
        .args(["--exact", EMIT_TEST, "--nocapture", "--test-threads=1"])
        .env(EMIT, "1")
        .output()
        .expect("the test binary must be runnable as a child process");
    assert!(
        output.status.success(),
        "the child process failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // `find` rather than `starts_with`: libtest writes `test <name> ... `
    // without a trailing newline, so the *first* emitted line arrives with the
    // harness's own prefix in front of it. A `starts_with` filter silently
    // drops exactly one corpus entry, which is the kind of quiet under-check
    // this whole file exists to prevent.
    let child: Vec<String> = stdout
        .lines()
        .filter_map(|line| line.find(MARKER).map(|at| line[at..].to_owned()))
        .collect();

    let parent = digest_lines();
    assert_eq!(
        child.len(),
        parent.len(),
        "the child emitted {} digest lines and the parent has {}; the child's \
         output was:\n{stdout}",
        child.len(),
        parent.len()
    );
    assert_eq!(
        child, parent,
        "the normal form differs between two processes running the same binary \
         on the same input. That is the signature of a process-global interner \
         index reaching extraction."
    );
}

/// Priming the interner in a different order must not move a single answer.
///
/// Narrower and faster than the cross-process test, and it fails with a much
/// more direct message: it isolates *interning order* from everything else a
/// second process changes.
#[test]
fn interner_priming_does_not_move_the_normal_form() {
    let term = Bound::max_of([Bound::var("nnn"), Bound::var("mmm")]);

    prime_interners(&["nnn", "mmm"]);
    let forward = normalise(&term).expect("normalisation must not fail");

    prime_interners(&["mmm", "nnn"]);
    let backward = normalise(&term).expect("normalisation must not fail");

    assert_eq!(forward.bound(), backward.bound());
    assert_eq!(
        format!("{}", forward.bound()),
        "max(mmm, nnn)",
        "the operand order must follow the names, not the order they were first seen"
    );
    assert_eq!(
        forward.bound().canonical_bytes().as_bytes(),
        backward.bound().canonical_bytes().as_bytes()
    );
}

/// The budget is part of the normal form, so two different budgets are allowed
/// to disagree - but each must be reproducible on its own.
#[test]
fn a_lowered_budget_is_still_reproducible() {
    let vars: Vec<Bound> = (0..5).map(|i| Bound::var(format!("v{i}"))).collect();
    let mut terms = Vec::new();
    for left in &vars {
        for right in &vars {
            terms.push(Bound::prod([left.clone(), right.clone()]));
        }
    }
    let wide = Bound::sum(terms);
    let budget = NormaliserBudget::new(3, 2_000);

    let first = normalise_with(&wide, budget).expect("normalisation must not fail");
    let second = normalise_with(&wide, budget).expect("normalisation must not fail");
    assert_eq!(first.stop(), second.stop());
    assert_eq!(first.egraph_nodes(), second.egraph_nodes());
    assert_eq!(
        first.bound().canonical_bytes().as_bytes(),
        second.bound().canonical_bytes().as_bytes()
    );
}
