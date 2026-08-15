//! [`Suppression`] — the record of one waiver, and what it did.
//!
//! # A waiver is a record, not a filter
//!
//! The cheap way to build suppression is to drop the finding on the floor. It
//! is also the way suppression rots: a `# noqa` written in 2026 to silence one
//! rule on one line is still there in 2029, nobody remembers who wrote it, the
//! code it was covering was rewritten twice, and nothing anywhere says the
//! waiver exists. `LAN-66` criterion 3 exists to stop that, and it can only be
//! satisfied by a type that *survives* the filtering.
//!
//! So every waiver — the ones that fired, the ones that did nothing, and the
//! ones that name a code this build has never heard of — produces a
//! [`Suppression`]. The findings are filtered; the waivers are published.
//!
//! # The seam `E-001` plugs into
//!
//! `docs/EDITIONS.md` puts org policy governance (`E-001`) on the paid side:
//! time-boxed waivers, named approvers, an audit trail. That layer consumes
//! this record. The fields are therefore chosen so that governance can be
//! *added around* the record rather than requiring it to be reshaped:
//!
//! | Governance question | Field |
//! |---|---|
//! | what was waived | [`Suppression::code`] |
//! | over what extent | [`Suppression::origin`] — one line, or a path glob |
//! | who to ask | the file and line the waiver was written on, which `git blame` resolves to a person |
//! | on what grounds | [`Suppression::reason`] |
//! | is it still earning its place | [`Suppression::status`] and [`Suppression::suppressed`] |
//!
//! Expiry dates and approver identities are the two things `E-001` adds. Both
//! are *additional* attributes of a waiver that already has an extent, a
//! justification and an effect; neither changes the meaning of anything here.
//! That is what "consume it later without a migration" means in practice.
//!
//! **No entitlement logic lives here or anywhere else in this repository.**
//! Core records waivers. It does not know, and must never learn, whether
//! something decided the user was allowed one.

use std::path::{Path, PathBuf};

use crate::{
    registry::{is_retired_code, rule_for_code},
    rule_code::RuleCode,
};

/// Where a waiver was written down, and therefore how much it covers.
///
/// The two forms are not interchangeable and governance treats them
/// differently: an inline waiver is narrow, sits against the code it excuses,
/// and `git blame` names its author on the spot; a per-path waiver is broad,
/// lives in configuration, and covers files that do not exist yet.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SuppressionOrigin {
    /// A `# noqa: LAV003` comment, covering the physical line it sits on.
    Inline {
        /// The file the comment is in.
        file: PathBuf,
        /// The 1-based line the comment is on, which is also the only line it
        /// covers.
        line: u32,
    },
    /// A configuration entry, covering every analysed file the glob matches.
    Path {
        /// The glob, exactly as it was written in configuration.
        pattern: String,
    },
}

impl SuppressionOrigin {
    /// The file an inline waiver was written in, or `None` for a per-path one.
    #[must_use]
    pub fn file(&self) -> Option<&Path> {
        match self {
            Self::Inline { file, .. } => Some(file.as_path()),
            Self::Path { .. } => None,
        }
    }

    /// The 1-based line an inline waiver was written on.
    ///
    /// `None` for a per-path waiver: it covers files, not lines, and inventing
    /// a line number for it would make the record lie about where to look.
    #[must_use]
    pub const fn line(&self) -> Option<u32> {
        match self {
            Self::Inline { line, .. } => Some(*line),
            Self::Path { .. } => None,
        }
    }

    /// The glob a per-path waiver was declared with, or `None` for an inline
    /// one.
    #[must_use]
    pub fn pattern(&self) -> Option<&str> {
        match self {
            Self::Inline { .. } => None,
            Self::Path { pattern } => Some(pattern.as_str()),
        }
    }
}

