# Graft — Overview

Graft runs Podman Quadlet containers from TOML files, built from the Nix store.
The user writes Graft TOML; the CLI resolves it to JSON; NixOS and Home Manager
materialise rootfs paths and Quadlet `.container` files.

The goal is a Nix-native container workflow with no image pulls, no ad-hoc
package installs inside containers, and no hand-written Quadlet boilerplate.

## Core flow

```text
Edit TOML
  ↓
nixos-rebuild switch / Home Manager activation
  ↓
Nix calls the Graft CLI via IFD
  ↓
CLI writes resolved JSON to stdout
  ↓
Nix reads the JSON
  ↓
Nix builds rootfs in the store → /nix/store/xxx-graft-<name>-env
  ↓
NixOS/Home Manager renders a Quadlet .container file
  ↓
target = "system" → /etc/containers/systemd/<name>.container
target = "user"   → ~/.config/containers/systemd/<name>.container
  ↓
systemd knows about the unit; it does not auto-start by default
```

## Responsibilities

### TOML

TOML is user intent only. It is not Quadlet and it is not Nix.

```toml
version = 1
name = "node-dev"

[config.runtime]
packages = ["nodejs"]
```

Users do not write rootfs boilerplate, `/nix/store` mounts, overlay setup, or
default keep-alive commands.

### CLI

The CLI translates TOML into a resolved JSON spec and writes that JSON to
stdout. The CLI owns defaults, dependencies, validation, and translation from
Graft concepts to the resolved spec.

Current CLI rules:

- require `version = 1`
- validate container names and supported values before JSON output
- add `graft-pause` to every rootfs
- use `/bin/graft-pause` when the user did not set a command
- preserve user commands exactly
- default `deploy.target` to `system`
- support only `rootfs-store` today
- include supported container, environment, filesystem, network, and service fields only when explicitly set
- include `deploy.enable` only when explicitly set
- never invent autostart

### Nix modules

The NixOS and Home Manager modules are dumb materialisers. They read resolved
JSON and mechanically:

1. filter for their target (`system` for NixOS, `user` for Home Manager)
2. map package names to Nix packages
3. build a `pkgs.buildEnv`
4. wrap it with real system directories (`/etc`, `/tmp`, `/var`, `/run`, ...)
5. render the Quadlet `.container` file
6. place the file in the matching Quadlet search path

The modules do not decide defaults or interpret TOML semantics.

## IFD and JSON stdout

The build integration uses Import From Derivation:

```nix
resolvedJson = pkgs.runCommand "graft-resolve-${name}" {} ''
  ${graft}/bin/graft ${tomlFile} > $out
'';

resolved = builtins.fromJSON (builtins.readFile resolvedJson);
```

The JSON is a Nix store artefact, not a file to commit.

Module-eval checks for this path should be built explicitly, for example with
`nix build .#checks.x86_64-linux.nixos-module-eval`. Because they use IFD,
`nix flake check` may omit them and must not be the only CI or release gate.

Cache behaviour:

```text
TOML unchanged        → same derivation → CLI does not run again
TOML changed          → CLI runs → new resolved JSON
packages changed      → rootfs changes
command/restart only  → Quadlet changes; rootfs may stay cached
```

## `graft-pause`

`graft-pause` is a tiny keep-alive binary shipped by the same Rust crate as the
CLI.

```text
/bin/graft
/bin/graft-pause
```

Rules:

```text
no user command → packages = ["graft-pause", ...], command = ["/bin/graft-pause"]
user command    → packages = ["graft-pause", ...], command = user command
```

`graft-pause` exits cleanly on `SIGTERM` and `SIGINT`, so stopping a Quadlet
service should not fall back to SIGKILL or leave a failed unit.

There is no default `bashInteractive`, no default `coreutils`, and no default
`sleep infinity`.

## Rendered Quadlet example

A TOML without a command resolves to a Quadlet file like:

```ini
[Container]
ContainerName=node-dev
Rootfs=/nix/store/xyz-graft-node-dev-env:O
Exec="/bin/graft-pause"
Volume=/nix/store:/nix/store:ro
```

Supported optional fields render mechanically when configured:

```ini
HostName=node-dev.local
User=1000
Group=1000
WorkingDir=/workspace
Environment="GREETING=hello world"
EnvironmentFile="/run/graft/node-dev.env"
PublishPort=127.0.0.1:8080:8080
Volume=/home/me/project:/workspace

[Service]
Restart=on-failure
RestartSec=10s
TimeoutStartSec=2m
TimeoutStopSec=30s
```

The current modules do not render `[Install]` by default. A `.container` file can
exist while the container does not start automatically. Future autostart support
must be explicit in TOML and resolved JSON before an `[Install]` section is
rendered.

## Rootfs-store container model

- Graft uses `Rootfs=`, not `Image=`, for store-based containers.
- The rootfs is a store path built from Nix packages.
- `Rootfs=...:O` gives Podman a writable overlay above the read-only store rootfs.
- `/nix/store` from the host is mounted read-only inside the container.
- Not in the store means not available in the container.
- No downloads happen at runtime.

System containers (`target = "system"`) use rootful Podman and kernel overlayfs
via `:O`. User containers (`target = "user"`) use rootless Podman and rootless
overlay support such as `fuse-overlayfs`.

## Everything is a service

All containers are Quadlet/systemd services. There is no separate shell-container
concept in the config model. A container stays alive as long as its resolved
`Exec=` process stays alive.

## Package management

Packages are declared in TOML and resolved at build time via Nix. To add a tool,
add it to `packages = [...]` and rebuild. Do not install packages ad-hoc inside
the container.

## Current scope

The current MVP proves the rootfs-store path for both NixOS and Home Manager.
It renders a useful subset of Quadlet fields, while the TOML schema remains
broader than the implemented renderer. See [Reference](reference.md) for the
current field list and [Non-goals](non-goals.md) for deliberate exclusions.

For the long-term direction, see [Roadmap](roadmap.md).

## Project structure

```text
graft/
  flake.nix
  modules/
    nixos.nix          # NixOS materialisation module
    home-manager.nix   # Home Manager materialisation module
  crates/
    graft/             # Rust package: CLI resolver + graft-pause
  examples/
    reference.toml     # annotated TOML reference
  docs/
    design.md          # design decisions and principles
    overview.md        # this file
    quadlet.md         # Quadlet output notes
    roadmap.md         # roadmap and future direction
    non-goals.md       # deliberate exclusions and deferred scope
    development.md     # contributor workflow and renderer checklist
```

## Flake outputs

- `nixosModules.graft` — system containers → `/etc/containers/systemd/`
- `homeManagerModules.graft` — user containers → `~/.config/containers/systemd/`
- `packages.<system>.default` — Graft CLI + `graft-pause`
