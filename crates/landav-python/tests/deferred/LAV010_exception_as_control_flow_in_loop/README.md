# LAV010 — `exception-as-control-flow-in-loop`, withdrawn at R0

**Status: withdrawn, not narrowed and not deferred pending a small fix.**
`LAV010` is retired as a code and must never be reassigned to a different
rule — `tests/rule_registry.rs::retired_codes_are_not_reused` enforces that,
because a suppression comment naming `LAV010` in someone's repository must not
start suppressing something else after an upgrade.

The rule fired on `try`/`except` inside a loop where the handler's only effect
was `continue`, `pass`, or a default assignment — exceptions used as a filter
or as a default, where a total lookup was assumed to exist and be cheaper.

I wrote this rule's fixtures and flagged it at the time as the highest
false-positive risk of the eleven. The false-positive adversary confirmed it,
and the reason it fails is worse than "the pattern needs a tighter syntactic
guard". This file records why, because the rule is attractive and **will be
proposed again**.

## Why it cannot work as a syntactic rule

The rule rested on two premises. Both are false, and they fail independently.

### Premise 1: a total-lookup rewrite exists

Falsified by two files here:

* `sqlite_row_has_no_get.py` — `sqlite3.Row.__getitem__` raises `IndexError`
  for an unselected column and the class exposes **no** `.get`. The suggested
  rewrite does not exist. `dict(row).get(...)` builds a dict per row, which is
  strictly more work than the handler.
* `element_has_no_children.py` — `Element.get` exists but reads an XML
  *attribute*. `element[0]` indexes the children. The rewrite is not a faster
  spelling of the same thing; it is a different operation that returns `None`
  for every element in the document. Suggesting it is suggesting a bug.

Deciding premise 1 needs the receiver's type. The frontend does not have type
inference at R0, so this half is blocked on `C-05`.

### Premise 2: the rewrite is never slower

This is the half that matters, and **type inference does not fix it.**

`dispatch_table_hot_path.py` is structurally identical to the positive
`except_keyerror_as_filter.py`: same `for`, same `try`, same subscript load
into a name, same `except KeyError:` whose only statement is `continue`. The
receiver is provably a `dict` — module-level, built by a function that returns
`{}` — so a perfect type oracle would confirm premise 1 and the rule would
still be **wrong to fire**.

It is wrong because in CPython setting up `try` costs nothing; only *raising*
costs. `d[k]` is a single `BINARY_SUBSCR`. `d.get(k)` is an attribute load plus
a Python-level call, paid on **every** iteration. When misses are rare, EAFP is
the faster spelling and `.get` is a pessimisation of the hot path.

So the two files differ only in **how often the handler is taken**, which is a
runtime frequency. My own positive asserts a high miss rate in its docstring
and nowhere in its syntax. A syntactic pass cannot read a docstring's claim,
and there is no narrowing of the AST that separates these two files, because
*the AST does not differ in the respect that decides the answer*.

Restricting to `KeyError` was considered and rejected: it drops both
`IndexError` files above and still fires on `dispatch_table_hot_path.py`.

## What would actually unblock it

Not type inference alone. The rule needs a **lower bound on how often the
exception edge is taken**, expressed relative to the loop's trip count — fire
when the handler runs Θ(n) times in a loop of n, stay silent when it runs O(1)
times. That is a derived-bound finding, not a pattern match.

Which means that if it returns, it returns in the `LAV2xx` block —
findings derived from an inferred bound — and not in `LAV0xx`, the block
reserved for syntactic superlinear patterns. `LAV010` stays burned either way.

Two further things would be needed before it is worth attempting:

1. **Type resolution (`C-05`)** for premise 1 — necessary, not sufficient.
2. **A cost model that distinguishes an unwind from a call.** The gap between
   `d[k]` and `d.get(k)` is a small constant factor, so the rule is a
   constant-factor argument dressed as a complexity one. Landav's claim is
   about bounds; a rule whose whole content is "this constant is larger than
   that constant" is a different product, and probably belongs to a profiler.

## The negatives are the part worth keeping

Eight negatives survive here. Three are the adversary's; five are mine. The one
I would most want a future proposer to read before writing any code:

**`open_has_no_race_free_guard.py`** — a loop that opens each of a list of
paths, recording failures. There is no non-racy look-before-you-leap test for
`open`: `os.path.exists` is a TOCTOU bug, and the handler is the *only* correct
way to express it. Any version of this rule that fires here is recommending a
race condition. This case is easy to re-derive the hard way and expensive to
re-derive in production.

The others, briefly:

| File | What it defends |
|---|---|
| `dispatch_table_hot_path.py` | EAFP as the faster spelling; the case that killed the rule |
| `sqlite_row_has_no_get.py` | Receiver with no total lookup at all |
| `element_has_no_children.py` | Receiver whose `.get` is a different operation |
| `open_has_no_race_free_guard.py` | LBYL is a TOCTOU bug |
| `bounded_retry_loop.py` | Loop bound is a small literal; the handler *is* the retry |
| `try_finally_without_except.py` | `try`/`finally` costs nothing until something raises |
| `stop_iteration_terminates_the_loop.py` | Handler runs once for the whole loop, not once per iteration |
| `try_wraps_the_whole_loop.py` | Setup outside the loop, paid once |

The two positives keep their `# LANDAV: LAV010 anchor=try:` markers. They are
inert — nothing walks this tree — and they are retained so that the withdrawn
rule's intended line and column survive with it.
