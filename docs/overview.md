# Graft — Overview

## What is Graft?

Graft runs Podman Quadlet containers from TOML files, built entirely from the
Nix store. The user writes TOML in Graft's own language; Graft resolves it during
`nixos-rebuild switch`; NixOS materialises the result as rootfs + `.container`
files.

Software lives in the Nix store and is never installed ad-hoc inside the
container. The host stays clean — no PATH pollution and no downloaded images.

## Core Flow

```text
Edit TOML
  ↓
nixos-rebuild switch
  ↓
Nix calls the Graft CLI via IFD
  ↓
CLI writes resolved JSON to stdout
  ↓
Nix reads the JSON
  ↓
Nix builds rootfs in the store → /nix/store/xxx-graft-<name>-env
  ↓
NixOS renders a Quadlet .container file
  ↓
Symlink: /etc/containers/systemd/<name>.container → store
  ↓
systemd knows about the unit; it does not auto-start unless explicitly configured
```

## Responsibilities

### TOML

TOML is user intent only. It is not Quadlet and it is not Nix.

The user writes Graft concepts:

```toml
version = 1
name = "node-dev"

[config.runtime]
packages = ["nodejs"]
```

The user does not write rootfs boilerplate, `/nix/store` mounts, overlay setup,
or default keep-alive commands.

### CLI

The CLI translates TOML into a resolved JSON spec and writes that JSON to stdout.
It does not write JSON files into the repo.

The CLI owns all logic:

- add `graft-pause` to every rootfs
- use `/bin/graft-pause` as the default command when the user did not set one
- preserve the user command when one is set
- default `deploy.target` to `system`
- support only `rootfs-store` for now
- include `restart` only when explicitly set
- include `deploy.enable` only when explicitly set
- never invent autostart

### NixOS module

The NixOS module is a dumb materialiser. It reads resolved JSON and mechanically:

1. maps package names to Nix packages
2. builds a `pkgs.buildEnv`
3. wraps it with real system directories (`/etc`, `/tmp`, `/var`, `/run`, ...)
4. renders the Quadlet `.container` file
5. places the symlink in the Quadlet search path

The module does not decide defaults or interpret TOML semantics.

## IFD / JSON stdout

The build integration is Import From Derivation:

```nix
resolvedJson = pkgs.runCommand "graft-resolve-${name}" {} ''
  ${graft}/bin/graft ${tomlFile} > $out
'';

resolved = builtins.fromJSON (builtins.readFile resolvedJson);
```

The JSON is a Nix store artefact, not a file to commit.

Cache behaviour:

```text
TOML unchanged → same derivation → CLI does not run again
TOML changed   → CLI runs → new resolved JSON
packages changed → rootfs changes
command/restart changed → Quadlet changes, rootfs may stay cached
```

## `graft-pause`

`graft-pause` is a tiny keep-alive binary shipped by the same Rust crate as the
CLI. The `graft` package provides both binaries:

```text
/bin/graft
/bin/graft-pause
```

Rules:

```text
no user command → packages = ["graft-pause", ...], command = ["/bin/graft-pause"]
user command    → packages = ["graft-pause", ...], command = user command
```

There is no default `bashInteractive`, no default `coreutils`, and no default
`sleep infinity`.

## Rendered Quadlet example

For a TOML without a command, the resolved spec leads to a Quadlet file like:

```ini
[Container]
ContainerName=node-dev
Rootfs=/nix/store/xyz-graft-node-dev-env:O
Exec=/bin/graft-pause
Volume=/nix/store:/nix/store:ro
```

If the user explicitly sets restart, NixOS may render:

```ini
[Service]
Restart=on-failure
```

If the user explicitly configures autostart, NixOS may render:

```ini
[Install]
WantedBy=multi-user.target
```

Without explicit autostart, the `.container` file can exist while the container
does not start automatically.

## Rootfs-store container model

- Graft uses `Rootfs=`, not `Image=`, for store-based containers.
- The rootfs is a store path built from Nix packages.
- `Rootfs=...:O` gives Podman a writable overlay above the read-only store rootfs.
- `/nix/store` from the host is mounted read-only inside the container.
- Not in the store means not available in the container.
- No downloads happen at runtime.

System containers (`target = "system"`) use rootful Podman and kernel overlayfs
via `:O`. User containers (`target = "user"`) use rootless overlay via
`fuse-overlayfs`.

## Everything is a service

All containers are Quadlet/systemd services. There is no separate “shell
container” concept in the config model. A container stays alive as long as its
resolved `Exec=` process stays alive.

## Package Management

Packages are declared in TOML and resolved at build time via Nix. To add a tool,
add it to `packages = [...]` and rebuild. Do not install packages ad-hoc inside
the container.

## Project Structure

```text
graft/
  flake.nix
  modules/
    nixos.nix          # NixOS materialisation module
    home-manager.nix   # Home Manager materialisation module
  cli/                 # Rust CLI resolver
  examples/
    reference.toml     # annotated TOML reference
  docs/
    design.md          # design decisions and principles
    overview.md        # this file
    quadlet.md         # Quadlet output notes
```

## Flake Outputs

- `nixosModules.graft` — system containers → `/etc/containers/systemd/`
- `homeManagerModules.graft` — user containers → `~/.config/containers/systemd/`
- `packages.<system>.default` — Graft CLI + `graft-pause`

## Roadmap

- [x] Phase 1 — Proof of concept: container from Nix store
- [x] Phase 2 — NixOS module: TOML → buildEnv → Quadlet, end-to-end proof
- [ ] Phase 3 — CLI resolver: TOML → resolved JSON stdout
- [ ] Phase 4 — NixOS module refactor: resolved JSON → rootfs + Quadlet
- [ ] Phase 5 — Home Manager module refactor
- [ ] Phase 6 — Promote/diff workflow
- [ ] Phase 7 — Graph resolution: `parents` / `children`
- [ ] Phase 8 — Security hardening (`userns=auto`)
