//! `LAN-67` criterion 3, as far as it can honestly be run here.
//!
//! # KoAT is not installed, so this is the substitute — and its limits
//!
//! The criterion says twenty hand-written functions *lower and are accepted by
//! KoAT*. There is no KoAT binary on this machine and `landav-solvers` (the
//! crate that will invoke one, `LAN-7`) is still a stub, so the second half of
//! that sentence cannot be executed and **is not claimed**.
//!
//! What is executed instead is an independently written **parser and checker**
//! for the emitted format: a tokeniser and recursive-descent parser written
//! from the format description in [`landav_its::koat`], which re-reads the
//! emitted text and reconstructs what it says. That catches the failure class
//! a real KoAT run would catch — text that does not parse, rules that mention
//! undeclared variables, an arity mismatch between a location's uses — and one
//! it would not: that the *meaning* of the emitted text agrees with the
//! [`landav_its::Polynomial`] it was rendered from, checked by evaluating both
//! at the same valuations.
//!
//! It cannot catch a dialect disagreement between this crate's reading of the
//! format and KoAT's own. Running the real binary remains open work and is
//! reported as such.

use std::collections::BTreeMap;

use landav_bound::Origin;
use landav_its::{ItsVar, Polynomial, SourceProgramBuilder, VarName, koat, lower};
use proptest::prelude::*;

use crate::reference::{State, evaluate};

// ---------------------------------------------------------------------------
// an independent reader for the emitted format
// ---------------------------------------------------------------------------

/// One rule, as read back out of the text.
#[derive(Debug, Clone)]
pub struct ParsedRule {
    pub source: String,
    pub arguments: Vec<String>,
    pub target: String,
    pub images: Vec<Ast>,
    pub guard: Vec<(Ast, String, Ast)>,
}

/// A parsed arithmetic expression.
#[derive(Debug, Clone)]
pub enum Ast {
    Int(i128),
    Var(String),
    Add(Box<Ast>, Box<Ast>),
    Sub(Box<Ast>, Box<Ast>),
    Mul(Box<Ast>, Box<Ast>),
    Neg(Box<Ast>),
}

impl Ast {
    /// Every variable name this expression mentions.
    fn identifiers(&self) -> Vec<String> {
        match self {
            Self::Int(_) => Vec::new(),
            Self::Var(name) => vec![name.clone()],
            Self::Add(left, right) | Self::Sub(left, right) | Self::Mul(left, right) => {
                let mut names = left.identifiers();
                names.extend(right.identifiers());
                names
            }
            Self::Neg(operand) => operand.identifiers(),
        }
    }

    /// The value of this expression under `lookup`.
    fn eval(&self, lookup: &BTreeMap<String, i128>) -> Option<i128> {
        match self {
            Self::Int(value) => Some(*value),
            Self::Var(name) => lookup.get(name).copied(),
            Self::Add(left, right) => left.eval(lookup)?.checked_add(right.eval(lookup)?),
            Self::Sub(left, right) => left.eval(lookup)?.checked_sub(right.eval(lookup)?),
            Self::Mul(left, right) => left.eval(lookup)?.checked_mul(right.eval(lookup)?),
            Self::Neg(operand) => operand.eval(lookup)?.checked_neg(),
        }
    }
}

/// What one emitted document says.
#[derive(Debug, Clone)]
pub struct ParsedIts {
    pub start: String,
    pub variables: Vec<String>,
    pub rules: Vec<ParsedRule>,
}

