# Graft — Overview

## What is Graft?
Graft runs Podman Quadlet containers configured via TOML files. It replaces
`nix develop` / `nix shell` with true container isolation. Software lives in
the Nix store and is never installed or exposed on the host.

## Vision
- Define a container in a TOML file inside your NixOS config (`containers/myapp.toml`)
- During `nixos-rebuild switch`, every TOML is read, a `buildEnv` is built, and a Quadlet file is rendered — all in the Nix store
- If `target = "system"`, the Quadlet file is placed in `/etc/containers/systemd/` and systemd manages the container
- The TOML is never touched after build — updating = edit TOML + rebuild
- The host stays clean — no PATH pollution, no installed packages
- Nothing is downloaded or built at runtime

## Core Flow

```
Edit TOML
  ↓
nixos-rebuild switch
  ↓
Nix reads TOML → buildEnv → Quadlet file in store
  ↓
target = "system"? → /etc/containers/systemd/ → systemd starts container
                    → no → ready in store, waiting for CLI
```

## Use Cases
- **Services** — long-running daemons managed by systemd via Quadlet
- **Dev shells** — isolated development environments, started on demand via CLI
- **Agent environments** — isolated, reproducible environments for AI agents
- **Testing** — pinned environments, only updated when explicitly changed

## How It Works
1. User writes `.toml` files in their NixOS config under `containers/`
2. The NixOS module reads each TOML via `builtins.fromTOML` at build time
3. For each container, `buildEnv` merges the listed packages into one store path
4. A `pkgs.runCommand` wraps the env with real system directories (`/etc`, `/tmp`, etc.)
5. A Quadlet `.container` file is rendered pointing to that store path
6. `/nix/store` is mounted `:ro` inside the container
7. Containers with `target = "system"` go to `/etc/containers/systemd/`
8. All other containers are pre-rendered in the store, ready for the CLI

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

## Package Management
Packages are always declared in the TOML and resolved at build time via Nix.
Nothing is installed ad-hoc inside the container. To add a tool: add it to
`packages = [...]` in the TOML and rebuild.

## Promote Workflow (Phase 4)
After a container run, Graft will show a diff of state/config changes made
inside the container. The user can selectively promote those back into the
TOML definition. Binaries are never promoted — those always come from Nix.

## Container Lifecycle
- `deploy.enable = true` → auto-starts at boot via systemd
- `deploy.enable = false` → on-demand via CLI

CLI wraps systemd:
- `graft shell <name>` → starts container if not running, then `podman exec -it`
- `graft down <name>` → `systemctl stop <name>`

## Project Structure
```
graft/
  flake.nix
  modules/
    nixos.nix          # NixOS module (services.graft)
    home-manager.nix   # Home Manager module (programs.graft)
  examples/
    reference.toml     # fully annotated example TOML
  docs/
    overview.md        # this file
    reference.md       # full option reference
```

## Flake Outputs
- `nixosModules.graft` — system containers → `/etc/containers/systemd/`
- `homeManagerModules.graft` — user containers → `~/.config/containers/systemd/`

## Tech Stack
- Module: **Nix** (build-time rendering)
- CLI: **Rust** (Phase 6 — `graft up`, `graft shell`, `graft diff`, `graft promote`)
- Runtime: **Podman + Quadlet**
- Init: **systemd**
- Packages: **Nix (`buildEnv`)**

## Roadmap
- [x] Phase 1 — Proof of concept: container from Nix store
- [x] Phase 2 — NixOS module: TOML → buildEnv → Quadlet
  - Fixed: rootless user containers require pre-created mount points in rootfs:
    `/etc/mtab → /proc/mounts`, `/etc/hostname`, `/etc/hosts`, `/etc/resolv.conf`, `/run/.containerenv`
- [ ] Phase 3 — Dev shell: interactive containers via CLI
- [ ] Phase 4 — Promote workflow: diff on exit, graduate to store
- [ ] Phase 5 — Agent environments
- [ ] Phase 6 — CLI tool (Rust)
- [ ] Phase 7 — Graph resolution: `parents` / `children` for composable configs
- [ ] Phase 8 — Security hardening

## Security Model

### Container isolation
Podman gives each container its own mount namespace. The container only sees:
- Its rootfs (the Nix store path via overlay)
- Explicitly mounted volumes (`Volume=`)

Host filesystem, home dir, SSH keys — none of it is visible unless explicitly mounted.

### Overlay approach
- **System containers** (`target = "system"`): kernel overlayfs via `:O` flag (runs as root, no issue)
- **User containers** (`target = "user"`): `fuse-overlayfs` for rootless overlay

Overlay layers:
```
lowerdir = /nix/store/xxx        (read-only, Nix store)
upperdir = ~/.local/share/graft/<name>/upper  (all writes go here)
workdir  = ~/.local/share/graft/<name>/work   (internal overlay use)
```

The `upperdir` is the promote/diff layer — after a container run, it contains exactly what changed.

### Phase 8: user isolation
Each container will get its own restricted UID via `--userns=auto`. Combined with
a single writable workdir, the container user has no rights outside its designated scope:

```toml
[config.security]
userns = "auto"

[config.workdir]
path = "/workspace"
```

This is especially important for agent environments.

## Updating Graft (after pushing to graft repo)
```bash
nix flake update graft
rebuild-all
```
