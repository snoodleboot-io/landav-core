//! `landav report` and `landav diff`: the coverage picture of a run, and of a
//! change between two runs.
//!
//! # Why this is a subcommand and not a script
//!
//! `LAN-119`, the first slice of `F-040`'s **coverage** metric. Every change
//! from `LAN-89` to `LAN-118` was measured by an ad-hoc script over two
//! `landav check --json` files, computing the same things each time: functions
//! by bound kind, holes by construct, functions blocked by a single construct,
//! functions that gained or lost holes, functions that went from exact to
//! anything else, and an explanation for every gained hole. The script was
//! rewritten for every ticket, its baseline was lost when a scratch directory
//! was recycled, and one headline number was wrong because a regular
//! expression counted the letters of `max(` as a variable. A number that has
//! to be re-derived is a number nobody can check; this is the derivation,
//! once, where the schema it reads lives.
//!
//! # What it reads
//!
//! The machine shape of `crate::machine`, by its stable fields only: a
//! function's position, its `bound_kind`, and the `construct` of each hole.
//! Everything else is ignored, which is the contract that module states for
//! consumers - a field may be added without a schema bump. The schema version
//! is checked, not ignored: a run from a different version is refused by
//! name, because a construct spelled differently across versions would make
//! every table below quietly wrong.
//!
//! # What it is not
//!
//! A gate. It exits clean whatever it finds, including a regression; the
//! regression line is printed first so that a reader cannot miss it, and a
//! consumer that wants to fail on it reads the JSON. Soundness and tightness
//! are the other two metrics of `F-040` and need an oracle this has no access
//! to.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{diagnostic::ToolError, machine::SCHEMA_VERSION, outcome::Outcome};

/// The slice of a run this module reads.
#[derive(Debug, Deserialize)]
struct RunView {
    schema_version: u32,
    #[serde(default)]
    functions: Vec<FunctionView>,
}

/// The slice of a function this module reads.
#[derive(Debug, Deserialize)]
struct FunctionView {
    name: String,
    file: String,
    line: u32,
    #[serde(default)]
    bound_kind: Option<String>,
    #[serde(default)]
    holes: Vec<HoleView>,
}

/// The slice of a hole this module reads.
#[derive(Debug, Deserialize)]
struct HoleView {
    construct: String,
}

/// The bound-kind tag for a function with no bound at all.
const NO_BOUND: &str = "none";

/// The tallies of one run.
#[derive(Debug, Default, Serialize)]
pub struct Picture {
    /// Functions in the run.
    pub functions: usize,
    /// Functions by `bound_kind`: `exact`, `partial`, `upper`, or `none`.
    pub by_kind: BTreeMap<String, usize>,
    /// Holes in the run.
    pub holes: usize,
    /// Holes by construct.
    pub holes_by_construct: BTreeMap<String, usize>,
    /// Partial functions whose holes are all of one construct, by construct.
    ///
    /// The table the backlog is prioritised from: a function here is one
    /// change away from a complete bound, and the construct says which.
    pub sole_blockers: BTreeMap<String, usize>,
}

impl Picture {
    fn of(functions: &[FunctionView]) -> Self {
        let mut picture = Self {
            functions: functions.len(),
            ..Self::default()
        };
        for function in functions {
            let kind = function.bound_kind.as_deref().unwrap_or(NO_BOUND);
            *picture.by_kind.entry(kind.to_owned()).or_default() += 1;
            picture.holes += function.holes.len();
            for hole in &function.holes {
                *picture
                    .holes_by_construct
                    .entry(hole.construct.clone())
                    .or_default() += 1;
            }
            if kind == "partial"
                && let Some(construct) = sole_blocker(function)
            {
                *picture
                    .sole_blockers
                    .entry(construct.to_owned())
                    .or_default() += 1;
            }
        }
        picture
    }

    fn kind(&self, kind: &str) -> usize {
        self.by_kind.get(kind).copied().unwrap_or_default()
    }
}

/// The one construct every hole of `function` is, if there is one.
fn sole_blocker(function: &FunctionView) -> Option<&str> {
    let constructs: BTreeSet<&str> = function
        .holes
        .iter()
        .map(|hole| hole.construct.as_str())
        .collect();
    match constructs.len() {
        1 => constructs.into_iter().next(),
        _ => None,
    }
}

