# Development

This page captures the working rules for changing Graft. It is mainly for
contributors and release preparation.

## Core workflow

For every issue:

1. read the issue and related docs or code
2. confirm scope before editing
3. use one branch per issue
4. keep changes focused
5. update tests and docs with the code
6. run the relevant checks
7. note follow-up work in the issue or pull request

Keep implementation decisions aligned with the main architecture:

```text
TOML → CLI → JSON stdout → NixOS/Home Manager modules → Quadlet .container
```

The CLI owns defaults, validation, dependency resolution, and semantic decisions.
The Nix modules should stay dumb materialisers.

## Quadlet renderer checklist

Apply this checklist whenever adding or changing a rendered Quadlet field.

Before implementation, decide and document:

- whether the field is already in the TOML schema
- the resolved JSON shape
- omitted behaviour
- empty list or map behaviour, if applicable
- invalid empty or whitespace-only values
- control-character handling
- ordering: sorted output or user order
- literal passthrough versus parser/policy
- whether NixOS and Home Manager must render the same output
- whether runtime verification is needed after merge

During implementation, update:

- Rust resolver tests
- NixOS module rendering
- Home Manager module rendering
- Nix module-eval assertions in `flake.nix`
- `docs/reference.md`
- the current-field pipeline in `docs/capabilities.md`
- other manual pages when the user-facing behaviour changes

## Ordering policy

Choose ordering deliberately.

Map-like values with unstable source order should be sorted for deterministic
output. Current example:

- `config.container.environment` → sorted by key

List-like or precedence-sensitive values should preserve user order. Current
examples:

- `config.container.environmentFile`
- `config.network.publish`
- `config.filesystem.volumes`

Document the choice in `docs/reference.md`.

## Literal passthrough policy

Some upstream syntaxes are broad and already validated by Podman, Quadlet, or
systemd. Do not accidentally replace those syntaxes with a narrow parser.

For broad syntaxes, prefer line-safe passthrough:

- reject empty or whitespace-only values when the field is present
- reject control characters
- render mechanically with field-appropriate renderer escaping
- do not add a parser, allowlist, or policy without a dedicated issue

Current examples:

- `PublishPort=` values
- `Volume=` strings assembled from TOML parts
- systemd service timing values such as `RestartSec=`

If an implementation repeatedly says "out of scope", "not yet", or "no parser
yet", update [Non-goals and deferred scope](non-goals.md) or link a tracking
issue.

## Matrix tests

Fields with valid combinations need matrix tests. Cover both valid combinations
and impossible combinations.

Examples:

- volume rendering: `target`, `source:target`, `source:target:mode`
- invalid volume rendering: `mode` without `source`
- service rendering: restart-only, timing-only, restart plus timing
- container identity: user-only, invalid group-only, user plus group, neither

Prefer small resolver tests for semantic combinations and module-eval assertions
for generated Quadlet text.

## Documentation parity

Resolver rules must be mirrored in user-facing docs.

If the resolver rejects or requires something, update:

- Rust tests
- `docs/reference.md`
- `docs/capabilities.md`

If the behaviour affects generated Quadlet output, also update
`docs/quadlet.md`. If the behaviour changes the visible project scope, update
`README.md`, `docs/index.md`, `docs/overview.md`, or `docs/roadmap.md` as
appropriate.

## Runtime verification

Local unit/module tests prove evaluation and rendering. Runtime-sensitive
features should also be validated after merge through a real
NixOS → Quadlet → Podman path.

Run privileged runtime checks on a local test machine where the operator can
approve privilege escalation directly. Do not require maintainer-specific
sessions, sockets, or other local-only workflow details in the public manual.

Runtime test output should check the generated unit text and the Podman runtime
state where possible.

Startup activation has an isolated x86_64 NixOS VM test that exercises real
system and rootless user managers, declarative linger, tty login, Podman,
specialisation transitions, and reboot behavior:

```bash
nix build .#packages.x86_64-linux.activation-runtime-test --no-link --print-build-logs
```

This expensive test is an advisory `activation-runtime` CI job and is not part
of the aggregate required checks while runner stability is evaluated.

## Dead-code and module-boundary hygiene

The baseline already has several hard gates for unused or dead code:

- Rust warnings, including compiler dead-code warnings, via clippy with
  `-D warnings -D clippy::pedantic`
- unused Rust dependencies via `cargo machete`
- orphaned Rust source files via `cargo modules orphans` for the library and
  standalone `graft-pause` binary
- unused Nix code via `deadnix --fail`
- missing Rust test coverage via `cargo llvm-cov --fail-under-lines 80`

Do not add unstable or noisy hygiene gates without a focused design issue.
`cargo-udeps` currently requires nightly-only rustc flags in this environment,
so track it as an advisory/local-only candidate in #96 and the later local
quality workflow in #23. Treat `tokei`/`scc` output as refactor signals, not CI
thresholds; `resolve.rs` splitting is tracked in #97. Public API usage remains a
manual visibility review until a low-noise tool path is proven; track that in
issue #98.

## Machine-readable TOML schema

`crates/graft/schema/graft-v1.schema.json` is generated from the Rust parser
types with unsupported/reserved fields excluded through schema-only attributes.
Do not edit the JSON by hand.

Regenerate it after an intentional supported-schema change:

```bash
nix develop .#ci -c bash -lc 'cd crates/graft && cargo run --example generate-schema > schema/graft-v1.schema.json'
```

