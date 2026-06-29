# Supply-chain update and cache flow

This document records the intended supply-chain flow for NixOS packages, container runtimes, npm/Pi.dev/addons, and later agent toolchains.

The goal is Renovate-like controlled updates:

```text
upstream updates
  -> scheduled pipeline discovers updates
  -> build/cache candidate closures
  -> scan/analyze candidate artifacts
  -> summarize release notes/security signals
  -> open PR/change for review
  -> only after approval: merge and deploy/run
```

## Core idea

Do not update the live system directly.

Instead:

1. detect new versions;
2. realize/build candidate packages/closures in CI;
3. push/cache those closures somewhere;
4. scan/check them;
5. present a reviewable diff/PR;
6. merge only after confidence;
7. NixOS/Home Manager/container configs then consume the approved lock/config.

This applies to:

- NixOS system packages;
- `graft` runtime packages;
- container TOML graph/package changes;
- container runtime closures;
- future OCI/image-based containers if supported;
- npm dependencies;
- Pi.dev/addons/extensions;
- future agent toolchains.

## Delayed N-x update policy

Preferred policy: do not follow upstream immediately. Use an `N-x` delay window.

Example:

```text
N     = newest discovered upstream/package/container update
N-7d  = only candidates that have existed for at least 7 days are eligible
```

So the pipeline may discover and cache new candidates immediately, but it should not propose them as review candidates until they have aged for at least one week, unless explicitly marked as an urgent security fix or test-only run. Even after the delay window, updates are **not automatic**: the delay only makes a candidate eligible for human review.

This gives time for:

- broken upstream releases to be yanked/fixed;
- vulnerability advisories to appear;
- other users/CI ecosystems to find regressions;
- release notes and changelogs to settle;
- binary cache/scanner results to complete.

Desired behavior:

```text
new version appears
  -> discover
  -> build/cache candidate
  -> scan/check
  -> hold until age >= 7 days
  -> mark eligible for review
  -> show changelog/release/security report
  -> user decides whether it is worth taking
  -> open/update PR only for selected updates
  -> merge only after approval
```

The default expectation is selective updates, not routine automatic updates. If the changelog is uninteresting or risk is unclear, the update can remain cached and skipped.

Possible TOML/pipeline metadata later:

```toml
[update]
minimumAge = "7d"
channel = "delayed" # delayed | security-fast | test
selection = "manual" # manual | security-only | all, default should be manual
```

Exceptions should be explicit:

```text
security-fast  = urgent security update, shorter delay, still reviewed
test           = may run in isolated test containers, not prod
```

## Nix cache-first update model

Desired weekly pipeline:

```text
flake.lock / TOML package refs
  -> update candidates
  -> nix build candidate system/container closures
  -> nix copy to binary cache
  -> scan/check metadata
  -> PR with lock/TOML changes and report
```

Important distinction:

```text
cached in / binary cache != deployed/running
```

A candidate closure may already be built and available from cache, but it should not be used by production containers or systems until the PR is reviewed and merged.

## Candidate artifacts

For each update candidate, the pipeline should collect:

- changed flake inputs;
- changed package versions;
- runtime closure paths;
- closure size diff;
- package/license metadata where available;
- release notes/changelogs;
- known vulnerability results;
- scan results;
- generated effective TOML/Quadlet diff for containers;
- test results.

## Security checks to investigate

Possible checks:

- vulnerability databases:
  - OSV.dev;
  - GitHub Security Advisories;
  - NVD where useful;
  - npm audit for npm graphs;
- malware/virus scanning:
  - ClamAV over fetched/source artifacts and selected outputs;
  - vendor-specific scanners if available;
- provenance/signature checks:
  - Nix binary cache signatures;
  - upstream signatures where packages expose them;
  - SLSA/provenance where available;
- license policy;
- suspicious install scripts for npm where possible;
- diff of dependency graph.

Virus scanning Nix store outputs has limits: many packages are source-built, packed, minified, or binary blobs. Results should be treated as signals, not absolute proof.

## PR shape

A generated PR should include:

```text
Title: update runtime/container deps YYYY-WW

Files:
  flake.lock
  containers/**/*.toml if package refs changed
  generated report markdown/json

Report:
  changed packages
  release notes links
  vulnerability findings
  scan findings
  closure/cache status
  tests run
  generated Quadlet diff if relevant
```

The reviewer decides whether to merge.

## Container-specific Renovate/SBOM flow

The same Renovate-like model must work for containers.

For containers managed by `graft`:

```text
TOML graph
  -> effective config
  -> package closure / runtime closure
  -> SBOM for effective runtime
  -> candidate cache
  -> vulnerability/malware/license/provenance checks
  -> release notes and package diff report
  -> user selects interesting updates
  -> PR
  -> merge
  -> NixOS/HM deploys approved version
```

For future OCI/image mode:

```text
image/tag/digest update candidate
  -> pull/build candidate image
  -> pin digest
  -> generate SBOM
  -> scan image layers
  -> compare with previous digest/SBOM
  -> PR only if selected
```

Container updates should be digest/closure based, not floating-tag based. A PR should show exactly what runtime closure or image digest will be used.

### Container SBOM artifacts

For each container candidate, collect:

- effective TOML;
- generated Quadlet;
- runtime package list;
- Nix closure store paths;
- closure size diff;
- SBOM in SPDX and/or CycloneDX if possible;
- vulnerability scan results;
- malware scan results where feasible;
- license summary;
- changed package changelog/release-note links;
- proxy/network/security profile summary;
- whether full `/nix/store` or closure-only store access is used.

Possible tools to evaluate:

- `nix path-info --recursive` for closure listing;
- `nix derivation show` / eval metadata for package provenance;
- `syft` for SBOM generation where it can understand outputs/images;
- `grype`/OSV scanner for vulnerability matching;
- `trivy` for image/filesystem scanning;
- `clamav` for coarse malware scanning;
- `npm audit` for npm lock graphs;
- GitHub Security Advisories / OSV.dev APIs.

Nix package-to-CVE mapping is imperfect. Reports should be treated as signals for review, not absolute truth.

For ephemeral update tools such as npm/Pi.dev:

```text
locked/proxy container
  -> one update action
  -> candidate workspace
  -> diff/report
  -> PR
```

The live container should not mutate itself into the source of truth.

## Test exception

There may be a test/dev mode where updates are built and run before full approval.

That should be explicit, e.g. future direction:

```toml
[deploy]
target = "user"
channel = "test"
```

or pipeline labels:

```text
safe-to-test
run-in-test-container
```

Even then, it should run in isolated/test containers, not silently update production/system containers.

## Open design questions

- Which binary cache: attic, cachix, nix-serve, SSH store, or existing infra?
- Where to store scan reports?
- How to map Nix package derivations to vulnerability database package names?
- How deep should closure scanning go?
- How to handle npm lockfiles inside candidate workspaces?
- How to represent approved pins in TOML vs flake.lock?
- How to distinguish test/staging/prod container channels?
- What is the minimum useful release-notes extraction?

## Relationship to graft

`graft` should eventually help with:

- effective config export;
- generated Quadlet diff;
- package/runtime closure listing;
- SBOM generation hooks;
- scan report collection;
- Renovate-like candidate grouping;
- promote/update PR generation;
- isolated test containers;
- maybe TUI review.

But the core source of truth remains:

```text
Git/JJ repo + TOML + flake.lock
```
