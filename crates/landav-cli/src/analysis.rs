//! The M0 Python scan.
//!
//! # Scope, stated honestly
//!
//! This is the structural half of `F-005`: the PERF/RUF-class rules that ship
//! value before bound inference works and validate the frontend plumbing. It
//! is a logical-line and indentation-oriented scan, **not a parser**, and it
//! derives no bounds. Real derivation is `landav-python` lowering to
//! `landav-its`, solved through `landav-solvers` and published as a
//! [`landav_bound::Verdict`] — none of which is implemented yet. This module
//! is a stopgap and should be deleted when that path lands.
//!
//! What matters for LAN-61 is that the scan produces the outcome shapes the
//! exit-code contract has to distinguish, and produces them without ever
//! panicking or guessing:
//!
//! * a rule fires against the code — a **finding**;
//! * a loop whose trip count is not bounded by any sized input — an
//!   **inconclusive** unit;
//! * a line the scan cannot recognise as a Python statement — also
//!   **inconclusive**, see below;
//! * neither — **clean**.
//!
//! # A scan that did not understand the input may not call it clean
//!
//! A line-oriented scan produces no observations for a file it cannot make
//! sense of, and "no observations" used to mean [`crate::outcome::Outcome`]`::Clean` —
//! which the contract defines as "analysis ran and every bound held". Nothing
//! held; nothing was even read as Python. A Python 2 module, a template, a
//! generated `.py` that is really JSON, a file with a syntax error: every one
//! of them took the quietest and most trusted code in the contract.
//!
//! So the scan now has to *recognise* what it reads. [`is_recognisable`]
//! checks each logical line against the statement shapes this scan models, and
//! anything outside them is reported as inconclusive with the line named. This
//! is not a syntax check and does not pretend to be one — it is the scan
//! declining to make a claim about input it did not understand.
//!
//! The direction of the remaining error matters: an unrecognised line that is
//! really valid Python costs a `1` on a file that deserved a `0`, which is
//! visible and arguable. The reverse — silence on a file nobody parsed —
//! is neither.
//!
//! # Logical lines
//!
//! Physical lines are joined into logical ones across explicit backslash
//! continuations, unclosed brackets, and triple-quoted strings, because the
//! rules are about statements and a statement is not a line. String literals
//! are replaced by [`STRING`] before matching, so a `#` or a keyword inside
//! one cannot be mistaken for code.

use std::fmt;

/// What a string literal is replaced by once the scan has read past it.
///
/// A single opaque token: it must survive as one word so that `print "x"` is
/// still visibly two juxtaposed values, which is what makes a Python 2 module
/// unrecognisable rather than clean.
const STRING: &str = "_str_";

/// What kind of thing the scan noticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A rule fired against the analysed code. Exit code `1`.
    Finding,
    /// Analysed, but no conclusion could be reached about this unit.
    /// Exit code `1`, and never `0`. See [`crate::outcome`].
    Inconclusive,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Finding => f.write_str("finding"),
            Self::Inconclusive => f.write_str("inconclusive"),
        }
    }
}

/// One thing the scan noticed, with enough blame attached to act on it.
#[derive(Debug, Clone)]
pub struct Observation {
    /// One-based number of the logical line's first physical line.
    pub line: usize,
    /// Whether this is a finding or an unaccounted term.
    pub kind: Kind,
    /// The rule or assumption identifier.
    pub rule: &'static str,
    /// What the operator needs to know, in their terms.
    pub message: String,
}

/// The result of scanning one file.
#[derive(Debug, Clone, Default)]
pub struct Scan {
    /// Everything noticed, in line order.
    pub observations: Vec<Observation>,
    /// Logical statements seen. Zero means there was nothing to analyse, which
    /// is not the same as nothing to report.
    pub statements: usize,
}

/// Which kind of block a header opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    /// `for` or `while` — statements inside repeat.
    Loop,
    /// Anything else that opens a suite.
    Other,
}

/// Keywords that open a suite.
const HEADERS: [&str; 15] = [
    "if", "elif", "else", "for", "while", "def", "class", "try", "except", "finally", "with",
    "match", "case", "async", "lambda",
];

/// Keywords that begin a simple statement.
const SIMPLE: [&str; 13] = [
    "return", "pass", "break", "continue", "import", "from", "raise", "assert", "del", "global",
    "nonlocal", "yield", "await",
];

