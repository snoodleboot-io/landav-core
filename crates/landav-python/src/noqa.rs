//! `# noqa` — the inline suppression comment, and nothing else.
//!
//! This is a Python fact and lives here for the same reason the parser does:
//! deciding whether a `#` opens a comment or sits inside a string literal is a
//! question only the frontend can answer, and a driver that guessed would
//! honour `SEPARATOR = "# noqa: LAV003"` as a waiver.
//!
//! # The spelling, and what landav does not claim
//!
//! ```text
//! out += piece  # noqa: LAV003 - vendored, rewrite tracked in LAN-70
//! out += piece  # noqa: LAV002, LAV003
//! out += piece  # noqa: E501, LAV003
//! ```
//!
//! `# noqa` is `flake8`'s spelling and `ruff` reads it too, so the comment is
//! shared property. Two consequences, both deliberate:
//!
//! * **A directive that names no `LAV` code is not landav's.** `# noqa: E501`
//!   is somebody else's suppression sitting in a file landav happens to read,
//!   and reporting it as an unrecognised landav code would make landav the
//!   noisy tool in every repository that already uses `ruff`. It is passed
//!   over in silence. A token that *does* begin with `LAV` is claimed, valid
//!   or not, because it was plainly aimed here.
//! * **There is no bare `# noqa` form.** A blanket waiver silences every rule
//!   this build has *and every rule the next release adds*, at a line whose
//!   author only ever looked at one of them; that is precisely how a codebase
//!   goes permanently quiet. It also destroys `LAN-66` criterion 3: a record
//!   that says "everything was waived here" has nothing an approver can
//!   approve or an expiry can expire. So a bare `# noqa` waives nothing in
//!   landav — and is not reported either, because it is a directive to another
//!   tool and narrating other people's comments is its own kind of noise.
//!
//! # Extent
//!
//! A directive covers **the physical line the comment is on**, and no other.
//! Findings point at the start of the offending expression rather than at the
//! enclosing statement, so the natural place to write the comment is already
//! the line the finding is reported on. A comment on a neighbouring line
//! waives nothing and is reported as unused, which is the honest answer: the
//! alternative — quietly extending a waiver to a line its author did not look
//! at — is the blanket problem wearing a smaller hat.

use crate::syntax::skip_string;

/// The prefix that makes a code landav's business.
const LANDAV_PREFIX: &str = "LAV";

/// The directive keyword, matched case-insensitively as a whole word.
const NOQA: &str = "noqa";

/// Characters that may introduce the free-text reason after the codes.
const REASON_LEADERS: [char; 5] = ['-', '–', '—', '#', ':'];

/// One `# noqa` comment that names at least one landav code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Directive {
    /// The 1-based physical line the comment is on, which is the only line it
    /// covers.
    pub(crate) line: u32,
    /// The `LAV`-prefixed codes it names, verbatim, in the order written.
    pub(crate) codes: Vec<String>,
    /// The free text after the codes, if any.
    pub(crate) reason: Option<String>,
}

/// Every landav suppression directive in `source`, in ascending line order.
///
/// Never panics: a source that ends inside a string literal, or that is not
/// where the byte scanner thinks it is, yields fewer directives rather than an
/// index out of bounds.
pub(crate) fn directives(source: &str) -> Vec<Directive> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut index = 0_usize;
    let mut line = 1_u32;

    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                line = line.saturating_add(1);
                index += 1;
            }
            b'"' | b'\'' => {
                // A `#` inside a literal is not a comment, and a triple-quoted
                // literal carries the line counter across its own newlines.
                //
                // The `index + 1` floor is unobservable today — `skip_string`
                // always moves past at least the opening quote, so no input
                // reaches it, and `cargo mutants` reports removing it as a
                // surviving equivalent mutant. It stays: it is the only thing
                // making progress a *local* guarantee, and a scanner that
                // fails to advance hangs the process rather than failing it.
                let end = skip_string(bytes, index).clamp(index + 1, bytes.len());
                line = line.saturating_add(newlines(&bytes[index..end]));
                index = end;
            }
            b'#' => {
                let end = bytes[index..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(bytes.len(), |offset| index + offset);
                // The `#` is carried into `parse` rather than trimmed off: it
                // is inert to every step of the directive grammar, and slicing
                // past it is one more offset that could be wrong.
                if let Some(comment) = source.get(index..end)
                    && let Some(directive) = parse(line, comment)
                {
                    found.push(directive);
                }
                index = end;
            }
            _ => index += 1,
        }
    }

    found
}