/// A function whose result changed between the two runs.
#[derive(Debug, Serialize)]
pub struct Changed {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub before_kind: String,
    pub after_kind: String,
    pub before_holes: Vec<String>,
    pub after_holes: Vec<String>,
}

/// Why a function has more holes than it had.
///
/// The three answers the measurements since `LAN-100` have needed, in the
/// order they are tried. A region that stood over an interior and is gone is
/// a *de-concealment*: the interior was always there and is now named. A
/// newly named `call`, `attribute` or `subscript` with nothing lost is the
/// same thing seen from the other side - a value that used to be refused
/// whole now has its parts walked. Anything else is listed in full, because
/// it is the one a reviewer has to look at.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "construct", rename_all = "kebab-case")]
pub enum Explanation {
    /// A region of this construct is gone and its interior shows.
    LostRegion(String),
    /// A region of this construct that nothing named before.
    NamedInterior(String),
    /// Neither.
    Unexplained,
}

/// A function that gained holes, and why.
#[derive(Debug, Serialize)]
pub struct Gainer {
    #[serde(flatten)]
    pub change: Changed,
    pub explanation: Explanation,
}

/// The picture of a change.
#[derive(Debug, Serialize)]
pub struct Delta {
    pub before: Picture,
    pub after: Picture,
    /// Functions present in both runs, by file and line.
    pub matched: usize,
    pub only_before: usize,
    pub only_after: usize,
    /// Functions that were exact and are not. **The regression line.**
    pub regressions: Vec<Changed>,
    /// Functions that were partial and are exact.
    pub improved: usize,
    pub fewer_holes: usize,
    /// Functions with more holes than before, each with its explanation.
    pub gainers: Vec<Gainer>,
}

/// The constructs a newly walked interior shows up as.
const INTERIOR: [&str; 3] = ["call", "attribute", "subscript"];

fn counts(holes: &[HoleView]) -> BTreeMap<&str, usize> {
    let mut counts = BTreeMap::new();
    for hole in holes {
        *counts.entry(hole.construct.as_str()).or_default() += 1;
    }
    counts
}

fn explain(before: &FunctionView, after: &FunctionView) -> Explanation {
    let (was, now) = (counts(&before.holes), counts(&after.holes));
    if let Some((construct, _)) = was
        .iter()
        .find(|(construct, count)| now.get(*construct).copied().unwrap_or_default() < **count)
    {
        return Explanation::LostRegion((*construct).to_owned());
    }
    if let Some(construct) = INTERIOR.into_iter().find(|construct| {
        now.get(construct).copied().unwrap_or_default()
            > was.get(construct).copied().unwrap_or_default()
    }) {
        return Explanation::NamedInterior(construct.to_owned());
    }
    Explanation::Unexplained
}

fn changed(before: &FunctionView, after: &FunctionView) -> Changed {
    let holes = |f: &FunctionView| f.holes.iter().map(|hole| hole.construct.clone()).collect();
    Changed {
        name: after.name.clone(),
        file: after.file.clone(),
        line: after.line,
        before_kind: before
            .bound_kind
            .clone()
            .unwrap_or_else(|| NO_BOUND.to_owned()),
        after_kind: after
            .bound_kind
            .clone()
            .unwrap_or_else(|| NO_BOUND.to_owned()),
        before_holes: holes(before),
        after_holes: holes(after),
    }
}