/// Words that may sit between two values without being a juxtaposition.
///
/// These are the operators and connectives Python spells with letters. Two
/// *other* words side by side with nothing between them are not an expression
/// in any Python version this tool targets.
const CONNECTIVES: [&str; 14] = [
    "and", "or", "not", "in", "is", "if", "else", "for", "lambda", "await", "yield", "async",
    "None", "as",
];

/// Scan one file's text.
pub fn scan(text: &str) -> Scan {
    let mut result = Scan::default();
    let mut open: Vec<(usize, Block)> = Vec::new();

    for statement in logical_lines(text) {
        result.statements += 1;

        // Close every suite this line has dedented out of.
        while open.last().is_some_and(|&(at, _)| statement.indent <= at) {
            open.pop();
        }

        if statement.malformed || !is_recognisable(&statement.text) {
            // The scan did not understand this line, so it has no basis for a
            // claim about the enclosing unit — and no basis for tracking the
            // block structure past it either.
            result.observations.push(Observation {
                line: statement.line,
                kind: Kind::Inconclusive,
                rule: "unrecognised-statement",
                message: format!(
                    "`{}` was not recognised as a Python statement, so this file was \
                     never read as Python and no bound was derived from it",
                    statement.text.trim()
                ),
            });
            continue;
        }

        let in_loop = open.iter().any(|&(_, block)| block == Block::Loop);
        let body = statement.text.as_str();

        if let Some(condition) = keyword_argument(body, "while") {
            // The whole of criterion 3 hangs on this arm. A `while` has no
            // syntactic trip count: nothing in the loop header names a sized
            // input, so termination is an assumption the analyser cannot
            // discharge. Reporting the enclosing unit clean would assert a
            // property nobody proved.
            result.observations.push(Observation {
                line: statement.line,
                kind: Kind::Inconclusive,
                rule: "unbounded-loop",
                message: format!(
                    "the trip count of `while {condition}` is not bounded by any sized \
                     input, so termination could not be established and no bound was \
                     derived for the enclosing function"
                ),
            });
            open.push((statement.indent, Block::Loop));
            continue;
        }

        if keyword_argument(body, "for").is_some() {
            // A `for` over a sized input is the bounded case, and the `in` of
            // its own header is not a membership test.
            open.push((statement.indent, Block::Loop));
            continue;
        }

        if in_loop {
            result
                .observations
                .extend(loop_body_rules(statement.line, body));
        }

        if let Some(keyword) = opening_keyword(body) {
            let block = if keyword == "while" || keyword == "for" {
                Block::Loop
            } else {
                Block::Other
            };
            open.push((statement.indent, block));
        }
    }

    result.observations.sort_by_key(|o| o.line);
    result
}

// ---------------------------------------------------------------------------
// Logical lines
// ---------------------------------------------------------------------------

/// One statement, with any continuation lines folded into it.
#[derive(Debug)]
struct Logical {
    /// One-based number of the first physical line.
    line: usize,
    /// Indentation of the first physical line.
    indent: usize,
    /// The joined text, with string literals replaced by [`STRING`].
    text: String,
    /// Whether the brackets failed to balance across the statement.
    malformed: bool,
}

/// Carried between physical lines while joining.
#[derive(Debug, Default)]
struct State {
    /// Open bracket depth.
    depth: i32,
    /// The quote character of an open triple-quoted string, if any.
    triple: Option<char>,
}

/// Split `text` into logical lines.
fn logical_lines(text: &str) -> Vec<Logical> {
    let mut out = Vec::new();
    let mut state = State::default();
    let mut buffer = String::new();
    let mut line = 0usize;
    let mut indent = 0usize;
    let mut malformed = false;

    for (index, raw) in text.lines().enumerate() {
        let (code, unmatched) = sanitise(raw, &mut state);
        if unmatched {
            malformed = true;
        }
        let trimmed = code.trim();

        if buffer.is_empty() {
            if trimmed.is_empty() {
                continue;
            }
            line = index + 1;
            indent = raw.len() - raw.trim_start().len();
        }

        let continued = trimmed.ends_with('\\');
        let piece = if continued {
            trimmed.trim_end_matches('\\').trim_end()
        } else {
            trimmed
        };
        if !buffer.is_empty() && !piece.is_empty() {
            buffer.push(' ');
        }
        buffer.push_str(piece);

        if continued || state.depth > 0 || state.triple.is_some() {
            continue;
        }
        out.push(Logical {
            line,
            indent,
            text: std::mem::take(&mut buffer),
            malformed,
        });
        malformed = false;
    }

    if !buffer.is_empty() {
        out.push(Logical {
            line,
            indent,
            text: buffer,
            malformed: malformed || state.depth != 0 || state.triple.is_some(),
        });
    }
    out
}

