# Graft — Overview

## What is Graft?

Graft runs Podman containers configured via TOML files, built entirely from the
Nix store. Software lives in the Nix store and is never installed or exposed on
the host. The host stays clean — no PATH pollution, no downloaded images.

## Core Flow

```
Edit TOML
  ↓
nixos-rebuild switch
  ↓
CLI resolves (base packages, defaults, dependencies)
  ↓
Nix builds rootfs in store  →  /nix/store/xxx-graft-<name>-env
  ↓
Quadlet .container file generated (complete blueprint, nothing more needed)
  ↓
Symlink: /etc/containers/systemd/<name>.container → store
  ↓
systemd knows about it — container does NOT start automatically unless configured
```

## How It Works

1. User writes `.toml` files in their NixOS config under `containers/`
2. The NixOS module reads each TOML via `builtins.fromTOML` at build time
3. The CLI resolves the complete spec: adds base packages, default keep-alive, dependencies
4. `buildEnv` merges all packages into one store path
5. A `pkgs.runCommand` wraps the env with real system directories (`/etc`, `/tmp`, etc.)
6. A Quadlet `.container` file is rendered pointing to that store path
7. `/nix/store` is mounted `:ro` inside the container
8. The file lives in the Nix store; the module creates a symlink to `/etc/containers/systemd/`

## Everything is a Service

All containers are systemd services managed via Quadlet. There is no distinction
between a "service container" and a "shell container" — both run their `Exec=`
command and stay alive as long as that process runs.

The keep-alive mechanism and base packages are handled automatically by the CLI.
The user only specifies what is extra.

## Container

- Rootfs = Nix store path (read-only lowerdir) + overlay upperdir (writable, gone on stop)
- `/nix/store` from the host mounted read-only inside the container
- Not in the store = not available in the container
- No downloads at runtime — everything resolved at build time

## Rendered Quadlet (example)

```ini
[Container]
ContainerName=myapp
Rootfs=/nix/store/xyz-myapp-env:O
Exec=bash -c 'sleep infinity'
Volume=/nix/store:/nix/store:ro

[Service]
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

## TOML

Only user choices. Minimal example:

```toml
version = 1
name    = "myapp"

[deploy]
enable = true
target = "system"
```

Extra packages only when needed:

```toml
[config.runtime]
packages = ["nodejs"]
```

## Package Management

Packages are declared in the TOML and resolved at build time via Nix.
Nothing is installed ad-hoc. To add a tool: add it to `packages = [...]` and rebuild.

## CLI Commands

- `graft up <name>` — start the container (`systemctl start <name>.service`)
- `graft down <name>` — stop the container
- More commands added as needed; names are user-defined

## Project Structure

```
graft/
  flake.nix
  modules/
    nixos.nix          # NixOS module (services.graft)
    home-manager.nix   # Home Manager module (programs.graft)
  cli/                 # Rust CLI
  examples/
    reference.toml     # fully annotated example TOML
  docs/
    design.md          # design decisions and principles
    overview.md        # this file
```

## Flake Outputs

- `nixosModules.graft` — system containers → `/etc/containers/systemd/`
- `homeManagerModules.graft` — user containers → `~/.config/containers/systemd/`
- `packages.<system>.default` — the Graft CLI binary

## Tech Stack

- Module: **Nix** (build-time rendering)
- CLI: **Rust** (config generation, container lifecycle)
- Runtime: **Podman + Quadlet**
- Init: **systemd**
- Packages: **Nix (`buildEnv`)**

## Security Model

### Overlay approach
- **System containers** (`target = "system"`): kernel overlayfs via `:O` (runs as root)
- **User containers** (`target = "user"`): `fuse-overlayfs` for rootless overlay

### Phase 8: user isolation
Each container will get its own restricted UID via `--userns=auto`.

## Roadmap

- [x] Phase 1 — Proof of concept: container from Nix store
- [x] Phase 2 — NixOS module: TOML → buildEnv → Quadlet, end-to-end working
- [ ] Phase 3 — CLI: `graft up/down`, config generation in Rust
- [ ] Phase 4 — Promote workflow: diff on exit, graduate to store
- [ ] Phase 5 — Home directory and workspace isolation
- [ ] Phase 6 — Agent environments
- [ ] Phase 7 — Graph resolution: `parents` / `children` for composable configs
- [ ] Phase 8 — Security hardening (`userns=auto`)