impl Delta {
    fn of(before: &[FunctionView], after: &[FunctionView]) -> Self {
        let index: BTreeMap<(&str, u32), &FunctionView> = before
            .iter()
            .map(|function| ((function.file.as_str(), function.line), function))
            .collect();
        let mut delta = Self {
            before: Picture::of(before),
            after: Picture::of(after),
            matched: 0,
            only_before: 0,
            only_after: 0,
            regressions: Vec::new(),
            improved: 0,
            fewer_holes: 0,
            gainers: Vec::new(),
        };
        let mut seen: BTreeSet<(&str, u32)> = BTreeSet::new();
        for now in after {
            let key = (now.file.as_str(), now.line);
            let Some(was) = index.get(&key) else {
                delta.only_after += 1;
                continue;
            };
            seen.insert(key);
            delta.matched += 1;
            let (was_kind, now_kind) = (
                was.bound_kind.as_deref().unwrap_or(NO_BOUND),
                now.bound_kind.as_deref().unwrap_or(NO_BOUND),
            );
            if was_kind == "exact" && now_kind != "exact" {
                delta.regressions.push(changed(was, now));
            }
            if was_kind == "partial" && now_kind == "exact" {
                delta.improved += 1;
            }
            if now.holes.len() < was.holes.len() {
                delta.fewer_holes += 1;
            }
            if now.holes.len() > was.holes.len() {
                delta.gainers.push(Gainer {
                    change: changed(was, now),
                    explanation: explain(was, now),
                });
            }
        }
        delta.only_before = index.len().saturating_sub(seen.len());
        delta
    }
}

/// Reads a run, refusing one from another schema version by name.
fn read(path: &Path) -> Result<RunView, ToolError> {
    let text = fs::read_to_string(path)
        .map_err(|why| ToolError::at_path(path, format!("could not read the run: {why}")))?;
    let run: RunView = serde_json::from_str(&text).map_err(|why| {
        ToolError::at_path(path, format!("not a `landav check --json` run: {why}"))
    })?;
    if run.schema_version != SCHEMA_VERSION {
        return Err(ToolError::at_path(
            path,
            format!(
                "schema version {} where this binary reads {SCHEMA_VERSION}; the tables would \
                 compare constructs that may be spelled differently",
                run.schema_version
            ),
        ));
    }
    Ok(run)
}

/// One line about a run on disk, for `landav bench run` to say what it wrote.
pub fn summary_of(path: &Path) -> Result<String, ToolError> {
    let run = read(path)?;
    let picture = Picture::of(&run.functions);
    Ok(format!(
        "{} functions, {} exact, {} holes",
        thousands(picture.functions),
        thousands(picture.kind("exact")),
        thousands(picture.holes)
    ))
}

/// `landav report RUN`.
pub fn report(run: &Path, json: bool) -> Outcome {
    let run = match read(run) {
        Ok(run) => run,
        Err(error) => return failed(&error),
    };
    let picture = Picture::of(&run.functions);
    if json {
        return emit(&picture);
    }
    let mut out = String::new();
    out.push_str(&format!("{} functions\n\n", thousands(picture.functions)));
    out.push_str(&render_picture(&picture));
    print!("{out}");
    Outcome::Clean
}

/// `landav diff BEFORE AFTER`.
pub fn diff(before: &Path, after: &Path, json: bool) -> Outcome {
    let (before, after) = match (read(before), read(after)) {
        (Ok(before), Ok(after)) => (before, after),
        (Err(error), _) | (_, Err(error)) => return failed(&error),
    };
    let delta = Delta::of(&before.functions, &after.functions);
    if json {
        return emit(&delta);
    }
    print!("{}", render_delta(&delta));
    Outcome::Clean
}

fn failed(error: &ToolError) -> Outcome {
    eprintln!("{error}");
    Outcome::Failed
}

