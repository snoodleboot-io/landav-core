//! Integer transition system exporter.
//!
//! # Scope
//!
//! Component `C-07`. Feature [`F-006`], release R0, milestone M0.5.
//!
//! # Turning the problem into a lowering
//!
//! Lowers the numeric fragment — integer variables, loops, conditionals, no
//! containers — into KoAT's integer transition system format. This is what
//! turns milestone one from "build a complexity analyser" into "build a
//! lowering", following the Pico precedent of a domain frontend onto KoAT.
//!
//! The ITS export cannot represent containers or heap effects. That is what the
//! Landav IR (`F-009`, R1) is for; this crate deliberately handles only the
//! fragment KoAT can already reason about, which is how R0 produces bounds at
//! all within weeks.
//!
//! # How much real code this reaches: almost none
//!
//! Measured, not estimated. Across five corpora and ~26,800 functions — the
//! Python 3.12 standard library, a typed application backend with its vendored
//! dependencies, that backend's own source, numpy, and an HPC teaching suite —
//! **every bound derived was a constant.** Not one mentioned a parameter.
//!
//! | Corpus | Functions | Lowered |
//! |---|---:|---:|
//! | Python 3.12 stdlib | 3,057 | 14 (0.5%) |
//! | typed backend + dependencies | 14,952 | 804 (5.4%) |
//! | that backend's own code | 53 | 0 |
//! | numpy | 2,906 | 25 (0.9%) |
//! | HPC teaching suite | 5,858 | 190 (3.2%) |
//!
//! The functions that *do* lower are docstring-only bodies and `@overload`
//! stubs, which is why the bounds are `0` and `1`.
//!
//! This crate is correct on what it accepts. What it accepts is a fragment
//! real Python almost never sits inside, and that is a property of the
//! fragment rather than a defect in the lowering. Anyone deciding what to
//! build next should start from the refusal counts below rather than from how
//! interesting a construct is.
//!
//! # The fragment
//!
//! A frontend hands over a [`SourceProgram`]: one function, its integer
//! parameters, and a body built from
//!
//! * **assignment** to a single integer variable;
//! * **`if` / `else`**, including a missing `else`;
//! * **`while`**, with any condition in the language below;
//! * **counted `for`**, over an integer [`RangeSpec`] with a literal non-zero
//!   step;
//! * **`return`**, which carries no value because the emitted system models
//!   runtime rather than results;
//! * **arithmetic** — `+`, `-`, `*`, unary `-`, and `**` with a small literal
//!   non-negative exponent;
//! * **conditions** — the six integer comparisons, `and`, `or`, `not`.
//!
//! Loops nest arbitrarily. Nothing in that list mentions a source language:
//! `range` arrives as a half-open integer interval, truthiness arrives as an
//! explicit comparison against zero, and `x += 1` arrives already expanded.
//! Non-negotiable 4 is structural here rather than aspirational — the crate
//! graph runs `landav-python` → `landav-its`, so this crate could not see a
//! Python AST if it wanted to.
//!
//! # What is refused, and why each one
//!
//! Everything else, by name, through [`Construct`]. Objects, dynamic dispatch,
//! comprehensions, exceptions, containers, calls, division, bitwise operators
//! and every form of iteration that is not a counted range. There is no
//! catch-all "unsupported" — each refusal names a construct and a position,
//! which is what makes `LAN-68`'s coverage report possible and what stops a
//! bare "unknown" ever reaching a user.
//!
//! ## Which refusals actually cost coverage
//!
//! Occurrences across the two largest corpora above. **These are occurrences,
//! not functions** — one function refusing for several reasons appears in
//! several rows — so removing the top row alone would not lower the functions
//! beneath it.
//!
//! | Construct | stdlib | typed backend |
//! |---|---:|---:|
//! | **call** | 15,121 | 67,164 |
//! | **non-integer-value** | 13,765 | 52,812 |
//! | attribute | 2,397 | 12,984 |
//! | collection | 3,033 | 11,193 |
//! | exceptional-control-flow | 1,734 | 6,278 |
//! | complex-assignment-target | 1,256 | 5,559 |
//! | subscript | 1,015 | 2,804 |
//! | declaration | 557 | 2,681 |
//! | conditional-expression | 379 | 2,176 |
//! | comprehension | 246 | 2,050 |
//! | unbounded-iteration | 443 | 1,833 |
//! | coroutine | 69 | 721 |
//! | integer-division | 209 | — |
//! | bitwise-operator | 166 | — |
//! | loop-jump | 103 | — |
//!
//! `call` and `non-integer-value` are **72–80% of every refusal**, in both
//! corpora, in the same order. Everything else is rounding error against them,
//! and the two of them are what the Landav IR (`F-009`, R1) exists to address.
//!
//! Three of those deserve their reasoning recorded, because in each case
//! refusing was a *choice* over an available alternative:
//!
//! **Division and modulo** ([`Construct::IntegerDivision`]) are not
//! polynomial, so there is no [`Polynomial`] to lower them to. They are
//! nonetheless *exactly* encodable, and the encoding is worth writing down
//! because it is the obvious next extension: `q = a // b` for positive `b` is
//! a nondeterministic assignment to `q` guarded by `b*q <= a && a < b*q + b`,
//! which pins `q` to the single correct value. That needs guards over the
//! post-state, which the emitter does not yet write, so it is refused today
//! rather than approximated. `while n > 1: n = n // 2` is the canonical
//! logarithmic loop and this fragment cannot express it.
//!
//! This documentation previously called division "the most valuable single
//! construct to add next". **The measurement says otherwise**: division is
//! 209 refusals out of ~34,000, twelfth by frequency. The encoding above is
//! worth keeping and the example is a good one; the priority claim was a guess
//! and it was wrong.
//!
//! **`break` and `continue`** ([`Construct::LoopJump`]) are sound to support —
//! a `break` is a transition to the loop's exit location and a `continue` one
//! to its head, and the lowering already has both locations in hand. They are
//! refused because the story's fragment did not name them and a loop-context
//! stack is machinery this lane did not need for the KoAT worked example.
//! Cheap to add — and, like division, not what is holding coverage back:
//! `loop-jump` is 103 refusals.
//!
//! **A symbolic loop step** ([`Construct::UnboundedIteration`]) *could* be
//! over-approximated rather than refused: emitting the loop with no guard at
//! all admits every execution and is perfectly sound. It is refused because
//! the result is worthless — an unguarded loop does not terminate, so every
//! bound derived through it is `omega` — and a named refusal a coverage report
//! can count is more useful than a silent `omega` that looks like an answer.
//! That is the general rule this crate follows: **over-approximate when the
//! result is still informative, refuse when it would not be.**
//!
//! # Which direction each construct errs
//!
//! Soundness has a zero target, and the only safe error is to admit *more*
//! executions than the program can perform. Every construct in the fragment
//! is one of:
//!
//! | Construct | Direction | Why |
//! |---|---|---|
//! | assignment | **exact** | a polynomial update denotes the same function |
//! | arithmetic `+ - * **` | **exact** | closed in the polynomial semiring over `Z`; overflow refuses rather than wraps |
//! | comparison | **exact** | over `Z`, each comparison and its negation are both constraints |
//! | `and` / `or` / `not` | **exact** | normal form computed in both polarities; `!=` expands to a real disjunction |
//! | `if` / `else` | **exact** | the two branch guards are the two polarities |
//! | `while` | **exact** | head, body and exit, with the condition's two polarities |
//! | `for` over a range | **exact** | endpoints snapshotted, counter is fresh |
//! | `return` | **over-approximates** | the returned *value* is discarded; runtime is preserved exactly |
//! | condition past [`MAX_DNF_CLAUSES`] | **over-approximates** | widened to `true`, so both branches become available |
//! | a variable left unset by a zero-trip loop | **over-approximates** | keeps its prior value where the source would raise |
//!
//! Nothing in the fragment errs downwards. The two widenings are argued in
//! the `cond_dnf` documentation inside [`lowering`] and in [`RangeSpec`]'s,
//! and both are property-tested against an independently written reference
//! semantics rather than against the lowering itself.
//!
//! The one place transitions are *discarded* is a clause whose guard is
//! unsatisfiable on its face, such as `1 = 0`. Removing a transition no
//! execution can take removes no execution.
//!
//! # Refusal is all-or-nothing, and that is the soundness decision
//!
//! [`lower`] returns a whole [`Its`] or none at all. It never returns a system
//! built from the parts it understood, because such a system admits *fewer*
//! executions than the program has — the refused construct might have been a
//! loop — and a bound derived from it can be exceeded. That is the one failure
//! class with a zero target, so the refusal is total and every refused
//! construct in the program is reported at once rather than one per run.
//!
//! Refusal is also **structural rather than diligent**. [`lower`] scans the
//! whole arena for [`SourceExpr::Unsupported`] nodes instead of relying on the
//! traversal to reach them, so building one anywhere refuses the program —
//! attached to a statement or not, reachable or not. A frontend cannot lose a
//! refusal by forgetting to hang a node off something, which is the easiest
//! mistake in a translation to make and the hardest to notice: the program
//! would lower cleanly and the bound would silently omit whatever the node
//! stood for. That scan is [`SourceProgram::unsupported_nodes`], and it is
//! published so that a second consumer with the same obligation can be built on
//! the same implementation rather than on a second walk that drifts.
//!
//! One node kind is scanned and not refused: one carrying a [`DeclaredEffect`],
//! which is a frontend saying it can *account for* what it did not translate —
//! this costs a constant, and it rebinds no local of the frame it stands in.
//! [`lower`] emits an ordinary step for it. Nothing here checks the claim and
//! nothing here could; what this crate guarantees is that a node with nothing
//! declared refuses exactly as it always has, so the mechanism is an opt-in per
//! node and never a relaxation of the default.
//!
//! A refused node may also say what it can *change* without ceasing to be a
//! refusal: [`Writes`] names the locals it may rebind and whether it may mutate
//! an object. [`lower`] ignores it — the node still refuses the program — and
//! `landav-engine` reads it to decide which values survive the region, in place
//! of the construct's answer for its whole kind. Same discipline: a node that
//! says nothing inherits the kind's answer, which is the conservative one.
//!
//! ## All-or-nothing is a property of *this path*, not of the toolchain
//!
//! It is worth being exact about the scope, because it narrowed. All-or-nothing
//! is a statement about [`lower`] and about what [`Coverage::lowered`] counts:
//! either the whole program becomes an [`Its`] or none of it does, and there is
//! no partial system for a solver to be misled by.
//!
//! It is **not** a statement about whether anything can be said about a refused
//! program. `landav-engine` derives costs from [`SourceProgram`] directly,
//! without a transition system, and treats each `Unsupported` node as a *hole*:
//! a named variable standing for that region's cost, blamed on its
//! [`Construct`] and its position. The cost around the hole is still derived,
//! and an unfilled hole denotes `omega`, so nothing complete is ever claimed.
//!
//! That second consumer needs one thing of a refusal that [`lower`] does not:
//! how much of a source statement the node stands for. A frontend has nowhere
//! but a statement of its own to record something unanalysable inside a `return`
//! or a refused assignment, so the same node kind arrives for "this line is a
//! call" and for "there is a call inside the line beside this one" - and a
//! consumer charging one step per statement must tell them apart. [`Extent`]
//! is that field. It is inert on this path: [`lower`] refuses either way.
//! A function whose only obstacle is a call therefore has no transition system
//! and does have a partial bound naming the call - two different answers to two
//! different questions, and this crate answers only the first.
//!
//! This crate gains no representation for unknown cost from that. [`Cost`] stays
//! a polynomial, [`Update`] stays a total map with no havoc, and a call still
//! refuses - because a call has an unknown *effect* on the integer state as well
//! as an unknown value, and there is no sound over-approximation of an unknown
//! effect.
//!
//! # The coverage report
//!
//! `LAN-67` built the diagnostic *vocabulary* and its collection: [`Construct`]
//! is the named set, [`Unsupported`] is one record with a position, [`Refusals`]
//! is the non-empty ordered ledger, and [`Construct::all`] enumerates the
//! vocabulary so a report can list the constructs that were **not** hit as well
//! as the ones that were.
//!
//! `LAN-68` is [`Coverage`], which turns that into a report over a whole run.
//! It accumulates across units and across files, ranks the constructs by how
//! often each one blocked the lowering, keeps a malformed program apart from a
//! language construct, and carries a percentage that **cannot reach 100 unless
//! every unit lowered**.
//!
//! Refusing one unit loudly is not enough on its own. The failure mode one
//! level up is that four functions out of five refused, the report named the
//! fifth, and the reader concluded the file was analysed — so every accessor on
//! [`Coverage`] exists to keep the denominator in view. The ratio is over
//! *units*, not statements: refusal is all-or-nothing per unit, and "90% of the
//! statements lowered" would describe a function that produced no transitions
//! at all as nearly analysed.
//!
//! [`Refusals::blames`] and [`LoweringError::blames`] are the other half of
//! the seam, pointing at `F-015`: they turn this crate's vocabulary into
//! [`landav_bound::Blames`] without this crate learning what a bound is.
//!
//! # Edition
//!
//! OSS — Apache-2.0, ships in `landav-core`.
//!
//! [`F-006`]: https://linear.app/snoodleboot/issue/LAN-6