/// What a waiver did, or the reason it could never do anything.
///
/// Four states, each of which a reader would act on differently. Written as a
/// closed enum rather than a `bool` because "it suppressed nothing" has three
/// causes that need three different fixes, and collapsing them would leave the
/// most dangerous one — a code that does not exist, so the author believes
/// they are covered and are not — indistinguishable from the harmless one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SuppressionStatus {
    /// It named a live rule and removed at least one finding.
    Applied,
    /// It named a live rule that did not fire in the code it covers.
    ///
    /// Usually the good outcome: somebody fixed the code, or the rule got
    /// narrower and stopped needing the waiver. Occasionally the waiver was
    /// always wrong. Either way it is dead weight, and it is reported.
    Unused,
    /// It named a code that was issued and then withdrawn, such as `LAV010`.
    ///
    /// Distinct from [`SuppressionStatus::Unknown`] on purpose. "You typed it
    /// wrong" and "the rule you were waiving no longer exists" call for
    /// different edits, and only a retired-code list can tell them apart. This
    /// is the payoff for burning a retired number instead of recycling it: the
    /// tool can still explain a comment written years before it was read.
    Retired,
    /// It named a code no landav rule has ever carried — a typo, or a code
    /// from a build that never shipped.
    ///
    /// The dangerous one. Silently ignoring it leaves the author believing a
    /// finding is waived when it is not.
    Unknown,
}

impl SuppressionStatus {
    /// Whether the waiver is dead weight: anything but
    /// [`SuppressionStatus::Applied`].
    #[must_use]
    pub const fn is_stale(self) -> bool {
        !matches!(self, Self::Applied)
    }

    /// A one-word rendering, for a report line.
    ///
    /// Total and wildcard-free, so a fifth status cannot be introduced without
    /// somebody deciding what it is called in the output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Unused => "unused",
            Self::Retired => "retired",
            Self::Unknown => "unknown",
        }
    }
}

/// One waiver, and what it did.
///
/// Construct through [`Suppression::inline`] or [`Suppression::per_path`];
/// both derive [`Suppression::status`] rather than accepting one, so a record
/// claiming to have suppressed nothing while reporting
/// [`SuppressionStatus::Applied`] is not a reachable state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppression {
    /// The rule code, verbatim as the author wrote it.
    code: String,
    /// Where the waiver was written, and what it covers.
    origin: SuppressionOrigin,
    /// The justification, if one was given.
    reason: Option<String>,
    /// What it did.
    status: SuppressionStatus,
    /// How many findings it removed.
    suppressed: usize,
}

impl Suppression {
    /// Records a `# noqa: CODE` comment at `file`:`line`.
    ///
    /// `code` is stored exactly as written — a typo is reported as the author
    /// spelled it, because "LAV03 is not a rule code" is actionable and
    /// "LAV003 was not applied" is baffling.
    #[must_use]
    pub fn inline(
        code: String,
        file: PathBuf,
        line: u32,
        reason: Option<String>,
        suppressed: usize,
    ) -> Self {
        let status = classify(&code, suppressed);
        Self {
            code,
            origin: SuppressionOrigin::Inline { file, line },
            reason,
            status,
            suppressed,
        }
    }

    /// Records a configured waiver of `code` over every file matching
    /// `pattern`.
    #[must_use]
    pub fn per_path(
        code: String,
        pattern: String,
        reason: Option<String>,
        suppressed: usize,
    ) -> Self {
        let status = classify(&code, suppressed);
        Self {
            code,
            origin: SuppressionOrigin::Path { pattern },
            reason,
            status,
            suppressed,
        }
    }

    /// The rule code as the author wrote it, valid or not.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The registered rule this waiver names, or `None` when it names none.
    ///
    /// `None` covers both [`SuppressionStatus::Retired`] and
    /// [`SuppressionStatus::Unknown`]; [`Suppression::status`] is what
    /// separates them.
    #[must_use]
    pub fn rule(&self) -> Option<RuleCode> {
        rule_for_code(&self.code).map(crate::rule::Rule::code)
    }

    /// Where the waiver was written, and what it covers.
    #[must_use]
    pub const fn origin(&self) -> &SuppressionOrigin {
        &self.origin
    }

