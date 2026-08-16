//! Reading KoAT's answer.
//!
//! # The solver is untrusted input, in reverse
//!
//! Everything below parses text printed by a program this repository does not
//! control, does not version, and cannot test against every future build of.
//! The output *looks* structured, which is the trap: it is tempting to read it
//! leniently and take whatever comes out. The consequence of a lenient read is
//! not a crash, it is a number - and a number smaller than the one KoAT proved
//! is an upper bound the analysed program can exceed.
//!
//! So the rule is: **a parse this crate cannot do is a refusal.** Not a
//! fallback to `omega`, which would be sound but would hide a build
//! incompatibility behind a plausible-looking "we found nothing"; not a
//! best-effort read of the part that was understood; and never a guess. Every
//! shape accepted below was captured from KoAT2 v2.1.0 run on a system
//! `landav_its::koat::render` emitted.
//!
//! # The grammar, exactly as observed
//!
//! ```text
//! answer     := expression WS '{' class '}'
//! class      := 'O(1)' | 'O(n)' | 'O(n^' k ')' | 'O(log(n))' | 'Infinity'
//! expression := 'inf' | sum
//! sum        := product ('+' product)*
//! product    := factor ('*' factor)*
//! factor     := integer | argument | 'log' '(' expression ')'
//! argument   := 'Arg_' index ('^' exponent)?
//! ```
//!
//! Captured examples, each of which is a row of `tests/koat_answers.rs`:
//!
//! ```text
//! Arg_1+2 {O(n)}
//! 13 {O(1)}
//! Arg_2^2+5*Arg_2+3 {O(n^2)}
//! Arg_2*Arg_3+3*Arg_3+Arg_2+2 {O(n^2)}
//! 3*Arg_3^3+7*Arg_3^2+11*Arg_3+3 {O(n^3)}
//! log(Arg_1)+6 {O(log(n))}
//! inf {Infinity}
//! ```
//!
//! # Three things this grammar deliberately does not have
//!
//! **Subtraction.** No captured answer contains a `-`, and there is nowhere
//! for one to go: [`Bound`] ranges over `N u {omega}` and has no inverse. The
//! obvious over-approximation - drop the negative term, since removing a
//! subtraction only raises the value - is sound but would silently loosen a
//! bound without recording that it had. It is refused instead.
//!
//! **`max` and `min`.** KoAT's internal bound language has both. This crate
//! has never observed either in an `-r overall` answer, so it has no verified
//! spelling for them and refuses rather than inventing one.
//!
//! **A recursive descent.** The parse is a shunting yard over a token vector
//! with an explicit operand and operator stack. `log(...)` nests, and nesting
//! depth is decided by the solver's output rather than by this crate, so a
//! recursive parser would turn a pathological answer into a stack overflow -
//! which is an *abort*, strictly worse than a panic and invisible to the
//! `panic` lint. [`MAX_NESTING`] caps the stack on top of that.
//!
//! # The announced class is a cross-check, not decoration
//!
//! KoAT states the growth class beside the bound. That is an independent
//! statement about the same object, so the parsed expression's own degree can
//! be compared against it at no cost. A quadratic answer read as linear -
//! which is what dropping a factor of `Arg_2^2` looks like - contradicts the
//! `{O(n^2)}` KoAT printed, and the answer is refused rather than published.
//! It does not catch every mis-parse (dropping a lower-order term leaves the
//! degree intact) but it catches the class of mistake that makes a bound
//! *smaller*, which is the class that matters.

use std::collections::HashMap;

use landav_bound::{Base, Bound, BoundKind, Nat, Symbol, TransKind};

use crate::{
    MAX_ANSWER_BYTES, MAX_ANSWER_TOKENS, MAX_EXPONENT, MAX_MEASURE_STEPS, MAX_NESTING,
    answer::Answer, arg_map::ArgMap, growth::Growth, solver::Solver, solver_error::SolverError,
};

/// The line KoAT prints when it stops on its own `--timeout`.
const SELF_TIMEOUT: &str = "TIMEOUT:";

/// One lexical item of a KoAT bound expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    /// `+`
    Plus,
    /// `*`
    Star,
    /// `)`, closing a `log(`.
    Close,
    /// `log`, always immediately followed by `(`.
    Log,
    /// `inf`
    Infinite,
    /// A non-negative literal.
    Number(u64),
    /// `Arg_i`, with the exponent that followed it (`1` when there was none).
    Argument {
        /// The positional index.
        index: u32,
        /// The literal exponent, already checked against [`MAX_EXPONENT`].
        exponent: u32,
    },
}

