# Graft — Roadmap

This document describes the intended direction for Graft. It is not a promise of
exact command names or implementation order, except where noted.

## Current MVP

The current implementation proves the core rootfs-store path:

- TOML is the user-facing Graft DSL.
- The `graft` CLI resolves one TOML file to JSON on stdout.
- NixOS and Home Manager consume that JSON via IFD.
- Nix builds a rootfs from Nix packages.
- The modules render Podman Quadlet `.container` files.
- `target = "system"` renders system/rootful containers.
- `target = "user"` renders user/rootless containers.
- Containers start and stop through systemd.
- `graft-pause` provides a tiny default keep-alive command.
- Common Quadlet fields are rendered for container identity, working directory,
  quoted environment, environment files, published ports, volumes, and service
  timing.

The MVP intentionally does not cover the full TOML schema yet.

## Direction

Graft should become a Nix-native container workflow for both local development
and multi-host deployment:

```text
repo TOML + host TOML
  ↓
graft resolve / merge
  ↓
Nix materialisation
  ↓
Quadlet services
  ↓
local dev or server deploy
```

The same Graft language should describe a local dev container, a user/rootless
workspace, and a system/rootful service on a server.

## Devenv workflow

Graft should be useful as a project-local development environment.

Goals:

- A repository can contain Graft TOML describing its dev environment.
- Developers can bring that environment up without hand-written Quadlet files.
- User/rootless containers should be a natural fit for local development.
- Packages are declared in TOML and realised by Nix, not installed ad-hoc inside
  the container.
- Explicit autostart can be modelled later for dev sessions, but there is no
  implicit autostart default.

Agreed lifecycle command names:

```text
graft up
graft down
```

No `graft shell` command is planned.

## CLI control plane

The CLI should grow from a build-time resolver into the main user interface for
Graft workflows, while keeping build-time resolution deterministic.

Likely responsibilities:

- resolve and inspect TOML configs
- lint TOML intent before rebuilds
- run host-aware diagnostics through a future `graft doctor` command
- inspect generated Quadlet, systemd, and Podman state
- start and stop containers through systemd
- show status and logs
- coordinate local development flows
- coordinate deployment flows
- expose diff/promote workflows

`graft lint` should stay mostly pure and TOML-focused. `graft doctor` may check
local host state such as user linger, generated units, mounted paths, and Podman
state, but it should report diagnostics rather than mutate host policy
implicitly.

Implementation detail: runtime operations should stay separate from pure
TOML-to-JSON resolution so Nix evaluation stays deterministic and side-effect
free.

## Merge workflow across repositories

Graft should support definitions from multiple sources:

- host/system container TOMLs
- project/repository TOMLs
- shared base TOMLs
- environment-specific overlays

Goals:

- deterministic merge order
- explicit `parents` / `children` graph resolution
- clear conflict detection
- useful validation errors
- no hidden state between modules
- repo-level container intent without host-specific boilerplate

The CLI owns merge semantics. Nix modules should continue to materialise only
resolved JSON.

## Multi-server deployment

Graft should be able to describe and deploy containers across multiple NixOS
machines.

Goals:

- one declarative language for local and server containers
- per-host materialisation of resolved containers
- CLI-assisted deployment to one or more hosts
- compatibility with normal NixOS rebuild/deploy workflows
- explicit target selection for system vs user containers

The deployment layer should not turn TOML into a second NixOS module language.
TOML remains user intent; Nix remains the materialisation substrate.

## Promote / diff workflow

Rootfs-store containers use writable overlay state above a read-only Nix store
rootfs. That overlay can become the basis for review workflows.

Goals:

- inspect changes made inside a running container
- diff overlay upperdir against the generated rootfs
- promote state/config changes back into declarative files
- never promote binaries or package-manager output
- keep packages managed by TOML + Nix rebuilds

Promote should help users capture intentional configuration/state changes, not
turn containers into mutable images.

## Security hardening

The current MVP proves the flow, not the final isolation model.

Planned hardening:

- `userns=auto`
- per-container limited UIDs
- workdir-only write access
- explicit mount policies
- explicit network policies
- secrets support
- resource limits

System containers and user containers may need different defaults, but those
rules should be resolved by the CLI and materialised mechanically by the Nix
modules.

## Broader Quadlet coverage

The TOML schema already contains more concepts than the MVP renders. The current
renderer covers useful basics, but later phases should map more of the schema
into resolved JSON and Quadlet output.

Remaining areas include:

- additional mount types beyond basic `Volume=` entries
- Quadlet `.network` and `.volume` units
- secrets and credentials
- resources and health checks
- labels and annotations
- DNS, aliases, and network policy
- podman args / explicit escape hatches

Escape hatches must not override keys owned by Graft.

## Non-goals and constraints

The detailed list of deliberate exclusions lives in
[Non-goals and deferred scope](non-goals.md).

The short version:

- TOML should not become raw Quadlet.
- TOML should not become raw Nix.
- Nix modules should not contain business logic.
- Packages should not be installed ad-hoc inside containers.
- Containers should not auto-start unless explicitly configured.
- Promote should never promote binaries.
- Hidden module state should be avoided.
