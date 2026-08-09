# Security Policy

## Reporting a vulnerability

Please report security issues privately through GitHub's
[private vulnerability reporting](https://github.com/snoodleboot-io/landav-core/security/advisories/new)
rather than opening a public issue.

We aim to acknowledge within three working days.

## What counts as a security issue here

Landav is an analysis tool, so the interesting cases are not the usual ones.

### Soundness bugs — report these privately

**A case where Landav reports a bound that the code can exceed is our highest
severity class**, and it takes priority over any feature work.

It is a security issue and not merely a correctness bug because of how Landav is
meant to be used: as a merge gate, and — via the reachability attribute
(`F-028`) — to flag superlinear bounds on attacker-controlled parameters. A
system that under-reports a bound can wave through exactly the
algorithmic-complexity vulnerability it was deployed to catch.

If you have a function where a derived bound is exceeded, please include the
function, the input, the reported bound and the calibration id.

### Ordinary vulnerabilities

The usual categories also apply: issues in the CLI's handling of untrusted
input, the solver bridge's handling of solver output, deserialisation of
signature packs and calibration profiles, and anything that turns analysing a
hostile repository into code execution.

Note that **analysing untrusted code is an explicit use case** — CI runs Landav
against pull requests. Treat parsing and lowering as untrusted-input paths.

### Out of scope

- Imprecise bounds that are still sound (too loose is a quality issue, not a
  security one)
- Failure to derive a bound at all — partial coverage is documented and expected
- Resource exhaustion caused by pointing Landav at pathological input *when it
  reports the timeout honestly*

## Supported versions

Pre-alpha. No released versions are supported yet; this policy is in place ahead
of the first release.