/// An entry on the operator stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// Pending `+`.
    Plus,
    /// Pending `*`.
    Star,
    /// An open `log(` waiting for its `)`.
    LogOpen,
}

/// Parse one KoAT answer.
///
/// `stdout` is the solver's entire standard output. The answer is expected to
/// be the only non-blank line: KoAT's default `-r overall` prints exactly one,
/// and anything else means either a different result mode or a build this
/// crate has not verified.
///
/// # Errors
///
/// Every way of not producing a bound is an error rather than a quiet
/// `Unknown`. See [`SolverError`]; the ones reachable from here are
/// [`SolverError::OutputTooLarge`], [`SolverError::NoAnswer`],
/// [`SolverError::SolverTimedOut`], [`SolverError::Unparsable`],
/// [`SolverError::ArgIndexOutOfRange`], [`SolverError::ExponentTooLarge`] and
/// [`SolverError::ClassMismatch`].
pub fn parse(stdout: &str, map: &ArgMap) -> Result<Answer, SolverError> {
    if stdout.len() > MAX_ANSWER_BYTES {
        return Err(SolverError::OutputTooLarge {
            got: stdout.len(),
            limit: MAX_ANSWER_BYTES,
            solver: Solver::Koat,
        });
    }

    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let Some(line) = lines.next() else {
        return Err(SolverError::NoAnswer {
            solver: Solver::Koat,
        });
    };
    if line.starts_with(SELF_TIMEOUT) {
        return Err(SolverError::SolverTimedOut {
            solver: Solver::Koat,
        });
    }
    if lines.next().is_some() {
        // Which of them is *the* answer is a guess, and this crate does not
        // guess. `-r overall` prints one line; more than one means the
        // invocation or the build is not what this crate verified.
        return Err(unparsable(stdout, "more than one non-blank line"));
    }

    let (expression, class) = split(line)?;
    let announced = growth_class(class).ok_or_else(|| {
        unparsable(
            class,
            "a growth class this build cannot check the bound against",
        )
    })?;
    let bound = expression_bound(expression, map)?;
    let derived = derived_growth(&bound);

    if derived != announced {
        return Err(SolverError::ClassMismatch {
            solver: Solver::Koat,
            announced,
            derived,
            text: expression.to_owned(),
        });
    }

    if announced == Growth::Unbounded {
        // `inf {Infinity}` - KoAT's search found no bound. A real answer, and
        // the caller publishes it as `omega` with blame.
        return Ok(Answer::Unknown);
    }
    Ok(Answer::Symbolic {
        bound,
        growth: announced,
    })
}

/// An [`SolverError::Unparsable`] quoting a bounded excerpt of `at`.
fn unparsable(at: &str, detail: &'static str) -> SolverError {
    /// How much of the offending text reaches the message. Long enough to
    /// identify what arrived, short enough not to paste a megabyte into a CI
    /// log.
    const EXCERPT: usize = 200;
    let excerpt: String = at.trim().chars().take(EXCERPT).collect();
    SolverError::Unparsable {
        solver: Solver::Koat,
        at: excerpt,
        detail,
    }
}

/// Split `expression {class}` at the final brace pair.
fn split(line: &str) -> Result<(&str, &str), SolverError> {
    let Some(open) = line.rfind('{') else {
        return Err(unparsable(line, "no `{class}` beside the bound"));
    };
    let Some(rest) = line.get(open + 1..) else {
        return Err(unparsable(line, "a `{` that does not start a class"));
    };
    let Some(class) = rest.strip_suffix('}') else {
        return Err(unparsable(line, "a class that is not closed by `}`"));
    };
    let expression = line.get(..open).unwrap_or("").trim();
    if expression.is_empty() {
        return Err(unparsable(line, "a class with no bound beside it"));
    }
    Ok((expression, class.trim()))
}

