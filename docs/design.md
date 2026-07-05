# Design: git-driven Podman containers

`graft` is a git-driven, TOML-first wrapper on top of Podman Quadlet.

Read [`vision.md`](vision.md) first for the current product vision: `graft` as
direnv for containers.

## Core idea

```text
Git repo              = source of truth
TOML configs          = declarative container definitions
NixOS/Home Manager    = installs the wrapper and points at configRoot
Podman Quadlet        = runtime/systemd executor
```

There are two routes:

```text
fast route:    TOML -> graft up -> transient Quadlet -> container runs
promote route: TOML -> review/branch/merge -> NixOS/HM managed Quadlet
```

## No implicit defaults

Empty always means: do nothing.

```toml
version = 1

[config]
# Empty means no-op.
```

or:

```toml
[containers]
# Empty means no-op.
```

must not implicitly activate a container, baseline, mount, or security policy.
Everything must come explicitly from TOML.

## TOML graph

A config file is a named unit. The name preferably comes from the TOML `name`.

Example:

```text
configs/my-app.toml -> my-app
```

A TOML file can reference parents and children:

```toml
version = 1
name = "my-app"

[parents]
add = ["base", "no-network"]

[children]
add = ["nix-store", "workspace"]

[config]
# Empty means no-op.
```

Resolution order:

```text
parents -> self -> children
```

Later layers may override earlier ones, so users can build their own presets,
addons, and parent/child combinations.

## Users define presets

There are no built-in presets such as `bare`, `safe`, or `agent` that
activate behaviour automatically. Users can define such presets themselves:

```toml
# configs/no-network.toml
version = 1
name = "no-network"

[config.network]
mode = "none"
```

```toml
# configs/with-nix-store.toml
version = 1
name = "with-nix-store"

[[config.filesystem.volumes]]
source = "/nix/store"
target = "/nix/store"
mode = "ro"
```

```toml
# configs/my-app.toml
version = 1
name = "my-app"

[parents]
add = ["no-network", "with-nix-store"]
```

These examples are indicative; the full schema is still being worked out.

## NixOS stays short

The NixOS/Home Manager configuration should only enable the package/module and
point at the git-tracked TOML.

Direction:

```nix
services.graft = {
  enable = true;
  configRoot = ./containers;
};
```

The contents of containers, presets, and deploy/session policy live in TOML, not
in `flake.nix`.

## Quadlet as the execution layer

Ultimately the wrapper renders effective TOML configs into native Podman Quadlet
units.

```text
TOML graph
  -> resolve/merge
  -> validate
  -> render .container/.volume/.network units
  -> systemd/Podman Quadlet runs them
```

So `graft` is not a replacement for Podman Quadlet, but a higher declarative
layer on top of it.

## Git-driven updates

Updates must not mutate the live environment directly.

Desired flow:

```text
tmp/candidate container
  -> update/install runs in isolation
  -> the result becomes a TOML/profile/snapshot change
  -> diff/PR
  -> merge
  -> switch/apply
```

The truth stays the Git repo, not runtime state such as:

```text
~/.config
~/.local/state
podman containers
npm cache
Pi runtime config
```

## Current scope

For now there is a working vertical slice: load TOML, inspect/render/run,
`graft up`, rootfs-store Quadlet, and a NixOS module with `configFiles`,
`configRoot` discovery, and `parents.*`/`children.*` resolution.

The planned separation is: TOML config engine, Quadlet runtime manager, and
cleanup/lifecycle policy.

See [`nixos-module.md`](nixos-module.md) for the first NixOS route that renders
TOML to `/etc/containers/systemd/*.container` during the Nix build.

Not building yet:

- no automatic containers;
- no built-in baseline;
- no implicit Quadlet units;
- no direct update flow.

Next goal: package refs/pins beyond simple `pkgs.<name>` strings, then session
lifecycle (`enter`/`leave`/`idle`).