/// Reads an emitted document, or says why it could not.
///
/// Written from the format description, not from the emitter.
pub fn parse(text: &str) -> Result<ParsedIts, String> {
    let mut start = None;
    let mut variables = Vec::new();
    let mut rules = Vec::new();
    let mut in_rules = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line == "(GOAL COMPLEXITY)" {
            continue;
        }
        if let Some(rest) = line.strip_prefix("(STARTTERM (FUNCTIONSYMBOLS ") {
            let name = rest.trim_end_matches(')').trim();
            start = Some(name.to_owned());
            continue;
        }
        if let Some(rest) = line.strip_prefix("(VAR") {
            let body = rest.trim_end_matches(')').trim();
            variables = body
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<String>>();
            continue;
        }
        if line == "(RULES" {
            in_rules = true;
            continue;
        }
        if line == ")" {
            in_rules = false;
            continue;
        }
        if in_rules {
            rules.push(parse_rule(line)?);
        } else {
            return Err(format!("unrecognised line outside (RULES ...): {line}"));
        }
    }

    Ok(ParsedIts {
        start: start.ok_or_else(|| "no (STARTTERM ...) declaration".to_owned())?,
        variables,
        rules,
    })
}

fn parse_rule(line: &str) -> Result<ParsedRule, String> {
    let (rule, guard_text) = match line.split_once(":|:") {
        Some((rule, guard)) => (rule.trim(), Some(guard.trim())),
        None => (line, None),
    };
    // Two arrow forms. `->` is KoAT's unit-cost rule; `-{c}>` carries an
    // explicit cost, which the lowering emits for every transition it invented
    // for its own convenience. Parsing only the bare form would let a
    // mis-costed rule through as a parse error rather than as a wrong number,
    // which reads like a format bug and is not one.
    //
    // The annotated form is tried first: `-{0}>` also contains no `->`, but a
    // naive `->` search on some future syntax could match inside the braces.
    let (lhs, rhs) = match rule.split_once("-{") {
        Some((lhs, rest)) => {
            let (cost, rhs) = rest
                .split_once("}>")
                .ok_or_else(|| format!("weighted arrow is not closed: {line}"))?;
            // The cost must parse as an expression, or the emitter has written
            // something the solver cannot read.
            parse_expr(cost.trim())
                .map_err(|why| format!("rule cost does not parse: {cost:?}: {why}"))?;
            (lhs, rhs)
        }
        None => rule
            .split_once("->")
            .ok_or_else(|| format!("rule has no arrow: {line}"))?,
    };

    let (source, argument_text) = split_call(lhs.trim())?;
    let (target, image_text) = split_call(rhs.trim())?;

    let arguments = if argument_text.trim().is_empty() {
        Vec::new()
    } else {
        argument_text
            .split(',')
            .map(|part| part.trim().to_owned())
            .collect()
    };

    let images = if image_text.trim().is_empty() {
        Vec::new()
    } else {
        image_text
            .split(',')
            .map(|part| parse_expr(part.trim()))
            .collect::<Result<Vec<Ast>, String>>()?
    };

    let mut guard = Vec::new();
    if let Some(text) = guard_text {
        for conjunct in text.split("&&") {
            guard.push(parse_constraint(conjunct.trim())?);
        }
    }

    Ok(ParsedRule {
        source,
        arguments,
        target,
        images,
        guard,
    })
}

fn split_call(text: &str) -> Result<(String, String), String> {
    let open = text
        .find('(')
        .ok_or_else(|| format!("not a call: {text}"))?;
    let close = text
        .rfind(')')
        .ok_or_else(|| format!("not a call: {text}"))?;
    if close < open {
        return Err(format!("unbalanced parentheses: {text}"));
    }
    let name = text.get(..open).unwrap_or_default().trim().to_owned();
    let body = text.get(open + 1..close).unwrap_or_default().to_owned();
    Ok((name, body))
}

fn parse_constraint(text: &str) -> Result<(Ast, String, Ast), String> {
    for operator in [">=", ">", "="] {
        if let Some((left, right)) = text.split_once(operator) {
            return Ok((
                parse_expr(left.trim())?,
                operator.to_owned(),
                parse_expr(right.trim())?,
            ));
        }
    }
    Err(format!("constraint has no relation: {text}"))
}