/// The growth class KoAT announced, for the spellings this crate has verified.
///
/// `None` for everything else - including `O(EXP)` and `O(n*log(n))`, which
/// KoAT can certainly print. They are refused rather than accepted because a
/// class this crate cannot derive from the parsed expression is a class it
/// cannot use as a cross-check, and an unchecked cross-check is worse than
/// none: it looks like one.
fn growth_class(class: &str) -> Option<Growth> {
    match class {
        "O(1)" => Some(Growth::Constant),
        "O(log(n))" => Some(Growth::Logarithmic),
        "O(n)" => Some(Growth::Polynomial(1)),
        "Infinity" => Some(Growth::Unbounded),
        _ => class
            .strip_prefix("O(n^")
            .and_then(|rest| rest.strip_suffix(')'))
            .and_then(|degree| degree.parse::<u32>().ok())
            .filter(|degree| *degree >= 1)
            .map(Growth::Polynomial),
    }
}

/// The bound an expression denotes.
fn expression_bound(expression: &str, map: &ArgMap) -> Result<Bound, SolverError> {
    let tokens = tokenise(expression)?;
    shunt(&tokens, expression, map)
}

/// Split an expression into tokens, refusing anything outside the grammar.
fn tokenise(expression: &str) -> Result<Vec<Token>, SolverError> {
    let bytes = expression.as_bytes();
    let mut tokens: Vec<Token> = Vec::new();
    let mut at = 0usize;

    // Counted, not `while at < bytes.len()`. Every arm below advances `at` by
    // at least one byte, so the input's own length is an upper bound on the
    // iterations - and writing it that way makes the loop finite *by
    // construction* rather than by every arm being right. A mutation that
    // stops one arm advancing then produces the post-condition failure below
    // instead of an infinite loop, and a hang is invisible to the panic lints.
    for _ in 0..bytes.len() {
        let Some(&byte) = bytes.get(at) else { break };
        if tokens.len() >= MAX_ANSWER_TOKENS {
            return Err(unparsable(expression, "more tokens than a bound can have"));
        }
        match byte {
            b' ' | b'\t' => at += 1,
            b'+' => {
                tokens.push(Token::Plus);
                at += 1;
            }
            b'*' => {
                tokens.push(Token::Star);
                at += 1;
            }
            b')' => {
                tokens.push(Token::Close);
                at += 1;
            }
            b'0'..=b'9' => {
                let (digits, next) = span(bytes, at, u8::is_ascii_digit);
                let literal = expression.get(digits).unwrap_or("");
                let value = literal
                    .parse::<u64>()
                    .map_err(|_| unparsable(expression, "a literal too large to represent"))?;
                tokens.push(Token::Number(value));
                at = next;
            }
            b'A' => {
                let (index, next) = argument(expression, bytes, at)?;
                tokens.push(index);
                at = next;
            }
            b'l' if bytes.get(at..at + 3) == Some(b"log".as_slice()) => {
                // `log` is only meaningful as `log(`; a bare `log` is a name
                // this crate has never seen.
                if bytes.get(at + 3) != Some(&b'(') {
                    return Err(unparsable(expression, "`log` without a parenthesis"));
                }
                // `Token::Log` carries its own parenthesis: `log` is only
                // ever `log(`, so a separate `Open` token would be a second
                // representation of the same fact and an arm no input reaches.
                tokens.push(Token::Log);
                at += 4;
            }
            b'i' if bytes.get(at..at + 3) == Some(b"inf".as_slice()) => {
                tokens.push(Token::Infinite);
                at += 3;
            }
            _ => return Err(unparsable(expression, "a character outside the grammar")),
        }
    }
    // The post-condition the counted loop trades for: every byte was consumed
    // by exactly one arm. If any arm stopped advancing, the loop runs out of
    // iterations with input left over and says so.
    if at != bytes.len() {
        return Err(unparsable(expression, "text this build could not consume"));
    }
    if tokens.is_empty() {
        return Err(unparsable(expression, "an empty bound"));
    }
    Ok(tokens)
}