#![doc(html_root_url = "https://docs.rs/landav-its")]
#![forbid(unsafe_code)]

pub mod arith_op;
pub mod compare_op;
pub mod cond_id;
pub mod constraint;
pub mod construct;
pub mod cost;
pub mod cost_effect;
pub mod coverage;
pub mod declared_effect;
pub mod expr_id;
pub mod extent;
pub mod guard;
pub mod its;
pub mod its_var;
pub mod koat;
pub mod location;
pub mod location_id;
pub mod lowering;
pub mod lowering_error;
pub mod monomial;
pub mod node_id;
pub mod polynomial;
pub mod range_spec;
pub mod refusals;
pub mod relation;
pub mod source_cond;
pub mod source_expr;
pub mod source_program;
pub mod source_program_builder;
pub mod source_stmt;
pub mod stmt_id;
pub mod transition;
pub mod unsupported;
pub mod unsupported_node;
pub mod update;
pub mod var_name;
pub mod writes;

pub use crate::{
    arith_op::ArithOp,
    compare_op::CompareOp,
    cond_id::CondId,
    constraint::Constraint,
    construct::Construct,
    cost::Cost,
    cost_effect::CostEffect,
    coverage::Coverage,
    declared_effect::DeclaredEffect,
    expr_id::ExprId,
    extent::Extent,
    guard::Guard,
    its::Its,
    its_var::ItsVar,
    location::Location,
    location_id::LocationId,
    lowering::lower,
    lowering_error::LoweringError,
    monomial::Monomial,
    node_id::NodeId,
    polynomial::Polynomial,
    range_spec::RangeSpec,
    refusals::Refusals,
    relation::Relation,
    source_cond::SourceCond,
    source_expr::SourceExpr,
    source_program::SourceProgram,
    source_program_builder::SourceProgramBuilder,
    source_stmt::SourceStmt,
    stmt_id::StmtId,
    transition::Transition,
    unsupported::Unsupported,
    unsupported_node::UnsupportedNode,
    update::Update,
    var_name::VarName,
    writes::{Locals, Writes},
};

