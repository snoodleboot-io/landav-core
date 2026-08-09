# Editions: what is open, what is not, and why

Landav is **open core**. This document is the authoritative statement of where
the line sits, so that neither contributors nor customers have to guess.

The line is tracked in Linear with a mutually-exclusive `Edition` label group:
`OSS`, `EE`, `Boundary`.

| Label | Repository | Licence |
|---|---|---|
| `Edition/OSS` | `landav-core` | Apache-2.0 |
| `Edition/Boundary` | `landav-core` | Apache-2.0 |
| `Edition/EE` | `landav-ee` | BSL 1.1 |

`Boundary` items ship in this repository like any other OSS item. The label
means something narrower: **this code defines an interface that `landav-ee`
plugs into**, so changes to it are breaking-change reviewed against both
repositories.

## The principle

> The verdict is always free. What is paid is the org-level accountability
> around the verdict, and the operational burden of keeping inputs fresh.

Concretely:

- Every **analysis engine** is OSS. There is no bound Landav can derive that
  requires a licence.
- Every **language frontend** is OSS.
- The **CLI** and the **CI integrations** are OSS, and stay free. Go-to-market
  Phase 3 pricing is explicitly *"Free CLI, paid team features"* and
  *"Free for OSS"*.
- The **differential engine** — the thing that makes a merge gate possible — is
  OSS. It is the most tempting thing to charge for and it stays free, because
  the bottom-up land motion requires that the thing people adopt is the thing
  that produces the verdict.

## What is EE, and why

| Item | Rationale |
|---|---|
| Hosted platform, ingestion, dashboards (`F-039`) | Sells the *trend*, not the run. Org-level history is a service, not a capability. |
| Org policy governance (`E-001`) | A merge gate is somebody's responsibility. Waiver authority, policy inheritance and an audit trail are what "responsible" means at org scale. |
| Entitlement and licence enforcement (`E-002`) | Lives entirely on the EE side — see below. |
| Premium calibration profiles (`E-003`) | The *harness* is OSS; the maintained matrix and its freshness SLA are operational work. |
| Telemetry-fed size envelopes (`E-004`) | The envelope *interface* is OSS with manual providers; the provider that maintains itself is the platform capability. |
| Tenancy, SSO, RBAC (`E-005`) | Enterprise table stakes with no OSS analogue. |
| Billing and metering (`E-006`) | Self-evidently. |
| pandas/numpy signature pack (`F-032`) | ⚠️ **Under review** — see below. |

## Two rules that keep the split honest

### 1. No entitlement logic in `landav-core`

Not stubbed, not feature-gated, not disabled by default — **absent**.

An Apache-2.0 repository with a licence check in it invites exactly one kind of
fork, and the check is trivially patchable anyway. It buys nothing and costs
credibility. Enforcement lives in `landav-ee`, which is what premium packs and
the hosted service are distributed through.

The OSS side only knows how to *load a pack*. It does not know, and must never
learn, whether something decided the user was allowed to have it.

### 2. The paid product accumulates; it never analyses

`landav-ee` consumes exactly the JSON schema this repository's CLI emits
(`F-021`). No forked analysis path, no EE-only analysis capability.

This keeps incentives aligned: a customer's history is only as good as the OSS
runs feeding it, so investment in the OSS analysis is investment in the paid
product.

## Boundary items and their EE counterparts

These are the seams. Each is an interface in this repository with at least one
implementation here and at least one in `landav-ee`.

| Interface (OSS) | OSS implementation | EE implementation |
|---|---|---|
| `F-003` calibration profile format + loader | self-calibration on your machine | `E-003` curated, maintained matrix |
| `F-013` signature pack registry | stdlib pack | `F-032` pandas/numpy pack |
| `F-016` size envelope provider | declaration, DB schema, app config | `E-004` production telemetry percentiles |
| `F-021` structured output schema | the CLI writes it | `F-039` the platform ingests it |
| `F-026` policy resolver | local policy file | `E-001` org governance |
| `F-043` FDK plugin boundary | Python, TypeScript, JVM frontends | premium packs load through it |
| `F-045` cost contract specification | reference implementation | — (published spec; open on purpose) |

### One plugin mechanism, not two

Language frontends and EE extensions use the **same** boundary (`F-043`). If the
EE extensions ever need a second mechanism, the FDK design is wrong.

### Packs are data

Signature packs and calibration profiles are runtime-discovered **data**, never
compiled in. This is what keeps the OSS/EE split reversible — and there is at
least one call we may want to reverse.

## Open questions

### `F-032` — the pandas/numpy signature pack

Currently scoped **EE**. This is the most debatable call in the split and is
flagged for review before R4.

**For EE:** a maintained pandas/numpy pack is high-effort, high-value, and
continuously decaying as those libraries change. It is the clearest candidate
for something worth paying for that is not the engine.

**Against:** GTM Phase 4's ideal customer profile is "data-heavy and scientific
Python shops", and its motion is coverage rising per account. pandas/numpy
coverage *is* the coverage those accounts care about. Paywalling it may cost
more in adoption and corpus contribution than it captures.

**Likely resolution:** a baseline pack in OSS — correct but not exhaustive —
with depth, breadth and a maintenance SLA in EE.

Note the asymmetry to resolve alongside it: the TypeScript *standard-library*
pack (`F-048`) is OSS. Stdlib-versus-third-party is a defensible line, but it
should be drawn deliberately.

## Changing the line

Moving an item across the boundary is a product decision, not an engineering
one. Record it here and re-label in Linear. Moving something **from EE to OSS**
is always safe; moving **from OSS to EE** after it has shipped under Apache-2.0
is not possible for the code already released, so err toward OSS when uncertain.