/// Read `Arg_<index>` and any `^<exponent>` that follows it.
fn argument(expression: &str, bytes: &[u8], at: usize) -> Result<(Token, usize), SolverError> {
    if bytes.get(at..at + 4) != Some(b"Arg_".as_slice()) {
        return Err(unparsable(expression, "a name that is not `Arg_`"));
    }
    let (digits, after_index) = span(bytes, at + 4, u8::is_ascii_digit);
    if digits.is_empty() {
        return Err(unparsable(expression, "`Arg_` with no index"));
    }
    let index = expression
        .get(digits)
        .unwrap_or("")
        .parse::<u32>()
        .map_err(|_| unparsable(expression, "an argument index too large to represent"))?;

    if bytes.get(after_index) != Some(&b'^') {
        return Ok((Token::Argument { index, exponent: 1 }, after_index));
    }
    let (power, after_power) = span(bytes, after_index + 1, u8::is_ascii_digit);
    if power.is_empty() {
        return Err(unparsable(expression, "`^` with no exponent"));
    }
    let exponent = expression
        .get(power)
        .unwrap_or("")
        .parse::<u64>()
        .map_err(|_| unparsable(expression, "an exponent too large to represent"))?;
    // Checked *before* a single factor is built. `Arg_0^k` expands into `k`
    // operands, so an unchecked `k` is an allocation the solver's output
    // chooses - and a `Vec` that cannot grow calls `handle_alloc_error`, which
    // aborts. See `tests/frozen_solver_invariants.rs`.
    if exponent > u64::from(MAX_EXPONENT) {
        return Err(SolverError::ExponentTooLarge {
            got: exponent,
            limit: MAX_EXPONENT,
        });
    }
    let exponent = u32::try_from(exponent).unwrap_or(MAX_EXPONENT);
    Ok((Token::Argument { index, exponent }, after_power))
}

/// The maximal run of bytes from `at` satisfying `accept`, and where it ends.
fn span(bytes: &[u8], at: usize, accept: impl Fn(&u8) -> bool) -> (core::ops::Range<usize>, usize) {
    let mut end = at;
    // Iterating the indices rather than advancing a cursor: the loop is
    // bounded by the slice, and `end` is assigned rather than incremented, so
    // no single-operator mutation can leave it standing still.
    for (index, byte) in bytes.iter().enumerate().skip(at) {
        if !accept(byte) {
            break;
        }
        end = index + 1;
    }
    (at..end, end)
}

/// Fold the token stream into a [`Bound`] with an explicit operand and
/// operator stack.
///
/// Precedence is `^` (already folded into the token) over `*` over `+`. The
/// only nesting is `log(`, which pushes [`Op::LogOpen`] as a barrier.
fn shunt(tokens: &[Token], expression: &str, map: &ArgMap) -> Result<Bound, SolverError> {
    let mut operands: Vec<Bound> = Vec::new();
    let mut operators: Vec<Op> = Vec::new();
    // `true` when the next token must be an operand: at the start, after an
    // operator, and after an opening parenthesis. It is what makes `Arg_1
    // Arg_0`, `+Arg_1` and `Arg_1+` all refusals rather than readings.
    let mut expect_operand = true;

    for token in tokens {
        match token {
            Token::Number(value) => {
                require(
                    expect_operand,
                    expression,
                    "a literal where an operator was due",
                )?;
                operands.push(Bound::constant(*value));
                expect_operand = false;
            }
            Token::Infinite => {
                require(
                    expect_operand,
                    expression,
                    "`inf` where an operator was due",
                )?;
                operands.push(Bound::omega());
                expect_operand = false;
            }
            Token::Argument { index, exponent } => {
                require(
                    expect_operand,
                    expression,
                    "an argument where an operator was due",
                )?;
                let name = map.name(*index)?;
                operands.push(monomial(name.clone(), *exponent));
                expect_operand = false;
            }
            Token::Log => {
                require(
                    expect_operand,
                    expression,
                    "`log` where an operator was due",
                )?;
                if operators.len() >= MAX_NESTING {
                    return Err(unparsable(expression, "more nesting than a bound can have"));
                }
                operators.push(Op::LogOpen);
                expect_operand = true;
            }
            Token::Close => {
                require(!expect_operand, expression, "an empty `log()`")?;
                loop {
                    match operators.pop() {
                        Some(Op::LogOpen) => break,
                        Some(op) => apply(op, &mut operands, expression)?,
                        None => return Err(unparsable(expression, "an unopened parenthesis")),
                    }
                }
                let Some(argument) = operands.pop() else {
                    return Err(unparsable(expression, "a `log` with no argument"));
                };
                // Base two, and not by accident. `log` is anti-monotone in its
                // base - `log_2(x) >= log_e(x) >= log_10(x)` - and KoAT does
                // not state one. For an *upper* bound the smallest permitted
                // base gives the largest value, so base two is the only
                // reading that cannot report less than KoAT proved.
                operands.push(Bound::log(Base::TWO, argument));
                expect_operand = false;
            }
            Token::Plus | Token::Star => {
                require(
                    !expect_operand,
                    expression,
                    "an operator with no left operand",
                )?;
                let incoming = if *token == Token::Plus {
                    Op::Plus
                } else {
                    Op::Star
                };
                while let Some(top) = operators.last().copied() {
                    if binding(top) < binding(incoming) {
                        break;
                    }
                    operators.pop();
                    apply(top, &mut operands, expression)?;
                }
                operators.push(incoming);
                expect_operand = true;
            }
        }
    }

    require(!expect_operand, expression, "a dangling operator")?;
    while let Some(op) = operators.pop() {
        if op == Op::LogOpen {
            return Err(unparsable(expression, "an unclosed `log(`"));
        }
        apply(op, &mut operands, expression)?;
    }
    match operands.pop() {
        Some(bound) if operands.is_empty() => Ok(bound),
        // Defensive, and knowingly unreachable: `expect_operand` maintains
        // "one more operand than pending operators", so the drain above always
        // leaves exactly one. Mutation testing reports the guard as a
        // survivor and it is a genuinely equivalent mutant. It stays because
        // the invariant it restates is maintained across four match arms and a
        // loop, and a future production added to the grammar could break it in
        // a way that would otherwise publish the *last* operand as the whole
        // bound - which is a bound smaller than the one the solver stated.
        Some(_) => Err(unparsable(expression, "two bounds side by side")),
        None => Err(unparsable(expression, "an empty bound")),
    }
}