    /// The justification, or `None` when the author gave none.
    ///
    /// Optional for an inline waiver and required for a per-path one — see
    /// the crate documentation for why the broader form has to justify itself
    /// and the narrow one does not.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// What the waiver did.
    #[must_use]
    pub const fn status(&self) -> SuppressionStatus {
        self.status
    }

    /// Whether it is dead weight.
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        self.status.is_stale()
    }

    /// How many findings it removed. Zero for anything but
    /// [`SuppressionStatus::Applied`].
    #[must_use]
    pub const fn suppressed(&self) -> usize {
        self.suppressed
    }

    /// The same waiver, credited with `extra` further findings.
    ///
    /// A per-path waiver is decided once and applied to many files, so a run
    /// over a tree produces one record per file and the driver folds them into
    /// the single waiver the operator actually wrote. Folding re-derives the
    /// status, so a waiver that did nothing in the first file and something in
    /// the second is [`SuppressionStatus::Applied`] and not stale.
    #[must_use]
    pub fn crediting(mut self, extra: usize) -> Self {
        self.suppressed = self.suppressed.saturating_add(extra);
        self.status = classify(&self.code, self.suppressed);
        self
    }
}

/// Derives a status from a code and what it managed to suppress.
///
/// Order matters. A retired code is not in the registry, so the retirement
/// check has to come first or `LAV010` reports as a typo — losing exactly the
/// distinction that burning the number bought.
fn classify(code: &str, suppressed: usize) -> SuppressionStatus {
    if is_retired_code(code) {
        return SuppressionStatus::Retired;
    }
    if rule_for_code(code).is_none() {
        return SuppressionStatus::Unknown;
    }
    if suppressed == 0 {
        return SuppressionStatus::Unused;
    }
    SuppressionStatus::Applied
}

/// A per-path waiver as declared in configuration, before it has met any code.
///
/// Deliberately separate from [`Suppression`]: this is the *declaration*, and
/// a [`Suppression`] is the *record of what the declaration did*. Governance
/// wants both — the first is what a reviewer approves, the second is what an
/// audit reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathWaiver {
    /// The glob, exactly as written.
    pattern: String,
    /// The rule codes waived, exactly as written.
    rules: Vec<String>,
    /// Why. Required — see [`PathWaiver::reason`].
    reason: String,
}

impl PathWaiver {
    /// Declares a waiver of `rules` over every file matching `pattern`.
    #[must_use]
    pub fn new(pattern: String, rules: Vec<String>, reason: String) -> Self {
        Self {
            pattern,
            rules,
            reason,
        }
    }

    /// The glob, exactly as written.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// The rule codes waived, exactly as written, valid or not.
    #[must_use]
    pub fn rules(&self) -> &[String] {
        &self.rules
    }

    /// Why the waiver exists.
    ///
    /// Required, unlike an inline reason. A per-path waiver covers files that
    /// do not exist yet, is read by people who were not in the room, and is
    /// approved once and never again; a sentence costs nothing and is the only
    /// thing a later auditor — or `E-001` — has to work with. An inline waiver
    /// sits against the line it excuses and can point at itself.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Whether this waiver covers `file`.
    #[must_use]
    pub fn covers(&self, file: &Path) -> bool {
        path_matches(&self.pattern, file)
    }

    /// Whether this waiver names `code`, compared verbatim.
    #[must_use]
    pub fn waives(&self, code: &str) -> bool {
        self.rules.iter().any(|rule| rule == code)
    }
}

