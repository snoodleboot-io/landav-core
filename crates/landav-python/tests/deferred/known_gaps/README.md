# Known gaps — defects the rules cannot currently see

Nothing here is asserted. These files exist so that "the rule set does not
report this" is a *recorded* fact rather than something a user discovers and
reports as a bug, or worse, relies on.

A gap is conservative: a missed finding costs coverage, never soundness. That
is why none of these is a defect in the rules. It is also why none of them is
a fixture — putting a comprehension form into a `negative/` directory would
assert that it *must stay* silent, and the day someone correctly closes the gap
the harness would fail and call the fix a regression. A gap must be recorded
somewhere that cannot ossify into a requirement.

## Gap 1 — comprehensions and generator expressions are invisible to every rule

**Confirmed empirically**, not inferred: the two files here fire nothing, while
the byte-for-byte equivalent statement forms in `tests/fixtures/` fire LAV002
and LAV005 respectively.

| This file | Silent | Statement equivalent | Fires |
|---|---|---|---|
| `comprehension_membership_scan.py` | yes | `LAV002_list_membership_in_loop/positive/banned_names_list.py` | `LAV002` |
| `comprehension_nested_scan.py` | yes | `LAV005_nested_loop_same_collection/positive/pairwise_checksum_scan.py` | `LAV005` |

**Cause.** Loop context is built only from `for`, `async for` and `while`
*statements* (`src/context.rs`). A comprehension's generators are part of an
expression, so no loop context is created and no rule that needs one can fire.
Every one of the ten rules needs one.

**Consequence, and the reason this is worth writing down.** A user can silence
any rule in the set by rewriting the loop as a comprehension. That is a rewrite
which changes nothing about the asymptotics — `[x for a in xs for b in xs]` is
the same quadratic as the nested statement form — so the suppression is
accidental rather than deliberate, and nobody involved will know it happened.
It also interacts badly with the obvious advice a reviewer gives ("make that a
comprehension"), which would remove a true finding as a side effect of a style
change.

**Scale.** Comprehensions are not a corner of Python. Two of the ten rules have
a comprehension as their *recommended fix* (`LAV003` → `"".join(...)`,
`LAV009` → a list comprehension of frames), so the idiom is one the rule set
actively pushes people towards.

**What closing it would take.** Comprehension generators would have to
contribute loop contexts alongside statement loops: an iterable, a bound
target, and a body that is the element expression plus the conditions. The
per-rule logic should then apply unchanged, since it consumes loop contexts
rather than statements. The trip-count and loop-invariance machinery would need
to understand that a comprehension's scope is its own.

**Do not close it by relaxing the statement requirement without also giving
comprehensions a scope.** A comprehension binds its target in a scope of its
own; treating its generators as if they were statements in the enclosing
function would make loop-invariance analysis wrong in the direction that
produces false positives, which is the failure mode this corpus exists to
prevent.