/// How tightly an operator binds: `*` over `+`, and `log(` binds nothing at
/// all so that it stops the fold rather than being applied by it.
///
/// Written as a comparison of two numbers rather than as a pair of equality
/// tests, which is the spelling this started as and which mutation testing
/// found a fully equivalent mutant in.
///
/// One equivalent mutant remains and it is unavoidable: `<` widened to `<=`
/// makes a run of same-precedence operators associate to the right instead of
/// the left. `Bound::sum` and `Bound::prod` both **flatten**, so `(a+b)+c` and
/// `a+(b+c)` are the identical term and no assertion can separate them.
/// Killing it would mean making the algebra distinguish two spellings of one
/// value, which is a worse crate for a better number.
const fn binding(op: Op) -> u8 {
    match op {
        Op::LogOpen => 0,
        Op::Plus => 1,
        Op::Star => 2,
    }
}

/// `name^exponent`, as repeated multiplication.
///
/// [`Bound::pow`] is the wrong operator: it raises a *constant* base to a
/// symbolic power, which is `2^n`, not `n^2`. A monomial is a product, and the
/// exponent has already been checked against [`MAX_EXPONENT`].
fn monomial(name: Symbol, exponent: u32) -> Bound {
    if exponent == 1 {
        return Bound::var(name);
    }
    // `Bound::prod` of an empty list is `1`, which is the right value for
    // `x^0` - and the reason `exponent == 0` needs no special case.
    Bound::prod((0..exponent).map(|_| Bound::var(name.clone())))
}

/// Pop two operands and push their combination.
fn apply(op: Op, operands: &mut Vec<Bound>, expression: &str) -> Result<(), SolverError> {
    let (Some(right), Some(left)) = (operands.pop(), operands.pop()) else {
        return Err(unparsable(expression, "an operator with too few operands"));
    };
    operands.push(match op {
        Op::Plus => Bound::sum([left, right]),
        Op::Star => Bound::prod([left, right]),
        // `LogOpen` is never applied: the `)` arm consumes it and the final
        // drain refuses it. Combining as a sum here would be sound but would
        // hide a stack that is not shaped the way this function assumes.
        Op::LogOpen => return Err(unparsable(expression, "an unclosed `log(`")),
    });
    Ok(())
}

/// Refuse unless `holds`.
fn require(holds: bool, expression: &str, detail: &'static str) -> Result<(), SolverError> {
    if holds {
        Ok(())
    } else {
        Err(unparsable(expression, detail))
    }
}

/// The growth class the parsed bound actually has.
///
/// Computed over the term's DAG with a memo, because `Bound` shares nodes and
/// a naive walk of a shared term is exponential in its depth.
///
/// `log` contributes a flag rather than a degree: `log(x)` is sub-polynomial,
/// so a term that is only logarithms is [`Growth::Logarithmic`] and one that
/// mixes a logarithm with a variable falls out as a polynomial of the
/// variable's degree. The latter will not match any class KoAT announces -
/// `O(n*log(n))` is not in [`growth_class`]'s vocabulary - so such an answer
/// is refused, which is the intended outcome for a shape this crate has not
/// verified.
fn derived_growth(bound: &Bound) -> Growth {
    let (degree, has_log, unbounded) = measure(bound);
    if unbounded {
        Growth::Unbounded
    } else if degree == 0 && has_log {
        Growth::Logarithmic
    } else {
        Growth::polynomial(degree)
    }
}

