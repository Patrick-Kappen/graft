# Rename: `podman-agent-container` / `pac` → `graft`

> Short handoff note for a human or agent picking up this repo. Date: 2026-06-30.

## What changed

The whole project was renamed. Old names you may encounter in older branches,
commits, issues, or notes all refer to exactly this project:

| Old | New |
|---|---|
| project name `podman-agent-container` | `graft` |
| CLI/binary `podman-agent-container` + alias `pac` | `graft` (one binary, **no more `pac` symlink**) |
| Go module `github.com/zerodawn1990/podman-agent-container` | `github.com/zerodawn1990/graft` |
| `cmd/podman-agent-container/` | `cmd/graft/` |
| config files `pac.toml` / `.pac.toml` / `podman-agent-container.toml` | `graft.toml` / `.graft.toml` (+ `config.toml` fallback) |
| config dir `~/.config/podman-agent-container/` | `~/.config/graft/` |
| runtime workdir `$XDG_RUNTIME_DIR/podman-agent-container/` | `$XDG_RUNTIME_DIR/graft/` |
| NixOS module `services.podman-agent-container` | `services.graft` |
| Home Manager module `programs.podman-agent-container` | `programs.graft` |
| sample names `pac-*`, env `PAC_*`, label `managed-by=pac` | `graft-*`, `GRAFT_*`, `managed-by=graft` |

Flake input for consumers: `inputs.graft.url`.

## Why `graft`

"Graft" = grafting a scion onto a rootstock — exactly the override/inheritance
model of the TOML graph (parents → self → children). It is also genuine Nix
jargon (`nixpkgs` grafting) and git/mercurial terminology, so it signals domain
knowledge. Chosen over `cairn`/`roost` because those sit semantically closer to
(and are partly taken by) AI-agent / safety tools.

## Namesake warning (do not confuse)

There is a prominent, unrelated project **`orbitinghail/graft`** (~1.5k★, a Rust
transactional storage engine, `graft.rs`). That is **not** this project. For
search/SEO use `graft nix` / `graft podman` / `graft containers`. It is
cross-ecosystem, so there is no practical confusion, but be alert when searching
for "graft".

## Status of the change

The rename was done in one mechanical pass and validated end to end:
`gofmt`/`go vet` clean, `go test ./...` green, `nix build .#default` produces
`result/bin/graft`, and `nix flake check` fully green (all NixOS + Home Manager
checks). The `vendorHash` in `flake.nix` was refreshed along with it.

See [`roadmap.md`](roadmap.md) for the strategic context of the name choice.
