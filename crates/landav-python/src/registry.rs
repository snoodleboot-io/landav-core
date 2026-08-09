//! The pattern-rule registry: every rule this frontend can emit.

use crate::{rule::Rule, rule_code::RuleCode};

/// Every rule this frontend can emit, in ascending [`RuleCode`] order.
///
/// The ordering is part of the contract: `landav explain --list` and the CI
/// baseline both iterate it, and an order that depends on declaration order is
/// an order that changes when somebody inserts a rule in the middle.
#[must_use]
pub fn registry() -> &'static [Rule] {
    RULES
}

/// Looks a rule up by its code. `None` for a code this build does not know.
#[must_use]
pub fn rule_for_code(code: &str) -> Option<&'static Rule> {
    registry().iter().find(|rule| rule.code().as_str() == code)
}

/// Looks a rule up by an already-validated [`RuleCode`].
#[must_use]
pub fn rule(code: RuleCode) -> Option<&'static Rule> {
    rule_for_code(code.as_str())
}

pub(crate) const LAV001: RuleCode = RuleCode::new("LAV001");
pub(crate) const LAV002: RuleCode = RuleCode::new("LAV002");
pub(crate) const LAV003: RuleCode = RuleCode::new("LAV003");
pub(crate) const LAV004: RuleCode = RuleCode::new("LAV004");
pub(crate) const LAV005: RuleCode = RuleCode::new("LAV005");
pub(crate) const LAV006: RuleCode = RuleCode::new("LAV006");
pub(crate) const LAV007: RuleCode = RuleCode::new("LAV007");
pub(crate) const LAV008: RuleCode = RuleCode::new("LAV008");
pub(crate) const LAV009: RuleCode = RuleCode::new("LAV009");
pub(crate) const LAV011: RuleCode = RuleCode::new("LAV011");

