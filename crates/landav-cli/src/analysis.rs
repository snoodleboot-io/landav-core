//! The M0 Python scan.
//!
//! # Scope, stated honestly
//!
//! This is the structural half of `F-005`: the PERF/RUF-class rules that ship
//! value before bound inference works and validate the frontend plumbing.
//! It is a line- and indentation-oriented scan, not a parser, and it does not
//! derive bounds. Real derivation is `landav-python` lowering to
//! `landav-its`, solved through `landav-solvers`, published as a
//! [`landav_bound::Verdict`] — none of which is implemented yet.
//!
//! What matters for LAN-61 is that the scan produces the *three outcome
//! shapes* the exit-code contract has to distinguish, and that it produces
//! them without ever panicking or guessing:
//!
//! * a rule fires against the code — a **finding**;
//! * a loop whose trip count is not bounded by any sized input — an
//!   **inconclusive** unit, which carries blame naming the loop and must never
//!   report clean;
//! * neither — **clean**.
//!
//! Known limitations, deliberate at this scope: triple-quoted strings are not
//! tracked, so a `#` inside one is treated as a comment, and line
//! continuations are not joined. Both make the scan *less* likely to fire a
//! rule, never more, which is the safe direction for a tool whose findings are
//! meant to be acted on.

use std::fmt;

/// What kind of thing the scan noticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A rule fired against the analysed code. Exit code `1`.
    Finding,
    /// Analysed, but no conclusion could be reached about this unit.
    /// Exit code `1`, and never `0`. See `crate::outcome`.
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
    /// One-based line number within the file.
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
    /// Non-blank, non-comment lines. Zero means there was nothing to analyse,
    /// which is not the same as nothing to report.
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
const HEADERS: [&str; 12] = [
    "if", "elif", "else", "for", "while", "def", "class", "try", "except", "finally", "with",
    "match",
];

/// Scan one file's text.
pub fn scan(text: &str) -> Scan {
    let mut result = Scan::default();
    let mut open: Vec<(usize, Block)> = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let code = strip_comment(raw);
        let body = code.trim();
        if body.is_empty() {
            continue;
        }
        let indent = code.len() - code.trim_start().len();

        // Close every suite this line has dedented out of.
        while open.last().is_some_and(|&(at, _)| indent <= at) {
            open.pop();
        }
        let in_loop = open.iter().any(|&(_, block)| block == Block::Loop);
        result.statements += 1;

        if let Some(condition) = keyword_argument(body, "while") {
            // The whole of criterion 3 hangs on this arm. A `while` has no
            // syntactic trip count: nothing in the loop header names a sized
            // input, so termination is an assumption the analyser cannot
            // discharge. Reporting the enclosing unit clean would assert a
            // property nobody proved.
            result.observations.push(Observation {
                line,
                kind: Kind::Inconclusive,
                rule: "unbounded-loop",
                message: format!(
                    "the trip count of `while {condition}` is not bounded by any sized \
                     input, so termination could not be established and no bound was \
                     derived for the enclosing function"
                ),
            });
            open.push((indent, Block::Loop));
            continue;
        }

        if keyword_argument(body, "for").is_some() {
            // A `for` over a sized input is the bounded case, and the `in` of
            // its own header is not a membership test.
            open.push((indent, Block::Loop));
            continue;
        }

        if in_loop {
            result.observations.extend(loop_body_rules(line, body));
        }

        if let Some(keyword) = opening_keyword(body) {
            let block = if keyword == "while" || keyword == "for" {
                Block::Loop
            } else {
                Block::Other
            };
            open.push((indent, block));
        }
    }

    result.observations.sort_by_key(|o| o.line);
    result
}

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
    HEADERS
        .iter()
        .copied()
        .find(|kw| keyword_argument(body, kw).is_some())
}

/// Drop a trailing comment, without being fooled by a `#` inside a string.
fn strip_comment(raw: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (offset, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match (quote, ch) {
            (Some(_), '\\') => escaped = true,
            (Some(open), c) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(ch),
            (None, '#') => return &raw[..offset],
            (None, _) => {}
        }
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::{Kind, scan, self_concatenation, strip_comment};

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

    #[test]
    fn a_hash_inside_a_string_does_not_start_a_comment() {
        assert_eq!(strip_comment("x = \"a # b\"  # tail"), "x = \"a # b\"  ");
        assert_eq!(strip_comment("plain"), "plain");
    }
}
