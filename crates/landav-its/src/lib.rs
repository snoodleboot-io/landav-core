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
//! fragment KoAT can already reason about, which is how R0 produces real bounds
//! within weeks.
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
//! rather than approximated. This is the most valuable single construct to add
//! next: `while n > 1: n = n // 2` is the canonical logarithmic loop and this
//! fragment cannot express it.
//!
//! **`break` and `continue`** ([`Construct::LoopJump`]) are sound to support —
//! a `break` is a transition to the loop's exit location and a `continue` one
//! to its head, and the lowering already has both locations in hand. They are
//! refused because the story's fragment did not name them and a loop-context
//! stack is machinery this lane did not need for the KoAT worked example.
//! Cheap to add, and the first thing to add after division.
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
//! stood for.
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
pub mod coverage;
pub mod expr_id;
pub mod guard;
pub mod its;
pub mod its_var;
pub mod koat;
pub mod location;
pub mod location_id;
pub mod lowering;
pub mod lowering_error;
pub mod monomial;
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
pub mod update;
pub mod var_name;

pub use crate::{
    arith_op::ArithOp, compare_op::CompareOp, cond_id::CondId, constraint::Constraint,
    construct::Construct, coverage::Coverage, expr_id::ExprId, guard::Guard, its::Its,
    its_var::ItsVar, location::Location, location_id::LocationId, lowering::lower,
    lowering_error::LoweringError, monomial::Monomial, polynomial::Polynomial,
    range_spec::RangeSpec, refusals::Refusals, relation::Relation, source_cond::SourceCond,
    source_expr::SourceExpr, source_program::SourceProgram,
    source_program_builder::SourceProgramBuilder, source_stmt::SourceStmt, stmt_id::StmtId,
    transition::Transition, unsupported::Unsupported, update::Update, var_name::VarName,
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