/// Strip comments, replace string literals, and track brackets.
///
/// Returns the line's code and whether it closed a bracket that was never
/// opened, which is the cheap half of "this is not Python".
fn sanitise(raw: &str, state: &mut State) -> (String, bool) {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len());
    let mut unmatched = false;
    let mut i = 0usize;

    while i < chars.len() {
        // Inside a triple-quoted string: everything is content until it closes.
        if let Some(quote) = state.triple {
            if chars[i] == quote
                && chars.get(i + 1) == Some(&quote)
                && chars.get(i + 2) == Some(&quote)
            {
                state.triple = None;
                i += 3;
            } else {
                i += 1;
            }
            continue;
        }

        let ch = chars[i];
        match ch {
            '#' => break,
            '\'' | '"' => {
                // `f"..."`, `rb'...'` and friends: the prefix letters are part
                // of the literal, not an identifier sitting next to it.
                drop_string_prefix(&mut out);
                if chars.get(i + 1) == Some(&ch) && chars.get(i + 2) == Some(&ch) {
                    // The placeholder is emitted when the string *opens*, so a
                    // docstring is one logical line beginning at its first line.
                    out.push_str(STRING);
                    state.triple = Some(ch);
                    i += 3;
                } else {
                    out.push_str(STRING);
                    i = end_of_string(&chars, i, ch);
                }
            }
            '(' | '[' | '{' => {
                state.depth += 1;
                out.push(ch);
                i += 1;
            }
            ')' | ']' | '}' => {
                if state.depth == 0 {
                    unmatched = true;
                } else {
                    state.depth -= 1;
                }
                out.push(ch);
                i += 1;
            }
            _ => {
                out.push(ch);
                i += 1;
            }
        }
    }
    (out, unmatched)
}

/// Remove a string literal's prefix letters from the end of `out`.
///
/// `f`, `r`, `b`, `u` and the two-letter combinations are part of the literal.
/// Left in place they read as an identifier immediately before a string, which
/// would make every `f"..." "..."` in a real codebase look like two juxtaposed
/// values rather than one implicitly concatenated literal.
fn drop_string_prefix(out: &mut String) {
    let letters = out
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphabetic())
        .count();
    if letters == 0 || letters > 2 {
        return;
    }
    let start = out.len() - letters;
    // Only a standalone prefix counts; `name"x"` is not a prefixed literal.
    if out[..start]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        return;
    }
    if out[start..]
        .chars()
        .all(|c| matches!(c.to_ascii_lowercase(), 'f' | 'r' | 'b' | 'u'))
    {
        out.truncate(start);
    }
}

/// The index just past a single-quoted string opened at `start`.
///
/// An unterminated one runs to the end of the line rather than being reported:
/// this scan does not do syntax, and a stray quote must not turn a whole file
/// inconclusive.
fn end_of_string(chars: &[char], start: usize, quote: char) -> usize {
    let mut i = start + 1;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    chars.len()
}

// ---------------------------------------------------------------------------
// Recognition
// ---------------------------------------------------------------------------

/// Whether the scan recognises `text` as a Python statement shape.
///
/// Deliberately permissive about *expressions* and strict about *shape*. The
/// job is not to validate Python — that is a parser's, and this is not one —
/// but to refuse to stay silent about input the scan plainly did not read as
/// code.
fn is_recognisable(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    // A compound statement, with or without an inline body.
    if HEADERS
        .iter()
        .any(|kw| keyword_argument(text, kw).is_some())
    {
        return true;
    }
    // A simple statement introduced by a keyword.
    if SIMPLE.iter().any(|kw| keyword_argument(text, kw).is_some()) {
        return true;
    }
    // An assignment, plain or augmented.
    if has_assignment(text) {
        return true;
    }
    is_expression_statement(text)
}

/// Whether `text` assigns at bracket depth zero.
///
/// Accepts `x = 1` and every augmented form; rejects the comparisons, whose
/// `=` is part of a two-character operator, and keyword arguments, whose `=`
/// is inside brackets.
fn has_assignment(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut depth = 0i32;
    for (i, &ch) in chars.iter().enumerate() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '=' if depth == 0 => {
                let before = i.checked_sub(1).and_then(|j| chars.get(j)).copied();
                let after = chars.get(i + 1).copied();
                if after == Some('=') || matches!(before, Some('=' | '!' | '<' | '>')) {
                    continue;
                }
                // Something has to be assigned *to*.
                let target: String = chars[..i].iter().collect();
                let target = target.trim_end_matches(['+', '-', '*', '/', '%', '&', '|', '^', '@']);
                return !target.trim().is_empty();
            }
            _ => {}
        }
    }
    false
}