/// `expr := term (('+' | '-') term)*`
fn parse_expr(text: &str) -> Result<Ast, String> {
    let tokens = tokenise(text)?;
    let mut cursor = 0_usize;
    let parsed = expression(&tokens, &mut cursor)?;
    if cursor != tokens.len() {
        return Err(format!("trailing tokens in `{text}`"));
    }
    Ok(parsed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Int(i128),
    Ident(String),
    Plus,
    Minus,
    Star,
}

fn tokenise(text: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let characters: Vec<char> = text.chars().collect();
    let mut index = 0_usize;
    while index < characters.len() {
        let character = characters.get(index).copied().unwrap_or(' ');
        match character {
            ' ' | '\t' => index += 1,
            '+' => {
                tokens.push(Token::Plus);
                index += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                index += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                index += 1;
            }
            _ if character.is_ascii_digit() => {
                let mut value: i128 = 0;
                while let Some(digit) = characters.get(index).and_then(|c| c.to_digit(10)) {
                    value = value
                        .checked_mul(10)
                        .and_then(|shifted| shifted.checked_add(i128::from(digit)))
                        .ok_or_else(|| format!("integer literal out of range in `{text}`"))?;
                    index += 1;
                }
                tokens.push(Token::Int(value));
            }
            _ if character.is_ascii_alphanumeric() || character == '_' => {
                let mut name = String::new();
                while let Some(next) = characters.get(index) {
                    if next.is_ascii_alphanumeric() || *next == '_' {
                        name.push(*next);
                        index += 1;
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Ident(name));
            }
            other => return Err(format!("unexpected character `{other}` in `{text}`")),
        }
    }
    Ok(tokens)
}

fn expression(tokens: &[Token], cursor: &mut usize) -> Result<Ast, String> {
    let mut left = term(tokens, cursor)?;
    while let Some(token) = tokens.get(*cursor) {
        match token {
            Token::Plus => {
                *cursor += 1;
                left = Ast::Add(Box::new(left), Box::new(term(tokens, cursor)?));
            }
            Token::Minus => {
                *cursor += 1;
                left = Ast::Sub(Box::new(left), Box::new(term(tokens, cursor)?));
            }
            _ => break,
        }
    }
    Ok(left)
}

fn term(tokens: &[Token], cursor: &mut usize) -> Result<Ast, String> {
    let mut left = factor(tokens, cursor)?;
    while let Some(Token::Star) = tokens.get(*cursor) {
        *cursor += 1;
        left = Ast::Mul(Box::new(left), Box::new(factor(tokens, cursor)?));
    }
    Ok(left)
}

fn factor(tokens: &[Token], cursor: &mut usize) -> Result<Ast, String> {
    match tokens.get(*cursor) {
        Some(Token::Minus) => {
            *cursor += 1;
            Ok(Ast::Neg(Box::new(factor(tokens, cursor)?)))
        }
        Some(Token::Int(value)) => {
            *cursor += 1;
            Ok(Ast::Int(*value))
        }
        Some(Token::Ident(name)) => {
            *cursor += 1;
            Ok(Ast::Var(name.clone()))
        }
        other => Err(format!("expected a factor, found {other:?}")),
    }
}

// ---------------------------------------------------------------------------
// the checks
// ---------------------------------------------------------------------------

/// Reads an emitted document back and checks it is well formed.
///
/// Returns the parsed form so callers can assert more.
pub fn check_well_formed(its: &landav_its::Its) -> ParsedIts {
    let text = its.to_koat();
    let parsed = match parse(&text) {
        Ok(parsed) => parsed,
        Err(error) => panic!("emitted text does not parse: {error}\n{text}"),
    };

    assert_eq!(
        parsed.start,
        its.start().to_string(),
        "the declared start term is not the start location"
    );
    assert_eq!(
        parsed.rules.len(),
        its.transitions().len(),
        "the emitted text has a different number of rules than the system has transitions"
    );

    let declared: Vec<String> = its.vars().iter().map(koat::mangle).collect();
    assert_eq!(parsed.variables, declared, "the (VAR ...) list is wrong");

    let locations: Vec<String> = its
        .locations()
        .iter()
        .map(|location| location.id().to_string())
        .collect();

    for rule in &parsed.rules {
        assert!(
            locations.contains(&rule.source),
            "rule leaves undeclared location `{}`",
            rule.source
        );
        assert!(
            locations.contains(&rule.target),
            "rule enters undeclared location `{}`",
            rule.target
        );
        assert_eq!(
            rule.arguments, declared,
            "a rule's left-hand side does not take the whole variable tuple in order"
        );
        assert_eq!(
            rule.images.len(),
            declared.len(),
            "a rule's right-hand side has the wrong arity"
        );

        // Every identifier the rule mentions must be a declared variable. A
        // rule referring to an undeclared name is the failure a real KoAT run
        // would reject the file for, and it is exactly what a mangling bug or
        // a forgotten fresh variable would produce.
        for image in &rule.images {
            for name in image.identifiers() {
                assert!(
                    declared.contains(&name),
                    "rule image mentions undeclared variable `{name}`"
                );
            }
        }
        for (left, _, right) in &rule.guard {
            for name in left.identifiers().into_iter().chain(right.identifiers()) {
                assert!(
                    declared.contains(&name),
                    "rule guard mentions undeclared variable `{name}`"
                );
            }
        }
    }

    check_meaning(its, &parsed);

    parsed
}

/// **The emitted text must mean what the system means.**
///
/// Well-formedness is not enough, and this is the check that says so: an
/// emitter that dropped every guard, or rendered an update as the identity,
/// would produce a document that parses, declares the right variables, has the
/// right arity and is completely wrong. A real KoAT run would happily accept
/// it and return a bound for a different program.
///
/// So each rule is evaluated against its transition at a spread of valuations
/// — the parsed text through the independent [`Ast::eval`], the system through
/// the independent [`evaluate`] — and the two must agree on both the guard's
/// truth and every image's value.
fn check_meaning(its: &landav_its::Its, parsed: &ParsedIts) {
    // A spread of valuations, including zero, both signs and a large value, so
    // that a guard which is accidentally constant is caught.
    let probes: [i128; 6] = [0, 1, -1, 3, -7, 128];

    for (index, transition) in its.transitions().iter().enumerate() {
        let Some(rule) = parsed.rules.get(index) else {
            panic!("rule {index} is missing from the emitted text");
        };

        for probe in probes {
            // Give each variable a distinct value derived from the probe, so
            // that swapping two variables is caught as well as dropping one.
            let mut by_name: State = State::new();
            let mut by_mangled: BTreeMap<String, i128> = BTreeMap::new();
            for (position, var) in its.vars().iter().enumerate() {
                let offset = i128::try_from(position).unwrap_or(0);
                let value = probe.saturating_add(offset);
                by_name.insert(var.as_str().to_owned(), value);
                by_mangled.insert(koat::mangle(var), value);
            }

            // The guard, both ways.
            let expected_guard = transition.guard().constraints().iter().all(|constraint| {
                let value = evaluate(constraint.polynomial(), &by_name)
                    .expect("probe valuations do not overflow");
                match constraint.relation() {
                    landav_its::Relation::Ge => value >= 0,
                    landav_its::Relation::Gt => value > 0,
                    landav_its::Relation::Eq => value == 0,
                }
            });
            let parsed_guard = rule.guard.iter().all(|(left, relation, right)| {
                let left = left.eval(&by_mangled).expect("no overflow");
                let right = right.eval(&by_mangled).expect("no overflow");
                match relation.as_str() {
                    ">=" => left >= right,
                    ">" => left > right,
                    "=" => left == right,
                    other => panic!("unknown relation `{other}` in emitted text"),
                }
            });
            assert_eq!(
                parsed_guard,
                expected_guard,
                "rule {index} guard disagrees with the transition at probe {probe}\n  \
                 emitted: {:?}\n  system:  {}",
                rule.guard,
                transition.guard()
            );

            // Each image, both ways.
            for (position, var) in its.vars().iter().enumerate() {
                let expected = match transition.update().get(var) {
                    Some(polynomial) => evaluate(polynomial, &by_name).expect("no overflow"),
                    // Unmentioned means unchanged, and the text must say so.
                    None => by_name.get(var.as_str()).copied().unwrap_or_default(),
                };
                let Some(image) = rule.images.get(position) else {
                    panic!("rule {index} has no image for `{var}`");
                };
                assert_eq!(
                    image.eval(&by_mangled),
                    Some(expected),
                    "rule {index} image for `{var}` disagrees with the transition at probe \
                     {probe}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// the properties
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, max_global_rejects: 0, ..ProptestConfig::default() })]

    /// **Escaping is injective.** Two distinct variable names never become one
    /// KoAT identifier.
    ///
    /// A soundness property rather than a cosmetic one: merging two variables
    /// silently equates them, which changes what every guard mentioning either
    /// one means, and not in the safe direction.
    ///
    /// # The alphabet is the whole test
    ///
    /// Drawing two independent strings from a wide alphabet finds nothing: the
    /// pairs that collide under a bad escaper differ only in *which*
    /// punctuation they use, and two independent 12-character samples over
    /// printable ASCII are essentially never such a pair. The first version of
    /// this test did exactly that and passed against a mangler that mapped
    /// every non-alphanumeric to `_`.
    ///
    /// So the alphabet is deliberately tiny and adversarial — one letter, the
    /// escape character, and four characters a naive escaper would collapse
    /// together — and a whole batch of names is checked at once, which turns
    /// "did these two collide" into "did any two of these collide".
    #[test]
    fn mangling_never_merges_two_names(
        names in prop::collection::hash_set("[aQ _.:-]{0,4}", 1..24),
    ) {
        let mut mangled: Vec<String> = names
            .iter()
            .map(|name| koat::mangle(&ItsVar::new(name.as_str())))
            .collect();
        let distinct_names = names.len();
        mangled.sort();
        mangled.dedup();
        prop_assert_eq!(
            mangled.len(),
            distinct_names,
            "{} distinct names mangled to {} distinct identifiers: {:?}",
            distinct_names,
            mangled.len(),
            names
        );
    }

    /// Every mangled name is a legal identifier: a letter, then letters,
    /// digits and underscores.
    #[test]
    fn mangling_always_produces_an_identifier(
        name in "[ -~\\u{00e9}\\u{4e16}]{0,12}",
    ) {
        let mangled = koat::mangle(&ItsVar::new(name.as_str()));
        let mut characters = mangled.chars();
        let first = characters.next();
        prop_assert!(
            first.is_some_and(|c| c.is_ascii_alphabetic()),
            "`{}` mangled to `{}`, which does not start with a letter", name, mangled
        );
        prop_assert!(
            characters.all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "`{}` mangled to `{}`, which is not an identifier", name, mangled
        );
    }

    /// **The emitted text means what the polynomial means.**
    ///
    /// Renders a polynomial, reads it back with the independent parser, and
    /// evaluates both at the same valuation. This is what makes the rendering
    /// a round trip rather than a hope.
    #[test]
    fn rendering_a_polynomial_preserves_its_value(
        coefficients in prop::collection::vec(-6_i64..=6, 1..5),
        exponents in prop::collection::vec(0_u32..3, 1..5),
        values in prop::collection::vec(-4_i128..=4, 3),
    ) {
        let names = ["x", "y", "z"];
        let mut monomials = Vec::new();
        for (index, coefficient) in coefficients.iter().enumerate() {
            let exponent = exponents.get(index).copied().unwrap_or(1);
            let var = ItsVar::new(names[index % names.len()]);
            if let Some(monomial) = landav_its::Monomial::new(*coefficient, [(var, exponent)]) {
                monomials.push(monomial);
            }
        }
        let Ok(polynomial) = Polynomial::from_monomials(monomials) else {
            // Past a cap; nothing to check.
            return Ok(());
        };

        let text = koat::render_polynomial(&polynomial);
        let ast = match parse_expr(&text) {
            Ok(ast) => ast,
            Err(error) => {
                prop_assert!(false, "rendered `{}` does not parse: {}", text, error);
                return Ok(());
            }
        };

        let mut by_mangled: BTreeMap<String, i128> = BTreeMap::new();
        let mut by_name: State = State::new();
        for (index, name) in names.iter().enumerate() {
            let value = values.get(index).copied().unwrap_or(0);
            by_mangled.insert(koat::mangle(&ItsVar::new(*name)), value);
            by_name.insert((*name).to_owned(), value);
        }

        prop_assert_eq!(
            ast.eval(&by_mangled),
            evaluate(&polynomial, &by_name),
            "`{}` evaluated differently after a round trip", text
        );
    }
}

/// Ordinary names pass through the escaper unchanged, and the document names
/// its locations `l0`, `l1`, ... .
///
/// Injectivity alone does not pin either: an escaper that escaped *everything*
/// is still injective and still produces identifiers, and a location renderer
/// that emitted the empty string is self-consistent between the declaration and
/// the rules, so a round-trip check agrees with itself. Both are pinned against
/// the literal text instead.
#[test]
fn ordinary_names_and_locations_render_literally() {
    assert_eq!(koat::mangle(&ItsVar::new("count")), "vcount");
    assert_eq!(koat::mangle(&ItsVar::new("_n1")), "v_n1");
    assert_eq!(koat::mangle(&ItsVar::new("")), "v");
    // Only the characters that must be escaped are.
    assert_eq!(koat::mangle(&ItsVar::new("a b")), "va".to_owned() + "Q20Qb");
    assert_eq!(koat::mangle(&ItsVar::new("Q")), "vQQ");

    let mut builder = SourceProgramBuilder::new(
        "named",
        Origin::new("named.py:1:1"),
        vec![VarName::new("n")],
    );
    let value = builder.int(1, Origin::new("named.py:2:1"));
    let assign = builder.assign(VarName::new("n"), value, Origin::new("named.py:2:1"));
    let program = builder.build(vec![assign]);
    let its = lower(&program).expect("inside the fragment");
    let text = its.to_koat();

    assert!(
        text.contains("(STARTTERM (FUNCTIONSYMBOLS l0))"),
        "the start term is not named l0:\n{text}"
    );
    assert!(
        text.contains("(VAR vn)"),
        "the variable tuple is wrong:\n{text}"
    );
    assert!(text.contains("l0(vn) ->"), "no rule leaves l0:\n{text}");
    assert!(
        text.starts_with("(GOAL COMPLEXITY)\n"),
        "the goal declaration is missing:\n{text}"
    );
}

/// A system with awkward variable names still emits a document that parses.
#[test]
fn awkward_variable_names_survive_the_emitter() {
    let mut builder = SourceProgramBuilder::new(
        "awkward",
        Origin::new("awkward.py:1:1"),
        vec![VarName::new("naïve count")],
    );
    let read = builder.var(VarName::new("naïve count"), Origin::new("awkward.py:2:1"));
    let one = builder.int(1, Origin::new("awkward.py:2:1"));
    let sum = builder.arith(
        landav_its::ArithOp::Add,
        read,
        one,
        Origin::new("awkward.py:2:1"),
    );
    let assign = builder.assign(
        VarName::new("total-so-far"),
        sum,
        Origin::new("awkward.py:2:1"),
    );
    let program = builder.build(vec![assign]);

    let its = lower(&program).expect("awkward names are not a refusal");
    let parsed = check_well_formed(&its);
    assert_eq!(parsed.variables.len(), its.vars().len());
}
