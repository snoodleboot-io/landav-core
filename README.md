<!-- Replace with landav-lockup-light.png / landav-lockup-dark.png from the brand kit before the repo goes public. -->
# Landav

**Symbolic resource analysis for real codebases.** Landav derives and verifies
resource bounds statically, as functions of input size — then turns them into
concrete numbers against a calibrated machine.

> Your profiler tells you what *was* slow. Landav tells you what *will be* slow.

---

## Status

**Pre-alpha.** This repository is a scaffold. R0 (Foundation) starts
2026-08-10; the first thing that will work is `landav calibrate`, because
nothing else is trustworthy until a tick is calibrated.

Nothing here is usable yet. Watch the repository rather than installing it.

## What it does

Landav answers a question profilers and linters cannot:

| | |
|---|---|
| **vs. profilers** | A profiler requires the slow input to already exist and be reproduced. Landav reasons about inputs you have not seen. |
| **vs. linters** | `PERF`-class rules match patterns someone already named. Landav *derives* bounds. |
| **vs. benchmark suites** | Benchmarks detect regressions after the fact at fixed sizes. Differential analysis catches them on the diff. |
| **vs. an LLM reviewing the diff** | A model shares the generator's failure modes — a quadratic written in clean, conventional Python *looks correct* to a system whose notion of correctness is largely a notion of typicality. Landav is deterministic, and sound where it succeeds. |

Where it succeeds, the bound is a **theorem about the code**, not a judgement
about how the code reads. Where it fails it produces **blame**: not "unknown",
but `O(n · f)` with `f` named as the unaccounted cost of a specific call — so a
reviewer knows exactly where human attention is needed.

## What we will not claim

**Completeness.** Coverage on real code will be partial for the foreseeable
future. Every output states its assumptions, its calibration, and what it could
not account for.

We track **coverage**, **soundness** and **tightness** as three *separate*
metrics, deliberately. Reporting tightness alone, on the subset one happens to
handle, is the standard way this field flatters itself.

**Soundness has a zero target.** A reported bound the code can exceed is the
single class of bug that invalidates the product, and it stops the line.

## Architecture

Thirty-six components across eight layers. The load-bearing distinction is
**scope**:

| Scope | Meaning | Cost |
|---|---|---|
| **Core** | Written once, language-neutral | 21 components · 644 points |
| **FDK** | The plugin boundary a language implements | — |
| **Frontend** | Per-language; recurs for every language | ~90 points *each* |
| **Surface** | CLI, output formats, CI integrations | — |

That roughly seven-to-one ratio is the whole argument for a language-neutral
Core. It only holds if the boundary is real, so it is enforced with a lint and
measured with a language-independent conformance suite.

Languages ship in order of what they prove: **Python** (hardest typing story,
most acute pain — proves the interface against the worst case), then
**TypeScript** (largest population, stronger type system), then the **JVM**
(ground proven by COSTA and Infer; where the enterprise buyer lives).

### Crates

| Crate | Component | Purpose |
|---|---|---|
| [`landav-bound`](crates/landav-bound) | C-01, C-02 | Bound algebra and cost semiring — the vocabulary |
| [`landav-calibrate`](crates/landav-calibrate) | C-03 | Calibration harness, profile format and loader |
| [`landav-its`](crates/landav-its) | C-07 | Integer transition system exporter |
| [`landav-solvers`](crates/landav-solvers) | C-13 | KoAT and LoAT bridge |
| [`landav-fdk`](crates/landav-fdk) | C-33 | **The plugin boundary** |
| [`landav-python`](crates/landav-python) | C-04, C-05, C-31, C-34 | Python frontend — the reference FDK implementation |
| [`landav-cli`](crates/landav-cli) | C-26 | The `landav` binary |

Crates for the remaining components land as their releases do.

### The load-bearing structure

Runtime bounds and size bounds are **mutually recursive**. Size bounds are
needed to lift a local runtime bound to a global one; runtime bounds are needed
to compute size bounds, because a runtime bound says how many times a local
change accumulates. The two alternate to fixpoint, with subprograms processed in
topological order.

This matters more than the choice of IR, and it is the one piece worth getting
exactly right before anything else is built on it.

## Open core

Landav is open core. The boundary is deliberate and documented:

| | |
|---|---|
| **This repository** (`landav-core`, Apache-2.0) | Every analysis engine, cost contracts, the IR, the CLI, output formats, all language frontends, the FDK, and the CI integrations. **The verdict is always free.** |
| **`landav-ee`** (internal, BSL 1.1) | The hosted platform: org policy governance, cross-repo history, telemetry-fed size envelopes, premium calibration and signature packs, SSO and billing. |

Two rules keep that honest:

1. **No licence checks live in this repository.** Not stubbed, not
   feature-gated — absent. This crate tree knows how to *load a pack*; it does
   not know whether something decided you were allowed to have it.
2. **The paid product accumulates; it never analyses.** `landav-ee` consumes
   the same JSON this CLI emits. There is no forked analysis path and no
   EE-only analysis capability.

See [`docs/EDITIONS.md`](docs/EDITIONS.md) for the full boundary and the
reasoning behind each call.

## Building

Requires Rust 1.95 or later.

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Contributions are welcome once R0 lands;
until then the interfaces are moving too quickly for outside patches to be a
good use of anyone's time.

If you want to help now, the most valuable thing is a **corpus contribution**: a
real function whose complexity surprised you, with the input that made it slow.

## Licence

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
