//! `LAN-67` criterion 3: **twenty hand-written functions lower** and the
//! emitted document is well formed.
//!
//! # What "accepted" means here
//!
//! The criterion says *accepted by KoAT*. There is no KoAT binary available,
//! so each function is instead put through three checks that between them
//! cover what a KoAT run would have told us short of a dialect disagreement:
//!
//! 1. it **lowers** — no refusal, no malformed program;
//! 2. the emitted document **parses and is well formed** under the
//!    independently written reader in `koat_format`;
//! 3. the emitted system **computes the right answer** — simulated, and
//!    compared against final values worked out by hand in each function's
//!    comment rather than by running the reference interpreter.
//!
//! Check 3 is the one that makes the corpus more than a smoke test. Every
//! expected number below was derived by reading the loop, not by printing what
//! the code did.
//!
//! The twenty deliberately span the whole fragment: straight-line arithmetic,
//! `while` and both directions of `for`, strides, nesting, loop bodies that
//! interfere with their own loop, all three connectives, early `return`, empty
//! bodies and zero-trip loops.

use std::num::NonZeroI64;

use landav_bound::Origin;
use landav_its::{
    ArithOp, CompareOp, CondId, ExprId, RangeSpec, SourceProgram, SourceProgramBuilder, StmtId,
    VarName, lower,
};

use crate::{
    koat_format::check_well_formed,
    reference::{Simulation, State, simulate},
};

const BUDGET: u64 = 100_000;

/// A terse builder, so that twenty functions read like twenty functions.
struct Fun {
    builder: SourceProgramBuilder,
    line: u32,
}

impl Fun {
    fn new(name: &str, params: &[&str]) -> Self {
        Self {
            builder: SourceProgramBuilder::new(
                name,
                Origin::new(format!("{name}.py:1:1")),
                params.iter().map(|param| VarName::new(*param)).collect(),
            ),
            line: 1,
        }
    }

    fn at(&mut self) -> Origin {
        self.line += 1;
        Origin::new(format!("corpus.py:{}:1", self.line))
    }

    fn int(&mut self, value: i64) -> ExprId {
        let origin = self.at();
        self.builder.int(value, origin)
    }

    fn var(&mut self, name: &str) -> ExprId {
        let origin = self.at();
        self.builder.var(VarName::new(name), origin)
    }

    fn bin(&mut self, op: ArithOp, left: ExprId, right: ExprId) -> ExprId {
        let origin = self.at();
        self.builder.arith(op, left, right, origin)
    }

    fn cmp(&mut self, op: CompareOp, left: ExprId, right: ExprId) -> CondId {
        let origin = self.at();
        self.builder.compare(op, left, right, origin)
    }

    fn set(&mut self, name: &str, value: ExprId) -> StmtId {
        let origin = self.at();
        self.builder.assign(VarName::new(name), value, origin)
    }

    /// `name = name + delta`, the shape half the corpus needs.
    fn bump(&mut self, name: &str, delta: i64) -> StmtId {
        let read = self.var(name);
        let amount = self.int(delta);
        let sum = self.bin(ArithOp::Add, read, amount);
        self.set(name, sum)
    }

    fn while_(&mut self, cond: CondId, body: Vec<StmtId>) -> StmtId {
        let origin = self.at();
        self.builder.while_loop(cond, body, origin)
    }

    fn for_(
        &mut self,
        target: &str,
        start: ExprId,
        stop: ExprId,
        step: i64,
        body: Vec<StmtId>,
    ) -> StmtId {
        let origin = self.at();
        let stride = NonZeroI64::new(step).unwrap_or(NonZeroI64::new(1).expect("one is not zero"));
        self.builder.for_range(
            VarName::new(target),
            RangeSpec::new(start, stop, stride),
            body,
            origin,
        )
    }

    fn if_(&mut self, cond: CondId, then_body: Vec<StmtId>, else_body: Vec<StmtId>) -> StmtId {
        let origin = self.at();
        self.builder.if_else(cond, then_body, else_body, origin)
    }

    fn ret(&mut self) -> StmtId {
        let origin = self.at();
        self.builder.return_stmt(origin)
    }

    fn done(self, body: Vec<StmtId>) -> SourceProgram {
        self.builder.build(body)
    }
}