static RULES: &[Rule] = &[
    Rule::new(
        LAV001,
        "list-index-in-loop",
        "list.index() inside a loop turns a lookup into a scan",
        "`list.index(x)` walks the list from the front until it finds `x`, so calling it once per \
         iteration over the same list costs O(n^2) comparisons in total. The usual intent is \
         \"where is this item\", which a dict built once before the loop answers in constant \
         time: `positions = {value: index for index, value in enumerate(items)}`. When the answer \
         wanted is really the index of the current item, `enumerate` supplies it for free. The \
         rule stays silent when the loop's trip count is fixed at compile time, because a scan \
         repeated three times is a constant.",
    ),
    Rule::new(
        LAV002,
        "list-membership-in-loop",
        "membership test against a list inside a loop",
        "`x in some_list` compares against every element until it matches, so a membership test \
         inside a loop is O(n*m). A `set` or `dict` answers the same question by hash in constant \
         time, and building one costs a single linear pass before the loop. The rewrite is almost \
         always `banned = frozenset(banned)` at module scope. Short literals are not reported: \
         against two or three constants a tuple comparison beats hashing, so `status in (\"ok\", \
         \"warn\")` is already the fastest spelling and rewriting it would be a pessimisation.",
    ),
    Rule::new(
        LAV003,
        "string-concat-in-loop",
        "a string accumulated with += inside a loop",
        "Python strings are immutable, so `out += piece` allocates a new string and copies \
         everything accumulated so far. Repeating that per iteration copies O(n^2) bytes even \
         though the output is linear in size. Collect the pieces in a list and call `\"\".join()` \
         once: one allocation, sized once, and the total work becomes linear. The rule requires \
         the accumulator to be initialised outside the loop and never reset inside it — a string \
         rebuilt from scratch each iteration is linear overall and is not reported.",
    ),
    Rule::new(
        LAV004,
        "list-front-mutation-in-loop",
        "insert(0, ...) or pop(0) on a list inside a loop",
        "A Python list is a contiguous array of pointers, so inserting at or removing from index \
         zero shifts every remaining element one slot. Doing it once per iteration while draining \
         or building a queue costs O(n^2) moves. `collections.deque` supports `appendleft` and \
         `popleft` in constant time and is a drop-in for queue use. `stack.pop()` with no index \
         removes from the end and is already O(1), so it is not reported, and neither is a single \
         `insert(0, header)` outside a loop.",
    ),
    Rule::new(
        LAV005,
        "nested-loop-same-collection",
        "a loop nested inside another loop over the same collection",
        "Two loops over the same unbounded collection, one inside the other, perform n^2 \
         iterations of the body. When the inner loop exists to find a partner for the outer item \
         — a duplicate, a match, the nearest value — an index built once before the outer loop \
         replaces the inner scan with a hash lookup and makes the whole thing linear. Nested \
         loops over *different* collections are not reported: their total cost is the size of the \
         inner data, not the product, which is the shape `for user in users: for role in \
         user.roles` has.",
    ),
    Rule::new(
        LAV006,
        "sort-inside-accumulating-loop",
        "sorting a list the same loop is still appending to",
        "Sorting costs O(m log m). Doing it inside the loop that is still growing the list \
         re-sorts an almost-sorted sequence n times, for O(n^2 log n) overall, to produce an \
         ordering only the final iteration needs. Sort once after the loop, or keep a \
         `heapq`-backed structure if a running order really is required at each step. A sort of a \
         per-item field list is not reported: nothing accumulates across iterations there, so the \
         total is linear in the input.",
    ),
    Rule::new(
        LAV007,
        "loop-invariant-collection-build",
        "a collection rebuilt every iteration from loop-invariant inputs",
        "Building a set, dict or comprehension whose inputs do not depend on the loop variable \
         repeats identical work every iteration: the build is O(m), so the loop pays O(n*m) for a \
         value that never changes. Hoisting the statement above the loop makes it O(m) once. The \
         rule fires only when hoisting is provably safe — the build must not read anything the \
         loop varies, and the result must never be mutated inside the body. A per-iteration \
         accumulator like `seen = set()` is deliberately excluded: hoisting one changes the \
         answer, not the cost.",
    ),
    Rule::new(
        LAV008,
        "repeated-slice-in-loop",
        "a slice whose length grows with the loop, taken every iteration",
        "Slicing a `str`, `bytes` or `list` copies the selected span. When the span grows with \
         the loop — `buffer = buffer[n:]` to consume, or `values[:i]` to accumulate — the copies \
         alone are O(n^2) even though each individual line looks cheap. Carry an integer offset \
         and index instead, or wrap the buffer in a `memoryview`, whose slices share storage and \
         copy nothing. Fixed-width slices such as `line[:19]` are constant-time and are not \
         reported, nor is a slice of the item the loop is currently on.",
    ),
    Rule::new(
        LAV009,
        "dataframe-growth-in-loop",
        "a dataframe grown one row or one frame at a time",
        "`pandas.concat` and the removed `DataFrame.append` both allocate a new frame and copy \
         every existing row, so growing a frame inside a loop copies O(n^2) rows in total and \
         re-infers dtypes each time. Collect the rows or frames in a plain list — `list.append` \
         is amortised O(1) — and call `pd.concat(frames)` or `pd.DataFrame.from_records(rows)` \
         once after the loop. A concat whose result is discarded each iteration is not reported: \
         nothing accumulates, so the total stays linear.",
    ),
    Rule::new(
        LAV011,
        "quadratic-regex-rescan",
        "a regex re-scanning a subject the loop keeps re-deriving",
        "Calling `re.search` on a fresh slice of the remaining text copies the tail *and* rescans \
         it from the start, so finding k matches in an n-byte document costs O(n*k). \
         `pattern.search(text, pos)` moves the start without copying, and `re.finditer` walks the \
         whole document in a single left-to-right pass. The same applies to substituting to a \
         fixpoint: `while previous != text: text = re.sub(...)` runs a linear pass per \
         replacement, where a repetition inside the pattern does it once. A scan per line, whose \
         total is linear in the file, is not reported.",
    ),
];