/// How many `\n` bytes `span` contains.
fn newlines(span: &[u8]) -> u32 {
    let count = span.iter().filter(|byte| **byte == b'\n').count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Reads one whole comment, `#` included, as a directive.
///
/// `None` when the comment is not a landav suppression: no `noqa` keyword, no
/// colon after it, or no `LAV`-prefixed code among the ones it names.
fn parse(line: u32, comment: &str) -> Option<Directive> {
    let body = after_noqa(comment)?.trim_start();
    // A bare `# noqa` has no colon and is not ours. See the module docs.
    let body = body.strip_prefix(':')?;

    let (tokens, tail) = split_codes(body);
    let codes: Vec<String> = tokens.into_iter().filter(|code| is_landav(code)).collect();
    if codes.is_empty() {
        return None;
    }

    let reason = tail.trim().trim_start_matches(REASON_LEADERS).trim();
    Some(Directive {
        line,
        codes,
        reason: (!reason.is_empty()).then(|| reason.to_owned()),
    })
}

/// The text after a standalone `noqa` keyword, or `None`.
///
/// Standalone matters: `# nonoqa` and `# noqad` are not directives, and a
/// substring search alone would read both as one.
fn after_noqa(comment: &str) -> Option<&str> {
    let lowered = comment.to_ascii_lowercase();
    let mut from = 0_usize;
    while let Some(offset) = lowered.get(from..).and_then(|rest| rest.find(NOQA)) {
        let start = from + offset;
        let end = start + NOQA.len();
        let before_free = start == 0 || !is_word_byte(lowered.as_bytes()[start - 1]);
        let after_free = lowered
            .as_bytes()
            .get(end)
            .is_none_or(|byte| !is_word_byte(*byte));
        if before_free && after_free {
            return comment.get(end..);
        }
        from = end;
    }
    None
}

/// Whether `byte` can appear inside an identifier.
const fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Splits the text after `noqa:` into the leading run of code-shaped tokens
/// and whatever follows.
///
/// Tokens are separated by whitespace or commas. The run stops at the first
/// token that is not code-shaped, and everything from there is the reason —
/// which is what lets `# noqa: LAV003 vendored dependency` work without a
/// delimiter nobody would remember to type.
fn split_codes(body: &str) -> (Vec<String>, &str) {
    let mut codes = Vec::new();
    let mut rest = body;

    loop {
        let trimmed = rest.trim_start_matches([' ', '\t', ',']);
        let end = trimmed.find([' ', '\t', ',']).unwrap_or(trimmed.len());
        let token = trimmed.get(..end).unwrap_or_default();
        if token.is_empty() || !is_code_shaped(token) {
            return (codes, trimmed);
        }
        codes.push(token.to_owned());
        rest = trimmed.get(end..).unwrap_or_default();
    }
}

/// Whether `token` looks like any linter's rule code: letters then digits,
/// nothing else.
///
/// The test is deliberately generous — `E501`, `PERF401` and `LAV003` all pass
/// — because its job is only to find where the codes stop and the prose
/// starts. Which of them landav owns is [`is_landav`]'s question.
fn is_code_shaped(token: &str) -> bool {
    let mut bytes = token.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && token.bytes().all(|byte| byte.is_ascii_alphanumeric())
        && token.bytes().any(|byte| byte.is_ascii_digit())
}

/// Whether `token` is aimed at landav.
///
/// Case-insensitive on the prefix so that `lav003` is claimed and reported as
/// the typo it is, rather than silently mistaken for another tool's code.
fn is_landav(token: &str) -> bool {
    token
        .get(..LANDAV_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(LANDAV_PREFIX))
}

#[cfg(test)]
mod tests {
    use super::{Directive, directives};

    fn one(source: &str) -> Option<Directive> {
        let mut found = directives(source);
        (found.len() == 1).then(|| found.remove(0))
    }

    fn codes(source: &str) -> Vec<Vec<String>> {
        directives(source)
            .into_iter()
            .map(|directive| directive.codes)
            .collect()
    }

    #[test]
    fn a_directive_names_its_line_and_code() {
        let directive = one("x = 1\ny = 2  # noqa: LAV003\n");
        assert_eq!(
            directive,
            Some(Directive {
                line: 2,
                codes: vec!["LAV003".to_owned()],
                reason: None,
            })
        );
    }

    #[test]
    fn several_codes_and_a_reason_are_read_together() {
        let directive = one("y = 2  # noqa: LAV002, LAV003 - vendored, see LAN-70\n");
        assert_eq!(
            directive.as_ref().map(|d| d.codes.clone()),
            Some(vec!["LAV002".to_owned(), "LAV003".to_owned()])
        );
        assert_eq!(
            directive.and_then(|d| d.reason),
            Some("vendored, see LAN-70".to_owned())
        );
    }

    #[test]
    fn a_reason_without_a_dash_is_still_a_reason() {
        let directive = one("y = 2  # noqa: LAV003 generated file\n");
        assert_eq!(
            directive.and_then(|d| d.reason),
            Some("generated file".to_owned())
        );
    }

    /// `# noqa` with no code is another tool's blanket directive. Landav does
    /// not honour it and does not narrate it.
    #[test]
    fn a_bare_noqa_is_not_a_landav_directive() {
        assert!(directives("y = 2  # noqa\n").is_empty());
        assert!(directives("y = 2  # noqa \n").is_empty());
        assert!(directives("y = 2  # NOQA\n").is_empty());
    }

    /// Another tool's codes are left entirely alone, including in a directive
    /// that also names one of ours.
    #[test]
    fn foreign_codes_are_not_claimed() {
        assert!(directives("y = 2  # noqa: E501\n").is_empty());
        assert!(directives("y = 2  # noqa: PERF401, B008\n").is_empty());
        assert_eq!(
            codes("y = 2  # noqa: E501, LAV003, B008\n"),
            vec![vec!["LAV003".to_owned()]]
        );
    }

    /// A typo aimed at landav is claimed so that it can be reported. Silently
    /// dropping it leaves the author believing they are covered.
    #[test]
    fn a_landav_shaped_typo_is_claimed() {
        assert_eq!(
            codes("y = 2  # noqa: LAV03\n"),
            vec![vec!["LAV03".to_owned()]]
        );
        assert_eq!(
            codes("y = 2  # noqa: lav003\n"),
            vec![vec!["lav003".to_owned()]]
        );
        assert_eq!(
            codes("y = 2  # noqa: LAV9999\n"),
            vec![vec!["LAV9999".to_owned()]]
        );
    }

    #[test]
    fn the_keyword_is_case_insensitive_and_whole_word() {
        assert_eq!(
            codes("y = 2  # NoQA: LAV003\n"),
            vec![vec!["LAV003".to_owned()]]
        );
        assert!(directives("y = 2  # nonoqa: LAV003\n").is_empty());
        assert!(directives("y = 2  # noqad: LAV003\n").is_empty());
    }

    /// The directive need not open the comment: `# type: ignore  # noqa: ...`
    /// is a real shape in typed codebases.
    #[test]
    fn a_directive_later_in_the_comment_is_still_read() {
        assert_eq!(
            codes("y = 2  # type: ignore  # noqa: LAV003\n"),
            vec![vec!["LAV003".to_owned()]]
        );
    }

    /// The load-bearing one. A `#` inside a literal opens no comment, so a
    /// string whose *contents* are a suppression waives nothing.
    #[test]
    fn a_directive_inside_a_string_literal_is_not_a_directive() {
        assert!(directives("SEP = \"# noqa: LAV003\"\n").is_empty());
        assert!(directives("SEP = '# noqa: LAV003'\n").is_empty());
        assert!(directives("SEP = \"\"\"# noqa: LAV003\"\"\"\n").is_empty());
    }

    /// A triple-quoted literal spans lines, and the line counter has to follow
    /// it or every directive after a docstring is attributed to the wrong line.
    #[test]
    fn lines_are_counted_through_a_multi_line_string() {
        let source = "\"\"\"One\nTwo\nThree\n\"\"\"\nx = 1  # noqa: LAV003\n";
        assert_eq!(directives(source).first().map(|d| d.line), Some(5));
    }

    #[test]
    fn several_directives_come_back_in_line_order() {
        let source = "a = 1  # noqa: LAV001\nb = 2\nc = 3  # noqa: LAV003\n";
        let lines: Vec<u32> = directives(source).iter().map(|d| d.line).collect();
        assert_eq!(lines, vec![1, 3]);
    }

    /// A file with no trailing newline still yields its last directive.
    #[test]
    fn a_directive_on_the_final_unterminated_line_is_read() {
        assert_eq!(
            directives("x = 1  # noqa: LAV003").first().map(|d| d.line),
            Some(1)
        );
    }

    /// This crate reads untrusted Python. None of these may panic, hang, or
    /// mis-attribute a line.
    #[test]
    fn hostile_input_yields_no_directive_rather_than_a_panic() {
        for source in [
            "x = \"unterminated # noqa: LAV003\n",
            "x = '''never closed # noqa: LAV003\n",
            "#",
            "#\n#\n#",
            "# noqa:",
            "# noqa::",
            "# noqa: ,,,",
            "x = \"é\"  # noqa: LAV003",
            "\u{feff}# noqa: LAV003",
        ] {
            let _ = directives(source);
        }
        // The one case that must still work: a non-ASCII line before the
        // directive must not shift the column-free line count.
        assert_eq!(
            directives("s = \"éé\"\nx = 1  # noqa: LAV003\n")
                .first()
                .map(|d| d.line),
            Some(2)
        );
    }

    /// A reason may be empty, and an empty one is `None` rather than `""`.
    #[test]
    fn an_absent_reason_is_none() {
        assert_eq!(
            one("x = 1  # noqa: LAV003 -\n").and_then(|d| d.reason),
            None
        );
        assert_eq!(
            one("x = 1  # noqa: LAV003  \n").and_then(|d| d.reason),
            None
        );
    }

    /// A code written twice is written twice: the record layer decides what to
    /// do about it, and this layer does not silently rewrite what was typed.
    #[test]
    fn a_repeated_code_is_reported_as_written() {
        assert_eq!(
            codes("x = 1  # noqa: LAV003, LAV003\n"),
            vec![vec!["LAV003".to_owned(), "LAV003".to_owned()]]
        );
    }
}