/// Whether `text` is a plausible bare expression statement.
///
/// Two things disqualify it. Starting with `{` or `[`: a statement that builds
/// a collection and discards it has no effect in Python, and as the whole
/// content of a module it is a renamed data file — the JSON case. And two
/// values side by side with only whitespace between them, which is not an
/// expression in any Python this targets — the prose case, and the Python 2
/// `print "x"` case.
fn is_expression_statement(text: &str) -> bool {
    // `...` is a literal and the idiomatic body of a stub or a Protocol method.
    if text.starts_with("...") {
        return true;
    }
    let Some(first) = text.chars().next() else {
        return false;
    };
    if !(first.is_alphanumeric() || first == '_' || first == '(' || first == '@') {
        return false;
    }
    !has_juxtaposed_values(text)
}

/// Whether two value tokens sit side by side with nothing but space between.
fn has_juxtaposed_values(text: &str) -> bool {
    let mut previous: Option<String> = None;
    let mut current = String::new();
    let mut gap = false;

    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
            continue;
        }
        if !current.is_empty() {
            if gap
                && previous
                    .as_ref()
                    .is_some_and(|prev| juxtaposed(prev, &current))
            {
                return true;
            }
            previous = Some(std::mem::take(&mut current));
        }
        // Only whitespace may separate two values and still count as adjacent;
        // any operator or punctuation resets the pairing.
        if ch.is_whitespace() {
            gap = true;
        } else {
            previous = None;
            gap = false;
        }
    }

    if !current.is_empty()
        && gap
        && previous
            .as_ref()
            .is_some_and(|prev| juxtaposed(prev, &current))
    {
        return true;
    }
    false
}

/// Whether `left` and `right`, seen side by side, are not an expression.
///
/// Two adjacent string literals *are* one: Python concatenates them, and
/// `warnings.warn("a long message " "split across lines")` is ubiquitous — it
/// appears in nineteen percent of the standard library's top-level modules. A
/// rule that called that unrecognised would report a fifth of every real
/// codebase as inconclusive, which is how a gate stops being believed.
fn juxtaposed(left: &str, right: &str) -> bool {
    if left == STRING && right == STRING {
        return false;
    }
    !is_connective(left) && !is_connective(right)
}

/// Whether `word` is a Python operator spelled with letters.
fn is_connective(word: &str) -> bool {
    CONNECTIVES.contains(&word) || SIMPLE.contains(&word) || HEADERS.contains(&word)
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

/// The `F-005` PERF/RUF rules that only mean anything inside a loop.
fn loop_body_rules(line: usize, body: &str) -> Vec<Observation> {
    let mut found = Vec::new();

    if let Some(name) = self_concatenation(body) {
        found.push(Observation {
            line,
            kind: Kind::Finding,
            rule: "list-concat-in-loop",
            message: format!(
                "`{name}` is rebuilt with `+` on every iteration, which copies the \
                 whole accumulator each time and makes the loop quadratic; append \
                 in place instead"
            ),
        });
    }

    if is_condition(body) && body.contains(" in ") {
        found.push(Observation {
            line,
            kind: Kind::Finding,
            rule: "membership-test-in-loop",
            message: "a membership test inside a loop scans the container on every \
                      iteration, which makes the loop quadratic when the container is \
                      a list; test against a set instead"
                .to_owned(),
        });
    }

    if body.contains(".index(") {
        found.push(Observation {
            line,
            kind: Kind::Finding,
            rule: "index-lookup-in-loop",
            message: "`.index()` inside a loop rescans from the start on every \
                      iteration, which makes the loop quadratic"
                .to_owned(),
        });
    }

    found
}

/// The name in `x = x + ...`, the quadratic accumulation shape.
///
/// Deliberately narrow. `x += y` is an in-place extend and is *not* quadratic,
/// and `n = 3 * n + 1` does not rebuild an accumulator, so neither matches.
fn self_concatenation(body: &str) -> Option<&str> {
    let (lhs, rhs) = body.split_once('=')?;
    // Reject the comparison and augmented-assignment operators, whose `=` this
    // would otherwise split on: `==`, `!=`, `<=`, `>=`, `+=`, and friends.
    if rhs.starts_with('=') {
        return None;
    }
    let name = lhs.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let rest = rhs.trim_start().strip_prefix(name)?.trim_start();
    let mut chars = rest.chars();
    match (chars.next(), chars.next()) {
        (Some('+'), Some('=')) => None,
        (Some('+'), _) => Some(name),
        _ => None,
    }
}

/// Whether this statement is a condition, where ` in ` is a membership test.
fn is_condition(body: &str) -> bool {
    ["if", "elif", "assert", "return", "while"]
        .iter()
        .any(|kw| keyword_argument(body, kw).is_some())
}

/// The text after `keyword`, if `body` is that kind of statement.
///
/// Matches on a word boundary so that `format(x)` is not a `for` and
/// `iffy = 1` is not an `if`.
fn keyword_argument<'a>(body: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = body.strip_prefix(keyword)?;
    if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(rest.trim().trim_end_matches(':').trim())
}

