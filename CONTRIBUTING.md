# Contributing to Landav

Thank you for looking. Landav is pre-alpha and the interfaces are moving
quickly, so please read the section that matches what you want to do.

## Right now, the most valuable contribution is a corpus entry

Landav's hardest problem is not derivation quality — it is **coverage on real
code**. If you have a function whose complexity surprised you in production, we
want it:

1. The function, reduced as far as it still misbehaves
2. The input shape that made it slow
3. What you expected, and what happened

Open an issue with the `corpus` label. This is more useful to us than a patch.

## Code contributions

Until R0 (Foundation) ships, outside patches are likely to be invalidated by
interface churn. If you want to work on something substantial, open an issue
first so we can tell you whether the ground is moving under it.

### The rules that are not negotiable

**1. Soundness has a zero target.** A reported bound the code can exceed is the
single class of bug that invalidates the product. A patch that improves
tightness or coverage at the cost of soundness will be rejected regardless of
how much it improves the numbers.

**2. Never panic in library code.** Use `Result<T, E>`, propagate with `?`, and
add context. A panicking analyser is worse than one that reports a partial
bound, because the partial bound carries blame and the panic carries nothing.
`unwrap_used`, `expect_used` and `panic` are lint-warned at the workspace level.

**3. Failure must carry blame.** When derivation fails, emit a partial bound
naming the unaccounted term and the assumption that could not be discharged —
never a bare "unknown". This is the property that makes Landav complementary to
review rather than redundant with it.

**4. No Python assumptions in Core.** Twenty-one of thirty-six components are
language-neutral and must stay that way. Reference behaviour, exception model,
integer width, iteration protocol and dispatch are all parameterised by the
frontend. If you find yourself reaching for a Python fact inside a Core crate,
the abstraction is in the wrong place.

**5. No licence or entitlement logic.** This repository contains none, by
design. See [`docs/EDITIONS.md`](docs/EDITIONS.md).

### Conventions

- Rust 1.95+, edition 2024, `rustfmt` and `clippy` clean
- `snake_case` files, `PascalCase` types, `UPPER_SNAKE` constants
- Errors via `thiserror`, in the libraries **and** in the binary. `landav-cli`
  carries a typed, blame-carrying `ToolError` by design: non-negotiable 3 says
  a failure must name its subject and the reason it failed, and `ToolError` has
  no constructor that omits either — a blameless error is unrepresentable.
  `anyhow::Error` accepts any string and so cannot enforce that, which is why
  the binary does not depend on it
- Group imports: `std` → third-party → internal, blank line between groups
- Prefer borrowing over cloning; prefer traits over concrete types
- Flag any new dependency explicitly in the PR description

### Testing

Coverage targets: **80%** line, **70%** branch, **90%** function, **85%**
statement, **80%** mutation.

Tests are co-located with source. The bound algebra in particular is
property-tested rather than example-tested — weak monotonicity and
composition-by-substitution soundness are properties, and a handful of examples
will not catch a violation.

#### Required tools

```sh
cargo install cargo-llvm-cov --locked --version 0.8.7   # line/branch/function coverage
cargo install cargo-mutants  --locked --version 27.1.0  # mutation coverage
```

Pin both. An unpinned tool makes the coverage and mutation gates drift between
your machine and CI, and a gate that means something different in two places is
not a gate.

`cargo-mutants` matters more here than on a typical project: **a surviving
mutant means the property set is too weak**, whatever line coverage claims. On
an analyser whose central promise is soundness, that distinction is the whole
point.

#### Property tests are owned by the test author

Property tests under `tests/properties/` encode the acceptance criteria. They
are written **before** the implementation and **by someone other than the
implementer**.

**If you are implementing a feature, do not edit its property tests to make them
pass.** Weakening a property needs sign-off from whoever wrote it. Without that
rule the property quietly softens until the code passes, and a zero-target
metric becomes decorative.

If a property is genuinely wrong, say so and get it changed deliberately — that
is a real outcome, and it should leave a trace.

### Commits and branches

Conventional Commits. Branches follow `{type}/{ticket}-{description}`:

```
feat/LAN-1-bound-algebra-core-types
bugfix/LAN-42-conformal-interval-off-by-one
hotfix/LAN-99-unsound-bound-in-nested-loop
```

Ticket IDs are real Linear issues (`LAN-*`). Every feature in the roadmap
already has one — do not invent identifiers.

## Reporting a soundness bug

A case where Landav reported a bound the code exceeded is the highest-severity
issue we accept, and it stops the line. Please report it privately first — see
[SECURITY.md](SECURITY.md).

## Licence

By contributing you agree that your contributions are licensed under
Apache-2.0.