/// Whether `pattern` matches `file`.
///
/// # The glob dialect, in full
///
/// * `?` matches exactly one character other than `/`;
/// * `*` matches any run of characters other than `/`, including none;
/// * `**` as a whole path segment matches any number of segments, including
///   none;
/// * every other character matches itself. There are no character classes: a
///   `[` is a literal `[`, because a half-supported `[a-z]` that silently
///   matched nothing would be worse than one that never existed.
///
/// A pattern with no `/` in it is matched against the **file name** alone, so
/// `conftest.py` and `*_test.py` do what they look like they do. A pattern
/// with a `/` is matched against the whole path with an implicit `**/` in
/// front, so `tests/**` covers `/home/me/project/tests/fixture.py` without the
/// author having to know where the checkout lives.
///
/// Path separators are normalised to `/` first, so a Windows-shaped path and a
/// POSIX-shaped pattern agree.
#[must_use]
pub fn path_matches(pattern: &str, file: &Path) -> bool {
    let subject = if pattern.contains('/') {
        normalise(file)
    } else {
        file.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };

    let anchored = pattern.strip_prefix('/');
    let implicit_prefix = anchored.is_none() && pattern.contains('/');
    let pattern = anchored.unwrap_or(pattern);

    let mut pattern_segments: Vec<&str> = pattern.split('/').collect();
    if implicit_prefix {
        pattern_segments.insert(0, "**");
    }
    let subject_segments: Vec<&str> = subject.split('/').filter(|s| !s.is_empty()).collect();

    match_segments(&pattern_segments, &subject_segments)
}

/// A path as a `/`-separated string, whatever the platform spells it with.
fn normalise(file: &Path) -> String {
    file.to_string_lossy().replace('\\', "/")
}

/// Matches segment lists, with `**` free to consume any number of segments.
///
/// Iterative with an explicit backtrack point rather than recursive: this
/// crate reads untrusted input, and `**/**/**/…` repeated ten thousand times
/// must cost heap rather than stack.
fn match_segments(pattern: &[&str], subject: &[&str]) -> bool {
    let mut p = 0_usize;
    let mut s = 0_usize;
    // Where to resume if the current `**` turns out to have eaten too little.
    let mut star_pattern: Option<usize> = None;
    let mut star_subject = 0_usize;

    while s < subject.len() {
        match pattern.get(p) {
            Some(&"**") => {
                star_pattern = Some(p);
                star_subject = s;
                p += 1;
            }
            Some(segment) if matches_segment(segment, subject[s]) => {
                p += 1;
                s += 1;
            }
            // Rewind *onto* the `**` rather than past it, and let the arm
            // above re-enter it. One extra iteration, one fewer index
            // calculation, and the resumption point cannot drift out of step
            // with where the `**` actually is.
            _ => match star_pattern {
                Some(star) => {
                    star_subject += 1;
                    s = star_subject;
                    p = star;
                }
                None => return false,
            },
        }
    }

    pattern[p.min(pattern.len())..]
        .iter()
        .all(|segment| *segment == "**")
}