/// The suite-opening keyword of `body`, if it has one.
fn opening_keyword(body: &str) -> Option<&'static str> {
    if !body.ends_with(':') {
        return None;
    }
    // `async for` and `async with` are the block they wrap.
    let body = keyword_argument(body, "async").unwrap_or(body);
    HEADERS
        .iter()
        .copied()
        .find(|kw| keyword_argument(body, kw).is_some())
}

#[cfg(test)]
mod tests {
    use super::{Kind, is_recognisable, logical_lines, scan, self_concatenation};

    const CLEAN: &str = "
def total(items):
    acc = 0
    for item in items:
        acc += item
    return acc
";

    const FINDINGS: &str = "
def intersect(xs, ys):
    out = []
    for x in xs:
        if x in ys:
            out = out + [x]
    return out
";

    const INCONCLUSIVE: &str = "
def collatz_steps(n):
    steps = 0
    while n != 1:
        if n % 2 == 0:
            n = n // 2
        else:
            n = 3 * n + 1
        steps += 1
    return steps
";

    #[test]
    fn a_bounded_loop_reports_nothing() {
        let result = scan(CLEAN);
        assert!(
            result.observations.is_empty(),
            "the `in` of a `for` header is not a membership test, and `+=` is not \
             a quadratic rebuild: {:?}",
            result.observations
        );
        assert!(result.statements > 0);
    }

    #[test]
    fn quadratic_shapes_are_findings_not_tool_errors() {
        let result = scan(FINDINGS);
        assert!(
            result.observations.iter().all(|o| o.kind == Kind::Finding),
            "{:?}",
            result.observations
        );
        let rules: Vec<&str> = result.observations.iter().map(|o| o.rule).collect();
        assert!(rules.contains(&"membership-test-in-loop"), "{rules:?}");
        assert!(rules.contains(&"list-concat-in-loop"), "{rules:?}");
    }

    #[test]
    fn an_unbounded_loop_is_inconclusive_and_carries_blame() {
        let result = scan(INCONCLUSIVE);
        let mut blamed = result
            .observations
            .iter()
            .filter(|o| o.kind == Kind::Inconclusive);
        let first = blamed.next();
        assert!(
            first.is_some_and(|o| o.line == 4
                && o.message.contains("while n != 1")
                && !o.message.contains("unknown")),
            "an unaccounted term must name the loop it could not discharge: {:?}",
            result.observations
        );
        assert!(blamed.next().is_none(), "{:?}", result.observations);
    }

    #[test]
    fn arithmetic_reassignment_is_not_a_quadratic_rebuild() {
        assert_eq!(self_concatenation("out = out + [x]"), Some("out"));
        assert_eq!(self_concatenation("n = 3 * n + 1"), None);
        assert_eq!(self_concatenation("n = n // 2"), None);
        assert_eq!(self_concatenation("acc += item"), None);
        assert_eq!(self_concatenation("if n % 2 == 0:"), None);
        assert_eq!(self_concatenation("out = outer + 1"), None);
    }

    #[test]
    fn an_empty_file_has_nothing_to_analyse() {
        assert_eq!(scan("").statements, 0);
        assert_eq!(scan("\n\n# just a comment\n").statements, 0);
    }