fn emit<T: Serialize>(value: &T) -> Outcome {
    match serde_json::to_string_pretty(value) {
        Ok(text) => {
            println!("{text}");
            Outcome::Clean
        }
        Err(why) => failed(&ToolError::new(
            "--json",
            format!("could not render the picture as JSON: {why}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// text
// ---------------------------------------------------------------------------

/// `12760` as `12,760`. The tables are read by people.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A delta as `+5`, `-5,749` or `0`.
fn signed(before: usize, after: usize) -> String {
    match after.cmp(&before) {
        std::cmp::Ordering::Greater => format!("+{}", thousands(after - before)),
        std::cmp::Ordering::Less => format!("-{}", thousands(before - after)),
        std::cmp::Ordering::Equal => "0".to_owned(),
    }
}

fn row(label: &str, before: usize, after: usize) -> String {
    format!(
        "  {label:<28} {:>9} {:>9} {:>8}\n",
        thousands(before),
        thousands(after),
        signed(before, after)
    )
}

fn single(label: &str, value: usize) -> String {
    format!("  {label:<28} {:>9}\n", thousands(value))
}

/// Every key of both maps, in descending order of the `after` count.
fn keys_by_after<'a>(
    before: &'a BTreeMap<String, usize>,
    after: &'a BTreeMap<String, usize>,
) -> Vec<&'a str> {
    let mut keys: Vec<&str> = before
        .keys()
        .chain(after.keys())
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys.sort_by_key(|key| std::cmp::Reverse(after.get(*key).copied().unwrap_or_default()));
    keys
}

fn render_picture(picture: &Picture) -> String {
    let mut out = String::new();
    for kind in ["exact", "partial", "upper", NO_BOUND] {
        out.push_str(&single(kind, picture.kind(kind)));
    }
    out.push_str(&single("holes", picture.holes));
    out.push_str("\nholes by construct\n");
    for (construct, count) in
        keys_by_after(&picture.holes_by_construct, &picture.holes_by_construct)
            .into_iter()
            .filter_map(|key| picture.holes_by_construct.get_key_value(key))
    {
        out.push_str(&single(construct, *count));
    }
    out.push_str("\nblocked by one construct\n");
    for (construct, count) in keys_by_after(&picture.sole_blockers, &picture.sole_blockers)
        .into_iter()
        .filter_map(|key| picture.sole_blockers.get_key_value(key))
    {
        out.push_str(&single(construct, *count));
    }
    out
}

/// How many gainers are listed in full before the rest are counted.
const LISTED_GAINERS: usize = 40;

/// One line per changed function. The hole lists themselves are in the JSON;
/// here a thirty-hole `__init__` reads as `holes 30 -> 33`, not as two
/// paragraphs.
fn describe(change: &Changed) -> String {
    format!(
        "{}:{} {}  {} -> {}  holes {} -> {}",
        change.file,
        change.line,
        change.name,
        change.before_kind,
        change.after_kind,
        change.before_holes.len(),
        change.after_holes.len()
    )
}

fn render_delta(delta: &Delta) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} functions matched ({} only before, {} only after)\n\n",
        thousands(delta.matched),
        thousands(delta.only_before),
        thousands(delta.only_after)
    ));
    // The regression line first: it is the one a reader must not miss.
    out.push_str(&format!(
        "exact -> not exact: {}\n",
        thousands(delta.regressions.len())
    ));
    for change in &delta.regressions {
        out.push_str(&format!("  REGRESSION {}\n", describe(change)));
    }
    out.push_str(&format!(
        "partial -> exact: {}\n",
        thousands(delta.improved)
    ));
    out.push_str(&format!(
        "functions with fewer holes: {}\n\n",
        thousands(delta.fewer_holes)
    ));
    out.push_str(&format!(
        "  {:<28} {:>9} {:>9} {:>8}\n",
        "", "before", "after", "delta"
    ));
    for kind in ["exact", "partial", "upper", NO_BOUND] {
        out.push_str(&row(kind, delta.before.kind(kind), delta.after.kind(kind)));
    }
    out.push_str(&row("holes", delta.before.holes, delta.after.holes));
    out.push_str("\nholes by construct\n");
    for key in keys_by_after(
        &delta.before.holes_by_construct,
        &delta.after.holes_by_construct,
    ) {
        out.push_str(&row(
            key,
            delta
                .before
                .holes_by_construct
                .get(key)
                .copied()
                .unwrap_or_default(),
            delta
                .after
                .holes_by_construct
                .get(key)
                .copied()
                .unwrap_or_default(),
        ));
    }
    out.push_str("\nblocked by one construct\n");
    for key in keys_by_after(&delta.before.sole_blockers, &delta.after.sole_blockers) {
        out.push_str(&row(
            key,
            delta
                .before
                .sole_blockers
                .get(key)
                .copied()
                .unwrap_or_default(),
            delta
                .after
                .sole_blockers
                .get(key)
                .copied()
                .unwrap_or_default(),
        ));
    }
    let lost = delta
        .gainers
        .iter()
        .filter(|g| matches!(g.explanation, Explanation::LostRegion(_)))
        .count();
    let named = delta
        .gainers
        .iter()
        .filter(|g| matches!(g.explanation, Explanation::NamedInterior(_)))
        .count();
    let unexplained: Vec<&Gainer> = delta
        .gainers
        .iter()
        .filter(|g| matches!(g.explanation, Explanation::Unexplained))
        .collect();
    out.push_str(&format!(
        "\nfunctions with more holes: {} (lost a region {}, named an interior {}, unexplained {})\n",
        thousands(delta.gainers.len()),
        thousands(lost),
        thousands(named),
        thousands(unexplained.len())
    ));
    // Every unexplained one, always; the explained ones up to a cap.
    for gainer in &unexplained {
        out.push_str(&format!("  UNEXPLAINED {}\n", describe(&gainer.change)));
    }
    let explained = delta
        .gainers
        .iter()
        .filter(|g| !matches!(g.explanation, Explanation::Unexplained));
    for gainer in explained.clone().take(LISTED_GAINERS) {
        let why = match &gainer.explanation {
            Explanation::LostRegion(construct) => format!("lost `{construct}`"),
            Explanation::NamedInterior(construct) => format!("named `{construct}`"),
            Explanation::Unexplained => unreachable!("filtered above"),
        };
        out.push_str(&format!("  {why:<28} {}\n", describe(&gainer.change)));
    }
    let rest = explained.count().saturating_sub(LISTED_GAINERS);
    if rest > 0 {
        out.push_str(&format!("  ... and {} more explained\n", thousands(rest)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(12760), "12,760");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn signed_reads_as_a_delta() {
        assert_eq!(signed(785, 790), "+5");
        assert_eq!(signed(82_331, 76_582), "-5,749");
        assert_eq!(signed(2, 2), "0");
    }

    fn function(name: &str, line: u32, kind: Option<&str>, holes: &[&str]) -> FunctionView {
        FunctionView {
            name: name.to_owned(),
            file: "w.py".to_owned(),
            line,
            bound_kind: kind.map(str::to_owned),
            holes: holes
                .iter()
                .map(|construct| HoleView {
                    construct: (*construct).to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_gainer_is_explained_in_the_order_the_measurements_need() {
        let lost = explain(
            &function("a", 1, Some("partial"), &["conditional-expression"]),
            &function("a", 1, Some("partial"), &["call", "attribute"]),
        );
        assert!(matches!(lost, Explanation::LostRegion(ref c) if c == "conditional-expression"));
        let named = explain(
            &function("b", 2, Some("partial"), &["call"]),
            &function("b", 2, Some("partial"), &["call", "call"]),
        );
        assert!(matches!(named, Explanation::NamedInterior(ref c) if c == "call"));
        let unexplained = explain(
            &function("c", 3, Some("partial"), &["subscript"]),
            &function("c", 3, Some("partial"), &["subscript", "non-integer-value"]),
        );
        assert!(matches!(unexplained, Explanation::Unexplained));
    }

    #[test]
    fn the_regression_line_is_exact_to_anything_else() {
        let before = [
            function("stays", 1, Some("exact"), &[]),
            function("regresses", 2, Some("exact"), &[]),
            function("improves", 3, Some("partial"), &["call"]),
            function("vanishes", 4, Some("exact"), &[]),
        ];
        let after = [
            function("stays", 1, Some("exact"), &[]),
            function("regresses", 2, Some("partial"), &["call"]),
            function("improves", 3, Some("exact"), &[]),
            function("appears", 9, None, &[]),
        ];
        let delta = Delta::of(&before, &after);
        assert_eq!(delta.matched, 3);
        assert_eq!(delta.only_before, 1);
        assert_eq!(delta.only_after, 1);
        assert_eq!(delta.regressions.len(), 1);
        assert_eq!(delta.regressions[0].name, "regresses");
        assert_eq!(delta.improved, 1);
        assert_eq!(delta.fewer_holes, 1);
        assert_eq!(delta.gainers.len(), 1);
        assert_eq!(delta.after.kind(NO_BOUND), 1);
    }

    #[test]
    fn a_sole_blocker_is_one_construct_however_many_holes() {
        assert_eq!(
            sole_blocker(&function("f", 1, Some("partial"), &["call", "call"])),
            Some("call")
        );
        assert_eq!(
            sole_blocker(&function("f", 1, Some("partial"), &["call", "attribute"])),
            None
        );
        assert_eq!(sole_blocker(&function("f", 1, Some("exact"), &[])), None);
    }
}