/// Matches one path segment against one pattern segment, `*` and `?` only.
///
/// The classic two-pointer wildcard match: linear in the common case, with a
/// single backtrack point for `*`, and no recursion.
fn matches_segment(pattern: &str, subject: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let subject: Vec<char> = subject.chars().collect();

    let mut p = 0_usize;
    let mut s = 0_usize;
    let mut star: Option<usize> = None;
    let mut star_subject = 0_usize;

    while s < subject.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some(p);
                star_subject = s;
                p += 1;
            }
            Some('?') => {
                p += 1;
                s += 1;
            }
            Some(character) if *character == subject[s] => {
                p += 1;
                s += 1;
            }
            // As in [`match_segments`]: rewind onto the `*` and let its arm
            // re-enter it.
            _ => match star {
                Some(index) => {
                    star_subject += 1;
                    s = star_subject;
                    p = index;
                }
                None => return false,
            },
        }
    }

    pattern[p.min(pattern.len())..]
        .iter()
        .all(|character| *character == '*')
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{PathWaiver, Suppression, SuppressionStatus, path_matches};

    fn inline(code: &str, suppressed: usize) -> Suppression {
        Suppression::inline(
            code.to_owned(),
            PathBuf::from("src/report.py"),
            12,
            None,
            suppressed,
        )
    }

    /// A waiver that removed a finding is applied, and says where it was
    /// written so that `git blame` can name the person who wrote it.
    #[test]
    fn an_inline_waiver_that_fired_names_its_own_position() {
        let record = inline("LAV003", 1);
        assert_eq!(record.status(), SuppressionStatus::Applied);
        assert!(!record.is_stale());
        assert_eq!(record.suppressed(), 1);
        assert_eq!(record.origin().line(), Some(12));
        assert_eq!(record.origin().file(), Some(Path::new("src/report.py")));
        assert_eq!(record.origin().pattern(), None);
        assert_eq!(record.rule().map(|rule| rule.as_str()), Some("LAV003"));
    }

    /// The stale case criterion 3 exists for. It is reported, and it is not an
    /// error.
    #[test]
    fn a_live_code_that_suppressed_nothing_is_unused() {
        let record = inline("LAV003", 0);
        assert_eq!(record.status(), SuppressionStatus::Unused);
        assert!(record.is_stale());
    }

    /// `LAV010` is burned, not free. The whole reason for burning it is that
    /// this record can still explain the comment years later, so "retired"
    /// must not collapse into "unknown".
    #[test]
    fn a_retired_code_is_reported_as_retired_and_never_as_unknown() {
        let record = inline("LAV010", 0);
        assert_eq!(record.status(), SuppressionStatus::Retired);
        assert_ne!(record.status(), SuppressionStatus::Unknown);
        assert!(record.rule().is_none());
    }

    /// A typo must survive into the report exactly as written, or the reader
    /// cannot see what is wrong with it.
    #[test]
    fn an_unknown_code_is_reported_verbatim() {
        for typo in ["LAV999", "LAV03", "lav003", "LAV0033"] {
            let record = inline(typo, 0);
            assert_eq!(record.status(), SuppressionStatus::Unknown, "{typo}");
            assert_eq!(record.code(), typo);
        }
    }

    /// A waiver can never claim to have applied while having suppressed
    /// nothing, whatever a caller passes.
    #[test]
    fn applied_implies_a_nonzero_count() {
        for code in ["LAV001", "LAV010", "LAV999"] {
            for count in [0, 1, 7] {
                let record = inline(code, count);
                if record.status() == SuppressionStatus::Applied {
                    assert!(record.suppressed() > 0, "{code} {count}");
                }
            }
        }
    }

    /// One configured waiver, many files. Folding the per-file records has to
    /// re-derive the status or a waiver that fired in the second file would
    /// still read as unused.
    #[test]
    fn folding_per_file_records_re_derives_the_status() {
        let first = Suppression::per_path(
            "LAV003".to_owned(),
            "tests/**".to_owned(),
            Some("fixtures build strings the slow way on purpose".to_owned()),
            0,
        );
        assert_eq!(first.status(), SuppressionStatus::Unused);

        let folded = first.crediting(2);
        assert_eq!(folded.status(), SuppressionStatus::Applied);
        assert_eq!(folded.suppressed(), 2);
        assert_eq!(folded.origin().pattern(), Some("tests/**"));
        assert_eq!(folded.origin().line(), None);
        assert_eq!(folded.origin().file(), None);
    }

    /// Every status has a distinct word, or a report cannot tell them apart.
    #[test]
    fn statuses_render_distinctly() {
        let words: Vec<&str> = [
            SuppressionStatus::Applied,
            SuppressionStatus::Unused,
            SuppressionStatus::Retired,
            SuppressionStatus::Unknown,
        ]
        .iter()
        .map(|status| status.as_str())
        .collect();
        let mut sorted = words.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), words.len(), "{words:?}");
        assert!(words.iter().all(|word| !word.is_empty()));
    }

    /// Only `Applied` is not stale.
    #[test]
    fn exactly_one_status_is_not_stale() {
        assert!(!SuppressionStatus::Applied.is_stale());
        for status in [
            SuppressionStatus::Unused,
            SuppressionStatus::Retired,
            SuppressionStatus::Unknown,
        ] {
            assert!(status.is_stale(), "{status:?}");
        }
    }

    #[test]
    fn a_bare_name_pattern_matches_the_file_name_at_any_depth() {
        assert!(path_matches("conftest.py", Path::new("a/b/conftest.py")));
        assert!(path_matches("*_test.py", Path::new("/x/y/report_test.py")));
        assert!(!path_matches("conftest.py", Path::new("a/b/report.py")));
    }

    /// A pattern with a slash gets an implicit `**/`, so it does not have to
    /// know where the checkout is on disk.
    #[test]
    fn a_pattern_with_a_slash_matches_anywhere_in_the_tree() {
        assert!(path_matches(
            "tests/**",
            Path::new("/home/me/project/tests/fixture.py")
        ));
        assert!(path_matches("src/*.py", Path::new("project/src/main.py")));
        assert!(!path_matches(
            "src/*.py",
            Path::new("project/src/pkg/main.py")
        ));
        assert!(!path_matches("tests/**", Path::new("/home/me/src/main.py")));
    }

    /// `*` stops at a separator and `**` does not. Conflating them is the
    /// classic glob bug and it silently widens every waiver written with `*`.
    #[test]
    fn a_single_star_does_not_cross_a_separator() {
        assert!(!path_matches("a/*/d.py", Path::new("a/b/c/d.py")));
        assert!(path_matches("a/**/d.py", Path::new("a/b/c/d.py")));
        assert!(path_matches("a/**/d.py", Path::new("a/d.py")));
    }

    /// What is left of the pattern after the path runs out decides the answer,
    /// and only a trailing `*` may be left. A matcher that accepted *any*
    /// leftover would match `src/report.py` against a file called `report`,
    /// silently widening every waiver written with a filename.
    #[test]
    fn a_pattern_longer_than_the_path_matches_only_through_a_trailing_star() {
        assert!(path_matches("src/report*", Path::new("x/src/report")));
        assert!(path_matches("src/report**", Path::new("x/src/report")));
        assert!(!path_matches("src/report.py", Path::new("x/src/report")));
        assert!(!path_matches("src/report?", Path::new("x/src/report")));
        assert!(path_matches("a/**/b/**", Path::new("a/b")));
        assert!(!path_matches("a/**/b/c", Path::new("a/b")));
    }

    #[test]
    fn a_question_mark_matches_exactly_one_character() {
        assert!(path_matches("test_?.py", Path::new("x/test_1.py")));
        assert!(!path_matches("test_?.py", Path::new("x/test_12.py")));
        assert!(!path_matches("test_?.py", Path::new("x/test_.py")));
    }

    /// An anchored pattern gets no implicit prefix.
    #[test]
    fn a_leading_slash_anchors_the_pattern() {
        assert!(path_matches("/srv/app/**", Path::new("/srv/app/main.py")));
        assert!(!path_matches("/app/**", Path::new("/srv/app/main.py")));
    }

    /// There are no character classes, and a `[` must not be silently eaten.
    #[test]
    fn brackets_are_literal() {
        assert!(path_matches("gen/[a].py", Path::new("x/gen/[a].py")));
        assert!(!path_matches("gen/[ab].py", Path::new("x/gen/a.py")));
    }

    /// A pathological pattern must cost heap, not stack: this crate reads
    /// untrusted input and an abort loses the blame path.
    #[test]
    fn a_pathological_pattern_does_not_overflow_the_stack() {
        let pattern = format!("{}z.py", "**/".repeat(20_000));
        let subject = PathBuf::from(format!("{}a.py", "d/".repeat(20_000)));
        assert!(!path_matches(&pattern, &subject));
    }

    #[test]
    fn a_waiver_matches_its_own_pattern_and_codes() {
        let waiver = PathWaiver::new(
            "tests/**".to_owned(),
            vec!["LAV003".to_owned()],
            "fixtures are deliberately slow".to_owned(),
        );
        assert!(waiver.covers(Path::new("project/tests/a.py")));
        assert!(!waiver.covers(Path::new("project/src/a.py")));
        assert!(waiver.waives("LAV003"));
        assert!(!waiver.waives("LAV002"));
        assert_eq!(waiver.reason(), "fixtures are deliberately slow");
        assert_eq!(waiver.rules(), ["LAV003"]);
        assert_eq!(waiver.pattern(), "tests/**");
    }
}
