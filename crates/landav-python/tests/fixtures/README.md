# The `F-005` pattern-rule fixture corpus

Ten rules, 21 positive fixtures, 55 negative ones. Every file is real Python 3
and is kept `py_compile`-clean; a fixture with a syntax error tests nothing.

```text
tests/fixtures/
  LAV003_string_concat_in_loop/
    positive/*.py   the genuine defect; every one must fire, at an exact position
    negative/*.py   idiomatic Python that resembles it; none may fire, ever
```

The directory name is `{code}_{rule name with '-' as '_'}`, which is what ties
the corpus to `landav_python::registry`. `tests/rule_registry.rs` asserts the
correspondence in **both** directions, so a rule cannot be documented but
untested, nor tested but unregistered.

## The negative tree is the load-bearing half

A negative fixture asserts **zero findings from any rule**, not merely from the
rule whose directory it sits in. The whole negative tree is therefore one
shared false-positive suite: a new rule that fires on `"".join(...)`,
`stack.pop()`, `deque.popleft()`, a `memoryview` window or
`compiled.search(text, pos)` fails here even though none of those files was
written with it in mind.

This is deliberate and it is the assertion that decides whether the rule set
survives contact with a real repository. A rule that fires on every occurrence
of a pattern gets switched off within a week, and a rule set that has been
switched off has no soundness properties at all.

Each rule carries at least two negatives chosen to trip a naive
implementation — typically a provably small constant loop bound, an idiom the
pattern does not apply to, and a case where the slow-looking call is on a
different object from the one the loop accumulates into.

## How an expectation is written

A positive fixture marks each expected finding with a trailing comment on the
line the finding must be reported at:

```python
out += format_row(row)  # LANDAV: LAV003 anchor=out += format_row(row)
```

* the **line** is the line the marker itself is on;
* the **column** is derived: the 1-based UTF-8 byte offset of the anchor text
  within the part of the line *before* the marker. The anchor must occur
  exactly once there, and an ambiguous anchor fails the fixture rather than the
  rule.

Deriving the column instead of writing a number keeps the expectation readable
and survives re-indentation, while still asserting an exact column. **A finding
is reported at the offending expression, not at the enclosing loop** — the edit
happens at the expression, and a rule that reports the `for` line fails here.

A negative fixture must contain no markers at all, which is checked, so a
positive fixture misfiled into `negative/` cannot pass by being ignored.

## Rule codes

`LAV` plus exactly three digits. No collision with `ruff`, `pylint` or `flake8`
in the same CI log; fixed width so lexical order matches numeric order in
baselines. `LAV0xx` is superlinear time, `LAV1xx` reserved for memory growth,
`LAV2xx` reserved for findings derived from an inferred bound.

**Codes are permanent and are never reused**, because a suppression comment in
someone's repository names one. `LAV010` is retired — see
`../deferred/LAV010_exception_as_control_flow_in_loop/README.md` — and
`tests/rule_registry.rs::retired_codes_are_not_reused` enforces that it stays
retired.

| Code | Rule |
|---|---|
| `LAV001` | `list-index-in-loop` |
| `LAV002` | `list-membership-in-loop` |
| `LAV003` | `string-concat-in-loop` |
| `LAV004` | `list-front-mutation-in-loop` |
| `LAV005` | `nested-loop-same-collection` |
| `LAV006` | `sort-inside-accumulating-loop` |
| `LAV007` | `loop-invariant-collection-build` |
| `LAV008` | `repeated-slice-in-loop` |
| `LAV009` | `dataframe-growth-in-loop` |
| `LAV010` | *retired — withdrawn at R0, never to be reassigned* |
| `LAV011` | `quadratic-regex-rescan` |

## Known gaps

Recorded, with evidence, in `../deferred/known_gaps/`. The one to know about:

> **Comprehensions and generator expressions are invisible to every rule.**
> Loop context is built only from `for`/`while` *statements*, so rewriting a
> loop as a comprehension silences any rule in the set without changing the
> asymptotics.

Conservative, so not a defect — a missed finding costs coverage, never
soundness — but it means "the corpus is green" is not "the rule set sees this
code". Gaps are deliberately recorded outside `tests/fixtures/`: a gap placed
in a `negative/` directory would assert that it must *stay* a gap, and the day
someone closes it the harness would fail and call the fix a regression.

## Two things that will look like bugs and are not

**`LAV002`'s eight-element floor.** Membership against a list literal of fewer
than eight elements is not reported, because a scan of a handful of interned
constants beats hashing the probe; reporting `method in ["GET", "HEAD"]` would
be advice to make the code slower. The positive fixture
`LAV002_list_membership_in_loop/positive/banned_names_list.py` sits at exactly
eight entries. Trimming that list to tidy it up will stop the fixture firing.

**Positive fixtures may produce findings from other rules.** A genuine defect
often trips more than one pattern, so the harness matches exactly only on the
codes a fixture has an opinion about — its directory's code, plus any code an
explicit marker names. The strictness lives in the negative tree, which is
where the false-positive budget is actually spent.

## Verifying

```sh
cargo test -p landav-python
python3 -m py_compile $(find crates/landav-python/tests -name '*.py')
```
