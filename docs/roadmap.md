# Graft — Roadmap

This document describes the active implementation direction for Graft. It is
not a promise of exact command names or implementation order, except where
noted. The broader product endgame is recorded in [Long-term vision](vision.md)
and does not change this roadmap's current delivery priority.

## Current MVP

The current implementation proves the core rootfs-store path:

- TOML is the user-facing Graft DSL.
- The `graft` CLI resolves one TOML file to JSON on stdout.
- NixOS and Home Manager consume that JSON via IFD.
- Nix builds a rootfs from Nix packages.
- The modules render Podman Quadlet `.container` files.
- `target = "system"` renders system/rootful containers.
- `target = "user"` renders into the current Home Manager account's user
  manager; Podman is rootless only for a non-root account.
- Containers start and stop through systemd.
- `graft-pause` provides the default keep-alive command for implicit and
  long-running lifecycle intent; finite workloads require a command.
- Common Quadlet fields are rendered for container identity, working directory,
  quoted environment, environment files, published ports, volumes, qualified
  CDI references, secure defaults and typed relaxations, service timing, and typed
  systemd dependency relationships.

The generated TOML schema intentionally exposes only the implemented MVP
contract. Additional parser-recognised roadmap fields fail closed; their status
is recorded in [Capability status](capabilities.md).

## Direction

Graft should become a Nix-native container workflow for both local development
and explicit multi-host deployment:

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

The same Graft language should describe a local dev container, a non-root
user/rootless workspace, and a system/rootful service on a server. This roadmap first treats
those as independently materialised workloads on explicit targets. A later
workload graph that deliberately spreads components across local, remote, and
temporary placements is direction only; see [Long-term vision](vision.md).

## Devenv workflow

Graft should be useful as a project-local development environment.

Goals:

- A repository can contain Graft TOML describing its dev environment.
- Developers can bring that environment up without hand-written Quadlet files.
- Non-root user/rootless containers should be a natural fit for local
  development.
- Packages are declared in TOML and realised by Nix, not installed ad-hoc inside
  the container.
- Explicit startup activation is available for manager-started workloads, but
  there is no implicit autostart default.

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
rootfs. The current `Rootfs=...:O` mode does not configure a persistent,
inspectable upperdir; it must not be treated as a current promote workflow. A
future explicit overlay design can become the basis for review workflows.

Goals:

- inspect changes made inside a running container
- diff overlay upperdir against the generated rootfs
- promote state/config changes back into declarative files
- never promote binaries or package-manager output
- keep packages managed by TOML + Nix rebuilds

Promote should help users capture intentional configuration/state changes, not
turn containers into mutable images.

## Security hardening

The current [Threat model and trust boundaries](threat-model.md) records what
the MVP protects and trusts. The [Capability policy](capability-policy.md)
classifies first-class intent, dangerous authority, and forbidden escape
hatches. The current implementation proves the flow, not the final isolation
model.

Current hardening applies a concrete shared baseline:

- drop all runtime-default capabilities
- no-new-privileges
- a read-only container root filesystem

`deploy.target` is required. Typed boolean opt-outs and canonical capability
additions are explicit dangerous intent implemented through
[#163](https://github.com/Patrick-Kappen/graft/issues/163). See
[Explicit container hardening](hardening.md) for the exact current boundary.

Planned hardening:

- `userns=auto`
- per-container limited UIDs
- workdir-only write access
- explicit mount and direct-device policies
- explicit network policies
- secrets support
- resource limits

System containers and user containers may need different defaults, but those
rules should be resolved by the CLI and materialised mechanically by the Nix
modules.

## Broader Quadlet coverage

The parser contains reserved roadmap concepts beyond the generated supported
schema. The current renderer covers useful basics, and later phases may promote
typed fields into the schema, resolved JSON, and Quadlet output only when their
full contract is implemented.

Qualified CDI resource references backed by host-owned specs are current through
[#203](https://github.com/Patrick-Kappen/graft/issues/203). The narrow contract
does not include direct device paths, remapping, or permissions; see
[Container Device Interface references](cdi.md).

Remaining areas include:

- additional mount types beyond basic `Volume=` entries
- Quadlet `.network` and `.volume` units
- secrets and credentials
- resources and health checks
- labels and annotations
- DNS, aliases, and network policy

Unrestricted Podman or Quadlet escape hatches are not a coverage goal. New needs
must become typed intent and cannot override keys owned by Graft.

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