/// `(total degree, contains a logarithm, contains omega)`.
fn measure(bound: &Bound) -> (u32, bool, bool) {
    let mut memo: HashMap<Bound, (u32, bool, bool)> = HashMap::new();
    // An explicit worklist, post-order: children are measured before the
    // parent that needs them. Recursion here would be bounded by `MAX_DEPTH`
    // rather than by anything this crate controls.
    //
    // Counted, for the same reason `tokenise` is. The loop's own exit
    // condition is "the worklist emptied", which depends on the two-phase push
    // being exactly right; weaken that and the loop pushes forever.
    // `MAX_MEASURE_STEPS` makes it finite whatever the body does, and running
    // out of steps yields the unbounded fallback below - which contradicts
    // every finite class KoAT can announce, so the answer is refused rather
    // than published on an incomplete measurement.
    let mut work: Vec<(Bound, bool)> = vec![(bound.clone(), false)];
    for _ in 0..MAX_MEASURE_STEPS {
        let Some((node, expanded)) = work.pop() else {
            break;
        };
        if memo.contains_key(&node) {
            continue;
        }
        let children = operands(&node);
        if !expanded && !children.is_empty() {
            work.push((node, true));
            for child in children {
                work.push((child, false));
            }
            continue;
        }
        let value = combine(&node, &memo);
        memo.insert(node, value);
    }
    memo.get(bound).copied().unwrap_or((0, false, true))
}

/// The operands of a node, as owned handles.
fn operands(bound: &Bound) -> Vec<Bound> {
    match bound.kind() {
        BoundKind::Sum(terms) | BoundKind::Prod(terms) => terms.as_slice().to_vec(),
        BoundKind::Max(terms) => terms.as_slice().to_vec(),
        BoundKind::Trans { arg, .. } => vec![arg.clone()],
        BoundKind::Const(_) | BoundKind::Var(_) => Vec::new(),
    }
}

/// Combine a node's already-measured children.
///
/// The n-ary shapes share one fold, differing only in whether degrees add or
/// are maximised. `Max` cannot arise from this parser - there is no `max`
/// production - so giving it an arm of its own would be an arm no input can
/// reach.
fn combine(bound: &Bound, memo: &HashMap<Bound, (u32, bool, bool)>) -> (u32, bool, bool) {
    let of = |child: &Bound| memo.get(child).copied().unwrap_or((0, false, true));
    match bound.kind() {
        BoundKind::Const(Nat::Omega) => (0, false, true),
        BoundKind::Const(Nat::Fin(_)) => (0, false, false),
        BoundKind::Var(_) => (1, false, false),
        BoundKind::Trans { kind, arg, .. } => {
            let (_, _, inf) = of(arg);
            match kind {
                // `log(x)` is sub-polynomial whatever `x` is, so it
                // contributes the flag rather than the operand's degree.
                TransKind::Log => (0, true, inf),
                // `Pow` is `base^arg` with a constant base - genuinely
                // exponential, and unreachable from this parser, which has no
                // production for it. Measured as unbounded so that an answer
                // which somehow contained one is refused against every finite
                // class rather than read as a constant.
                TransKind::Pow => (0, false, true),
            }
        }
        // A product's degree is the sum of its factors'; a sum's is the
        // largest of its terms'.
        //
        // The `inf` disjunction below is unreachable and mutation testing says
        // so: `Bound::sum` and `Bound::prod` both absorb `omega` into a single
        // `Const(Omega)`, so no n-ary node can have an unbounded operand to
        // propagate. It is written out anyway because the alternative is a
        // fold that silently drops the flag if that absorption ever changes,
        // and dropping it would read `omega` as a polynomial.
        BoundKind::Sum(_) | BoundKind::Max(_) | BoundKind::Prod(_) => {
            let adds = matches!(bound.kind(), BoundKind::Prod(_));
            operands(bound).iter().map(of).fold(
                (0, false, false),
                |(degree, log, inf), (d, l, i)| {
                    let combined = if adds {
                        degree.saturating_add(d)
                    } else {
                        degree.max(d)
                    };
                    (combined, log || l, inf || i)
                },
            )
        }
    }
}
