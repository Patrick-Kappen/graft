# Development

This page captures the working rules for changing Graft. It is mainly for
contributors and release preparation.

## Core workflow

For every issue:

1. read the issue, recent changelog entries, and learnings
2. confirm scope before editing
3. use one branch per issue
4. keep changes focused
5. update tests and docs with the code
6. run the relevant checks
7. record exactly one changelog line and one learning after approval

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
- `examples/reference.toml`
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
- render mechanically with shared renderer escaping for systemd syntax
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
- container identity: user-only, group-only, user plus group, neither

Prefer small resolver tests for semantic combinations and module-eval assertions
for generated Quadlet text.

## Documentation parity

Resolver rules must be mirrored in user-facing docs.

If the resolver rejects or requires something, update:

- Rust tests
- `docs/reference.md`
- `examples/reference.toml`

If the behaviour affects generated Quadlet output, also update
`docs/quadlet.md`. If the behaviour changes the visible project scope, update
`README.md`, `docs/index.md`, `docs/overview.md`, or `docs/roadmap.md` as
appropriate.

## Runtime verification

Local unit/module tests prove evaluation and rendering. Runtime-sensitive
features should also be validated after merge through a real
NixOS → Quadlet → Podman path.

For privileged local runtime tests, use the dedicated tmux socket/session:

```bash
tmux -S /tmp/AI-tests.sock attach -t AI-tests
```

Create a new window per test. The user enters sudo passwords there; do not pass
or log sudo passwords through the agent session.

Runtime test output should check the generated unit text and the Podman runtime
state where possible.

## Standard checks

For Rust changes:

```bash
nix develop .#ci -c bash -lc 'cd crates/graft && cargo fmt --check && cargo test'
nix develop .#ci -c bash -lc 'cd crates/graft && cargo clippy --all-targets -- -D warnings -D clippy::pedantic'
```

For Nix/module/docs changes:

```bash
nix flake check
nix develop .#ci -c mdbook build
git diff --check
```