    /// A backslash continuation splits a statement across physical lines. The
    /// rules are about statements, so the join has to happen first.
    #[test]
    fn a_continuation_does_not_hide_a_finding() {
        let continued = "def intersect(xs, ys):\n\
                         \x20   out = []\n\
                         \x20   for x in xs:\n\
                         \x20       if x \\\n\
                         \x20          in ys:\n\
                         \x20           out = out \\\n\
                         \x20               + [x]\n\
                         \x20   return out\n";
        let rules: Vec<&str> = scan(continued)
            .observations
            .iter()
            .map(|o| o.rule)
            .collect();
        assert!(rules.contains(&"membership-test-in-loop"), "{rules:?}");
        assert!(rules.contains(&"list-concat-in-loop"), "{rules:?}");
    }

    /// An unclosed bracket folds the following lines into one statement, and
    /// the statement never balances.
    #[test]
    fn a_syntax_error_is_not_clean() {
        let result = scan("def f(:\n    return ]]] @@@\n");
        assert!(
            result
                .observations
                .iter()
                .any(|o| o.kind == Kind::Inconclusive),
            "{result:?}"
        );
    }

    #[test]
    fn input_that_is_not_python_is_not_recognised() {
        assert!(!is_recognisable(
            "This file is a README that someone renamed."
        ));
        assert!(!is_recognisable("{_str_: _str_, _str_: _str_}"));
        assert!(!is_recognisable("print _str_"));
    }

    /// The recogniser has to accept ordinary Python, or every real run goes
    /// inconclusive and the exit code stops meaning anything.
    #[test]
    fn ordinary_python_is_recognised() {
        for line in [
            "def total(items):",
            "async def fetch(url):",
            "class Thing:",
            "acc = 0",
            "acc += item",
            "self.items[key] = value",
            "return acc",
            "return a if b else c",
            "yield x",
            "raise ValueError(_str_)",
            "import os",
            "from a.b import c as d",
            "with open(path) as handle:",
            "except ValueError as err:",
            "else:",
            "for item in items:",
            "while n != 1:",
            "if x in ys:",
            "print(value)",
            "handler.dispatch(event, retries=3)",
            "obj.method().chained()",
            "_str_",
            "@decorator",
            "assert x, _str_",
            "del cache[key]",
            "lambda x: x",
            "(a, b) = pair",
            "not_a_keyword_call()",
        ] {
            assert!(is_recognisable(line), "rejected valid Python: `{line}`");
        }
    }

    /// Implicit string concatenation is one literal, not two juxtaposed
    /// values. Measured against the standard library, treating it as a
    /// juxtaposition reported nineteen percent of top-level modules as
    /// inconclusive — a rate at which nobody believes the exit code.
    #[test]
    fn implicit_string_concatenation_is_recognised() {
        for line in [
            "warnings.warn(_str_ _str_, DeprecationWarning, stacklevel=2)",
            "result.append(_str_ _str_)",
            "parser.error(_str_ _str_ _str_)",
        ] {
            assert!(is_recognisable(line), "rejected valid Python: `{line}`");
        }
    }

    /// A prefixed literal is a literal. Left unhandled, `f"a" "b"` sanitises
    /// to `f_str_ _str_` and reads as an identifier beside a string.
    #[test]
    fn string_prefixes_are_part_of_the_literal() {
        let text = |src: &str| {
            logical_lines(src)
                .first()
                .map(|l| l.text.clone())
                .unwrap_or_default()
        };
        assert_eq!(text("x = f\"a{b}c\"\n"), "x = _str_");
        assert_eq!(text("x = rb'raw'\n"), "x = _str_");
        assert_eq!(text("warn(f\"a\" \"b\")\n"), "warn(_str_ _str_)");
        // Not a prefix: an identifier that merely ends in a prefix letter.
        assert_eq!(text("conf = \"a\"\n"), "conf = _str_");
    }

    /// `...` is the idiomatic body of a stub and of a Protocol method.
    #[test]
    fn an_ellipsis_body_is_recognised() {
        assert!(is_recognisable("..."));
    }

    /// A docstring is one logical line, and its prose is never read as code.
    #[test]
    fn a_docstring_is_not_read_as_code() {
        let source =
            "def f():\n    \"\"\"\n    Returns a thing, eventually.\n    \"\"\"\n    return 1\n";
        let result = scan(source);
        assert!(
            result.observations.is_empty(),
            "docstring prose was read as code: {:?}",
            result.observations
        );
    }

    #[test]
    fn a_hash_inside_a_string_does_not_start_a_comment() {
        let lines = logical_lines("x = \"a # b\"  # tail\n");
        let first = lines.first().map(|l| l.text.clone());
        assert_eq!(first.as_deref(), Some("x = _str_"));
    }
}
