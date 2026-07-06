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

### Container resolution (`graft <name>`)

The CLI checks two places and combines them into one flow:

**Case 1 — system container only:**
- The container was built at nixos rebuild time (`.ini` is already in place for systemd)
- No local TOML in `pwd/.graft/containers/<name>.toml`
- CLI starts the container directly — no TOML needed at runtime

**Case 2/3 — local TOML present (in `.graft/containers/`):**
- No parent → use local TOML as-is
- Has parent (references a system container) → merge base TOML + local TOML
- CLI renders a Quadlet `.ini` from the merged config
- `.ini` is saved to `.graft/` in the project repo
- CLI starts that `.ini`
- By default temporary; if the TOML indicates persistent, the `.ini` stays in `.graft/`

**Write locations — strictly enforced:**
| Source | `.ini` location |
|--------|----------------|
| nixos module (build time) | nix store → systemd path (`/etc/containers/systemd/` or `~/.config/containers/systemd/`) |
| CLI (runtime, case 2/3) | `.graft/` in the project repo — nowhere else |

**Finding base TOMLs for merge (case 3):**
The nixos module writes the `configRoot` path to a known read location at activation
(e.g. `/etc/graft/config`). The CLI reads this to locate base TOMLs for merging.
The base TOMLs themselves stay in the nixos-config repo — they travel with the machine config
to other machines. The CLI never writes to this location.

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

## CLI Design (Phase 3+)

### Container TOML locations
- **Base TOML** — in nixos-config, in the directory set via `configRoot`; processed at rebuild, never touched at runtime
- **Local TOML** — in `pwd/.graft/containers/<name>.toml`; read by CLI at runtime

```
cd /my/project
graft devshell   # looks up 'devshell' in system containers
                 # AND checks .graft/containers/devshell.toml
                 # if local TOML has parent → merge → render .ini → .graft/
                 # if local TOML has no parent → render .ini → .graft/
                 # if only system container → start directly
```

### Persistence modes

| Mode | Description |
|------|-------------|
| `ephemeral` (default) | Nothing persists. Auto-cleanup after 1 week. |
| `persistent` | Also writes TOML to nixos-config. Available after rebuild as systemd service. |
| `data` | Container is temporary. `.graft/<name>/data/` persists for review/cleanup/promote. |

`data` mode is the foundation for the promote workflow: inspect `.graft/<name>/data/`, decide what to keep, move to real location.

### Smart shell detection (CLI)
When `shell = "auto"`, the CLI:
1. Detects `$SHELL` on the host
2. Finds relevant config files (e.g. `~/.zshrc`, `~/.config/zsh/` for zsh)
3. Bind-mounts only those files (read-only) into the container
4. Sets `GRAFT_CONTAINER=<name>` so the prompt can show a container indicator

## CLI Architecture

```
cli/src/
  main.rs
  commands/     ← one file per command (shell, up, down, run, list)
  container/    ← ContainerRuntime trait + Podman impl
  config/       ← ConfigProvider trait + TOML impl
  workspace/    ← WorkspaceProvider trait + .graft/ impl
```

### Layer rules
- `commands` may depend on `container`, `config`, `workspace`
- `container` knows nothing about TOML or workspaces
- `config` knows nothing about containers or workspaces
- `workspace` knows nothing about containers
- Dependencies are explicit in function signatures — no hidden state

### Traits
```rust
pub trait ContainerRuntime {
    fn start(&self, name: &str) -> Result<()>;
    fn stop(&self, name: &str) -> Result<()>;
    fn exec(&self, name: &str, cmd: &[&str]) -> Result<()>;
    fn status(&self, name: &str) -> Result<ContainerStatus>;
}
// Podman is one implementation — replaceable
```

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