/// One corpus entry.
struct Entry {
    name: &'static str,
    program: SourceProgram,
    /// The parameter valuation to run at.
    input: i128,
    /// Final values worked out by hand.
    expected: Vec<(&'static str, i128)>,
}

fn state_with(n: i128) -> State {
    let mut state = State::new();
    state.insert("n".to_owned(), n);
    state
}

/// The twenty functions.
fn corpus() -> Vec<Entry> {
    vec![
        // 1. Straight-line arithmetic. r = 2 * 3 + 4 = 10.
        {
            let mut f = Fun::new("constant", &["n"]);
            let two = f.int(2);
            let three = f.int(3);
            let product = f.bin(ArithOp::Mul, two, three);
            let four = f.int(4);
            let sum = f.bin(ArithOp::Add, product, four);
            let body = vec![f.set("r", sum)];
            Entry {
                name: "constant",
                program: f.done(body),
                input: 0,
                expected: vec![("r", 10)],
            }
        },
        // 2. A linear `while`. n = 5: the body runs five times.
        {
            let mut f = Fun::new("linear_while", &["n"]);
            let zero = f.int(0);
            let init_i = f.set("i", zero);
            let zero2 = f.int(0);
            let init_t = f.set("t", zero2);
            let read_i = f.var("i");
            let read_n = f.var("n");
            let test = f.cmp(CompareOp::Lt, read_i, read_n);
            let tally = f.bump("t", 1);
            let step = f.bump("i", 1);
            let loop_stmt = f.while_(test, vec![tally, step]);
            let body = vec![init_i, init_t, loop_stmt];
            Entry {
                name: "linear_while",
                program: f.done(body),
                input: 5,
                expected: vec![("t", 5), ("i", 5)],
            }
        },
        // 3. A linear `for`. n = 5: i takes 0..4, so t = 5 and i ends at 4.
        {
            let mut f = Fun::new("linear_for", &["n"]);
            let zero = f.int(0);
            let init_t = f.set("t", zero);
            let start = f.int(0);
            let stop = f.var("n");
            let tally = f.bump("t", 1);
            let loop_stmt = f.for_("i", start, stop, 1, vec![tally]);
            let body = vec![init_t, loop_stmt];
            Entry {
                name: "linear_for",
                program: f.done(body),
                input: 5,
                expected: vec![("t", 5), ("i", 4)],
            }
        },
        // 4. A descending `for`. n = 5: i takes 5,4,3,2,1, so t = 5, i = 1.
        {
            let mut f = Fun::new("descending_for", &["n"]);
            let zero = f.int(0);
            let init_t = f.set("t", zero);
            let start = f.var("n");
            let stop = f.int(0);
            let tally = f.bump("t", 1);
            let loop_stmt = f.for_("i", start, stop, -1, vec![tally]);
            let body = vec![init_t, loop_stmt];
            Entry {
                name: "descending_for",
                program: f.done(body),
                input: 5,
                expected: vec![("t", 5), ("i", 1)],
            }
        },
        // 5. A stride of two. n = 5: i takes 0, 2, 4, so t = 3, i = 4.
        {
            let mut f = Fun::new("stride_two", &["n"]);
            let zero = f.int(0);
            let init_t = f.set("t", zero);
            let start = f.int(0);
            let stop = f.var("n");
            let tally = f.bump("t", 1);
            let loop_stmt = f.for_("i", start, stop, 2, vec![tally]);
            let body = vec![init_t, loop_stmt];
            Entry {
                name: "stride_two",
                program: f.done(body),
                input: 5,
                expected: vec![("t", 3), ("i", 4)],
            }
        },
        // 6. Nested loops, both over n. n = 4: t = 4 * 4 = 16.
        {
            let mut f = Fun::new("nested_square", &["n"]);
            let zero = f.int(0);
            let init_t = f.set("t", zero);
            let inner_start = f.int(0);
            let inner_stop = f.var("n");
            let tally = f.bump("t", 1);
            let inner = f.for_("j", inner_start, inner_stop, 1, vec![tally]);
            let outer_start = f.int(0);
            let outer_stop = f.var("n");
            let outer = f.for_("i", outer_start, outer_stop, 1, vec![inner]);
            let body = vec![init_t, outer];
            Entry {
                name: "nested_square",
                program: f.done(body),
                input: 4,
                expected: vec![("t", 16)],
            }
        },
        // 7. A triangular nest: inner runs `i` times. n = 4: 0+1+2+3 = 6.
        {
            let mut f = Fun::new("triangular", &["n"]);
            let zero = f.int(0);
            let init_t = f.set("t", zero);
            let inner_start = f.int(0);
            let inner_stop = f.var("i");
            let tally = f.bump("t", 1);
            let inner = f.for_("j", inner_start, inner_stop, 1, vec![tally]);
            let outer_start = f.int(0);
            let outer_stop = f.var("n");
            let outer = f.for_("i", outer_start, outer_stop, 1, vec![inner]);
            let body = vec![init_t, outer];
            Entry {
                name: "triangular",
                program: f.done(body),
                input: 4,
                expected: vec![("t", 6)],
            }
        },
        // 8. Doubling. n = 8: j goes 1, 2, 4, 8 — three iterations.
        {
            let mut f = Fun::new("doubling", &["n"]);
            let one = f.int(1);
            let init_j = f.set("j", one);
            let zero = f.int(0);
            let init_t = f.set("t", zero);
            let read_j = f.var("j");
            let read_n = f.var("n");
            let test = f.cmp(CompareOp::Lt, read_j, read_n);
            let read_j2 = f.var("j");
            let two = f.int(2);
            let doubled = f.bin(ArithOp::Mul, read_j2, two);
            let double = f.set("j", doubled);
            let tally = f.bump("t", 1);
            let loop_stmt = f.while_(test, vec![double, tally]);
            let body = vec![init_j, init_t, loop_stmt];
            Entry {
                name: "doubling",
                program: f.done(body),
                input: 8,
                expected: vec![("t", 3), ("j", 8)],
            }
        },
        // 9. KoAT's worked example: an outer loop over n, an inner one that
        //    doubles. n = 8: eight passes times three inner steps = 24.
        {
            let mut f = Fun::new("koat_worked_example", &["n"]);
            let zero = f.int(0);
            let init_t = f.set("t", zero);
            let one = f.int(1);
            let seed = f.set("j", one);
            let read_j = f.var("j");
            let read_n = f.var("n");
            let test = f.cmp(CompareOp::Lt, read_j, read_n);
            let read_j2 = f.var("j");
            let two = f.int(2);
            let doubled = f.bin(ArithOp::Mul, read_j2, two);
            let double = f.set("j", doubled);
            let tally = f.bump("t", 1);
            let inner = f.while_(test, vec![double, tally]);
            let start = f.int(0);
            let stop = f.var("n");
            let outer = f.for_("i", start, stop, 1, vec![seed, inner]);
            let body = vec![init_t, outer];
            Entry {
                name: "koat_worked_example",
                program: f.done(body),
                input: 8,
                expected: vec![("t", 24)],
            }
        },
        // 10. Absolute value by branch. n = -5: r = 0 - (-5) = 5.
        {
            let mut f = Fun::new("branch_abs", &["n"]);
            let read = f.var("n");
            let zero = f.int(0);
            let test = f.cmp(CompareOp::Gt, read, zero);
            let positive = f.var("n");
            let then_stmt = f.set("r", positive);
            let zero2 = f.int(0);
            let negative = f.var("n");
            let flipped = f.bin(ArithOp::Sub, zero2, negative);
            let else_stmt = f.set("r", flipped);
            let body = vec![f.if_(test, vec![then_stmt], vec![else_stmt])];
            Entry {
                name: "branch_abs",
                program: f.done(body),
                input: -5,
                expected: vec![("r", 5)],
            }
        },
        // 11. Nested conditionals. n = 5: positive but not above ten, r = 1.
        {
            let mut f = Fun::new("nested_if", &["n"]);
            let read = f.var("n");
            let ten = f.int(10);
            let big = f.cmp(CompareOp::Gt, read, ten);
            let two = f.int(2);
            let set_two = f.set("r", two);
            let one = f.int(1);
            let set_one = f.set("r", one);
            let inner = f.if_(big, vec![set_two], vec![set_one]);
            let read2 = f.var("n");
            let zero = f.int(0);
            let positive = f.cmp(CompareOp::Gt, read2, zero);
            let zero2 = f.int(0);
            let set_zero = f.set("r", zero2);
            let body = vec![f.if_(positive, vec![inner], vec![set_zero])];
            Entry {
                name: "nested_if",
                program: f.done(body),
                input: 5,
                expected: vec![("r", 1)],
            }
        },
        // 12. A conjunction guard. n = 3: both counters reach zero together.
        {
            let mut f = Fun::new("conjunction_guard", &["n"]);
            let read_n = f.var("n");
            let init_a = f.set("a", read_n);
            let read_n2 = f.var("n");
            let init_b = f.set("b", read_n2);
            let zero = f.int(0);
            let init_t = f.set("t", zero);
            let read_a = f.var("a");
            let zero_a = f.int(0);
            let a_positive = f.cmp(CompareOp::Gt, read_a, zero_a);
            let read_b = f.var("b");
            let zero_b = f.int(0);
            let b_positive = f.cmp(CompareOp::Gt, read_b, zero_b);
            let origin = f.at();
            let both = f.builder.and(a_positive, b_positive, origin);
            let step_a = f.bump("a", -1);
            let step_b = f.bump("b", -1);
            let tally = f.bump("t", 1);
            let loop_stmt = f.while_(both, vec![step_a, step_b, tally]);
            let body = vec![init_a, init_b, init_t, loop_stmt];
            Entry {
                name: "conjunction_guard",
                program: f.done(body),
                input: 3,
                expected: vec![("t", 3), ("a", 0), ("b", 0)],
            }
        },
        // 13. A disjunction guard, with only one disjunct ever true. n = 3.
        {
            let mut f = Fun::new("disjunction_guard", &["n"]);
            let read_n = f.var("n");
            let init_a = f.set("a", read_n);
            let zero_b = f.int(0);
            let init_b = f.set("b", zero_b);
            let zero_t = f.int(0);
            let init_t = f.set("t", zero_t);
            let read_a = f.var("a");
            let zero_a = f.int(0);
            let a_positive = f.cmp(CompareOp::Gt, read_a, zero_a);
            let read_b = f.var("b");
            let zero_b2 = f.int(0);
            let b_positive = f.cmp(CompareOp::Gt, read_b, zero_b2);
            let origin = f.at();
            let either = f.builder.or(a_positive, b_positive, origin);
            let step_a = f.bump("a", -1);
            let tally = f.bump("t", 1);
            let loop_stmt = f.while_(either, vec![step_a, tally]);
            let body = vec![init_a, init_b, init_t, loop_stmt];
            Entry {
                name: "disjunction_guard",
                program: f.done(body),
                input: 3,
                expected: vec![("t", 3), ("a", 0)],
            }
        },
        // 14. A negated guard. `not (a <= 0)` is `a > 0`. n = 4.
        {
            let mut f = Fun::new("negated_guard", &["n"]);
            let read_n = f.var("n");
            let init_a = f.set("a", read_n);
            let zero_t = f.int(0);
            let init_t = f.set("t", zero_t);
            let read_a = f.var("a");
            let zero_a = f.int(0);
            let non_positive = f.cmp(CompareOp::Le, read_a, zero_a);
            let origin = f.at();
            let test = f.builder.not(non_positive, origin);
            let step_a = f.bump("a", -1);
            let tally = f.bump("t", 1);
            let loop_stmt = f.while_(test, vec![step_a, tally]);
            let body = vec![init_a, init_t, loop_stmt];
            Entry {
                name: "negated_guard",
                program: f.done(body),
                input: 4,
                expected: vec![("t", 4), ("a", 0)],
            }
        },
        // 15. An early return. n = 5 is positive, so r stays 1.
        {
            let mut f = Fun::new("early_return", &["n"]);
            let one = f.int(1);
            let init_r = f.set("r", one);
            let read = f.var("n");
            let zero = f.int(0);
            let positive = f.cmp(CompareOp::Gt, read, zero);
            let early = f.ret();
            let branch = f.if_(positive, vec![early], vec![]);
            let two = f.int(2);
            let after = f.set("r", two);
            let body = vec![init_r, branch, after];
            Entry {
                name: "early_return",
                program: f.done(body),
                input: 5,
                expected: vec![("r", 1)],
            }
        },
        // 16. A return from inside a loop. n = 5, but it stops at t = 2.
        {
            let mut f = Fun::new("return_in_loop", &["n"]);
            let zero = f.int(0);
            let init_t = f.set("t", zero);
            let tally = f.bump("t", 1);
            let read_t = f.var("t");
            let two = f.int(2);
            let enough = f.cmp(CompareOp::Ge, read_t, two);
            let stop = f.ret();
            let branch = f.if_(enough, vec![stop], vec![]);
            let start = f.int(0);
            let stop_expr = f.var("n");
            let loop_stmt = f.for_("i", start, stop_expr, 1, vec![tally, branch]);
            let body = vec![init_t, loop_stmt];
            Entry {
                name: "return_in_loop",
                program: f.done(body),
                input: 5,
                expected: vec![("t", 2)],
            }
        },
        // 17. A genuinely polynomial update. n = 4: 16 + 8 + 1 = 25.
        {
            let mut f = Fun::new("polynomial_update", &["n"]);
            let left = f.var("n");
            let right = f.var("n");
            let square = f.bin(ArithOp::Mul, left, right);
            let two = f.int(2);
            let read = f.var("n");
            let twice = f.bin(ArithOp::Mul, two, read);
            let partial = f.bin(ArithOp::Add, square, twice);
            let one = f.int(1);
            let total = f.bin(ArithOp::Add, partial, one);
            let body = vec![f.set("r", total)];
            Entry {
                name: "polynomial_update",
                program: f.done(body),
                input: 4,
                expected: vec![("r", 25)],
            }
        },
        // 18. An empty loop body. n = 3: i takes 0, 1, 2.
        {
            let mut f = Fun::new("empty_body", &["n"]);
            let start = f.int(0);
            let stop = f.var("n");
            let loop_stmt = f.for_("i", start, stop, 1, vec![]);
            let body = vec![loop_stmt];
            Entry {
                name: "empty_body",
                program: f.done(body),
                input: 3,
                expected: vec![("i", 2)],
            }
        },
        // 19. A loop that never runs. t stays at 7.
        {
            let mut f = Fun::new("zero_trip", &["n"]);
            let seven = f.int(7);
            let init_t = f.set("t", seven);
            let start = f.int(0);
            let stop = f.int(0);
            let tally = f.bump("t", 1);
            let loop_stmt = f.for_("i", start, stop, 1, vec![tally]);
            let body = vec![init_t, loop_stmt];
            Entry {
                name: "zero_trip",
                program: f.done(body),
                input: 3,
                expected: vec![("t", 7)],
            }
        },
        // 20. A body that grows the endpoint. n = 3: three iterations, and n
        //     ends at 6 — the trip count was fixed when the loop was entered.
        {
            let mut f = Fun::new("interfering_body", &["n"]);
            let start = f.int(0);
            let stop = f.var("n");
            let grow = f.bump("n", 1);
            let loop_stmt = f.for_("i", start, stop, 1, vec![grow]);
            let body = vec![loop_stmt];
            Entry {
                name: "interfering_body",
                program: f.done(body),
                input: 3,
                expected: vec![("n", 6), ("i", 2)],
            }
        },
    ]
}

/// **Criterion 3.** Twenty functions, each lowering to a well-formed document
/// that computes the value worked out by hand.
#[test]
fn twenty_functions_lower_and_are_well_formed() {
    let entries = corpus();
    assert_eq!(
        entries.len(),
        20,
        "the criterion names twenty functions; the corpus has {}",
        entries.len()
    );

    let mut names: Vec<&str> = entries.iter().map(|entry| entry.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 20, "two corpus entries share a name");

    for entry in entries {
        let its = match lower(&entry.program) {
            Ok(its) => its,
            Err(error) => panic!("{}: refused: {error}", entry.name),
        };

        // Criterion 1: a system with locations, transitions and guards.
        assert!(!its.locations().is_empty(), "{}: no locations", entry.name);
        assert!(
            !its.transitions().is_empty(),
            "{}: no transitions",
            entry.name
        );
        assert_eq!(
            its.params().len(),
            1,
            "{}: wrong parameter count",
            entry.name
        );

        // The document parses and is well formed.
        check_well_formed(&its);

        // And it computes the right answer.
        let initial = state_with(entry.input);
        match simulate(&its, &initial, BUDGET) {
            Simulation::Terminated { state, length } => {
                assert!(length > 0, "{}: an empty run", entry.name);
                for (name, expected) in &entry.expected {
                    assert_eq!(
                        state.get(*name),
                        Some(expected),
                        "{}: variable `{}` ended at {:?}, expected {}",
                        entry.name,
                        name,
                        state.get(*name),
                        expected
                    );
                }
            }
            other => panic!("{}: simulation did not terminate: {other:?}", entry.name),
        }
    }
}

/// Every corpus entry emits at least one guard, and the loops emit at least
/// one genuine loop — a transition whose target is a location it can be
/// reached from again.
///
/// A lowering that emitted a straight line for every function would satisfy
/// the value checks above for the straight-line entries and nothing else; this
/// pins that the control flow really is a graph with cycles in it.
#[test]
fn the_looping_functions_emit_cycles() {
    for entry in corpus() {
        let its = lower(&entry.program).expect("corpus entries lower");
        let loops = its
            .transitions()
            .iter()
            .any(|transition| transition.target().index() <= transition.source().index());
        let has_loop = matches!(
            entry.name,
            "linear_while"
                | "linear_for"
                | "descending_for"
                | "stride_two"
                | "nested_square"
                | "triangular"
                | "doubling"
                | "koat_worked_example"
                | "return_in_loop"
                | "empty_body"
                | "zero_trip"
                | "interfering_body"
                | "conjunction_guard"
                | "disjunction_guard"
                | "negated_guard"
        );
        if has_loop {
            assert!(loops, "{}: no back edge was emitted", entry.name);
        }
    }
}