/// The highest total degree a [`Polynomial`] may reach.
///
/// Bounds the *work* as well as the shape: the emitter expands `x^n` into `n`
/// multiplications, so an unbounded degree would be an unbounded amount of
/// output text from a bounded amount of input.
pub const MAX_DEGREE: u32 = 8;

/// The most monomials a [`Polynomial`] may contain.
///
/// A separate cap from [`MAX_DEGREE`] because neither implies the other.
/// `(a + b + c)^8` is three variables and one operator away from trivial, has
/// degree 8, and expands to 45 terms; `(a + b + c + d)^8` expands to 165. The
/// degree cap alone would let a frontend hand over a short expression whose
/// expansion is exponential in the number of variables.
pub const MAX_MONOMIALS: usize = 256;

/// The most clauses a condition's disjunctive normal form may reach before it
/// is widened to `true`.
///
/// A chain of `n` `or`-ed inequalities has `n` clauses positively and `2^n`
/// negatively, so this cap is reached by ordinary-looking source. Reaching it
/// costs precision and never soundness; the argument is in the `cond_dnf`
/// documentation inside [`lowering`].
pub const MAX_DNF_CLAUSES: usize = 64;

/// The most nodes any one arena of a [`SourceProgram`] may hold.
///
/// A frontend fed a generated or hostile file must hit a limit somewhere. It
/// is checked in [`SourceProgramBuilder`] rather than in [`lower`], because
/// the allocation happens there, and it produces a refusal rather than a panic
/// or an unbounded allocation.
pub const MAX_ARENA_NODES: usize = 1 << 22;