The `schema` integration test compares generated and tracked JSON byte for byte
and asserts the supported property sets. `.taplo.toml` applies the schema to
runnable examples and `tests/nix` fixtures; `taplo lint` therefore checks parser
shape, examples, and fixture drift in CI. The `documentation-drift` Nix check
recursively compares every supported semantic schema path with the marked
current-field rows in `docs/capabilities.md`; missing, extra, and duplicate rows
fail the build.

Adding a parser field does not automatically make it supported intent. Expose it
in the machine-readable schema only when the resolver and materialiser implement
it. Normal resolution fails closed for every explicitly configured reserved
field, including `false`, zero, and empty leaf values; empty parent sections with
no configured fields remain valid. `validation.level` cannot downgrade this
contract. Classify every new parser field in the exhaustive unsupported-intent
validation and extend its field-path matrix before merging it.

## Standard checks

For Rust changes:

```bash
nix develop .#ci -c bash -lc 'cd crates/graft && cargo fmt --check && mkdir -p target/nextest && cargo nextest run --profile ci && cargo test --doc'
nix develop .#ci -c bash -lc 'cd crates/graft && cargo clippy --all-targets -- -D warnings -D clippy::pedantic'
nix develop .#ci -c bash -lc 'cd crates/graft && cargo machete'
nix develop .#ci -c bash -lc 'cd crates/graft && NO_COLOR=1 cargo modules orphans --lib && NO_COLOR=1 cargo modules orphans --bin graft-pause'
```

The nextest run writes JUnit test results to
`crates/graft/target/nextest/ci/junit.xml` for Codecov uploads.

Generate Rust coverage locally and enforce the 80% line threshold:

```bash
nix develop .#ci -c bash -lc '
  set -euo pipefail
  cd crates/graft
  mkdir -p target/coverage
  export LLVM_COV="$(command -v llvm-cov)"
  export LLVM_PROFDATA="$(command -v llvm-profdata)"
  cargo llvm-cov --workspace --all-features --fail-under-lines 80 --lcov --output-path target/coverage/lcov.info
'
```

For dependency security and policy checks:

```bash
nix develop .#ci -c bash -lc 'cd crates/graft && cargo-audit audit'
nix develop .#ci -c cargo deny --manifest-path crates/graft/Cargo.toml check --config deny.toml
```

For secret scanning, copy tracked files to a temporary directory so ignored local
files stay out of scope:

```bash
nix develop .#ci -c bash -lc '
  set -euo pipefail

  scan_root=$(mktemp -d)
  cleanup() {
    rm -rf "${scan_root}"
  }
  trap cleanup EXIT

  git ls-files -z | tar --null --files-from=- -cf - | tar -xf - -C "${scan_root}"
  gitleaks dir --no-banner --no-color --redact "${scan_root}"
'
```

For workflow changes:

```bash
nix develop .#ci -c actionlint
nix develop .#ci -c zizmor --no-progress --color never --min-confidence high .github/workflows/*.yml .github/actions/setup-nix/action.yml
```

For Nix/module/docs changes:

```bash
nix develop .#ci -c bash -lc 'git ls-files "*.nix" -z | xargs -0 nixfmt --check'
nix develop .#ci -c bash -lc 'git ls-files "*.toml" -z | xargs -0 taplo format --check'
nix develop .#ci -c bash -lc 'git ls-files "*.toml" -z | xargs -0 taplo lint'
nix develop .#ci -c bash -lc 'git ls-files "*.md" -z | xargs -0 markdownlint-cli2 --config .markdownlint.jsonc'
nix develop .#ci -c bash -lc 'git ls-files "*.md" | lychee --files-from - --offline --include-fragments --no-progress'
nix develop .#ci -c bash -lc '{ git ls-files "*.md"; git ls-files "examples/quickstart/**"; } | typos --file-list -'
nix build .#checks.x86_64-linux.documentation-drift --print-out-paths
nix develop .#ci -c statix check .
nix develop .#ci -c deadnix --fail .
nix build \
  .#checks.x86_64-linux.nixos-module-eval \
  .#checks.x86_64-linux.home-manager-module-eval \
  .#checks.x86_64-linux.quadlet-activation \
  .#checks.x86_64-linux.quadlet-lifecycle \
  .#checks.x86_64-linux.quadlet-network \
  --print-out-paths
nix flake check
nix build .#packages.x86_64-linux.activation-runtime-test --no-link --print-build-logs
network_rootfs=$(nix build .#checks.x86_64-linux.network-runtime-rootfs --no-link --print-out-paths)
GRAFT_REQUIRE_NETWORK_RUNTIME=1 nix develop .#ci -c tests/runtime/network.sh "${network_rootfs}"
nix develop .#ci -c mdbook build
git diff --check
```

The module-eval and Quadlet generator checks use IFD, so build them explicitly.
The activation check verifies fixed system/user target links, lifecycle
combinations, absent intent, dependency activation, and generator reruns. The
separate activation runtime package validates actual system and user-manager
transitions in a VM. `nix flake check` may omit these IFD-backed checks and must
not be the only Nix module or generated-service gate. The rootless network
runtime test reports an explicit skip when Podman is unavailable or the
execution environment blocks rootless containers. Set
`GRAFT_REQUIRE_NETWORK_RUNTIME=1` on a capable host to make availability
mandatory.
