# Graft overview

Graft materialises typed TOML workloads as Podman Quadlet containers backed by a
Nix-store rootfs. Workload authors describe supported intent; Graft and Nix own
the translation into runtime artefacts.

The current flagship backend is `rootfs-store`. It performs no image pull and no
package installation inside the running container. Other artifact backends are
not part of the current contract.

## From intent to service

```text
Edit TOML
  ↓
NixOS rebuild or Home Manager activation
  ↓
Graft resolves the selected TOML set to deterministic JSON
  ↓
Nix builds a rootfs and renders a Quadlet .container file
  ↓
systemd discovers the generated service
  ↓
Podman starts the container when explicitly requested
```

Graft does not autostart a workload merely because its TOML exists. Optional
`deploy.activation = "startup"` requests a fixed manager-start relationship;
otherwise the generated service waits for an explicit start or another unit.

## Typed workload intent

A minimal workload chooses its authority explicitly:

```toml
version = 1
name = "node-dev"

[deploy]
target = "user"

[config.runtime]
packages = ["nodejs"]
```

TOML does not contain rootfs assembly, store-mount boilerplate, overlay flags,
raw Quadlet sections, or Nix expressions. The generated schema accepts only
implemented fields. Unknown input and explicitly configured reserved fields fail
closed instead of disappearing.

The filename stem currently selects the generated source-unit and service name,
while top-level `name` selects `ContainerName=`. Keep them equal until the final
identity contract in [#107](https://github.com/Patrick-Kappen/graft/issues/107).

## Two explicit authority scopes

| Target | Materialiser | Service manager | Podman authority |
| --- | --- | --- | --- |
| `system` | NixOS | system manager | rootful |
| `user` | Home Manager | current account's user manager | rootless only for a non-root account |

A user target owned by root remains rootful. Graft does not infer or enforce the
account UID. Host configuration remains responsible for Podman, rootless overlay
support, user linger, accounts, firewall and DNS policy, and other prerequisites.

## Rootfs and package model

The resolver always includes Graft's small `graft-pause` package. Other package
names resolve from the target host's pinned `pkgs`, and Nix builds the resulting
rootfs before activation. An implicit or long-running workload without a command
uses `/bin/graft-pause`; finite `job` and `setup` workloads require an explicit
command.

Graft uses Quadlet `Rootfs=<store-path>:O`, not `Image=`. The overlay itself is
writable, but the current secure baseline renders `ReadOnly=true`, so rootfs
paths are read-only unless the user explicitly opts out. Typed tmpfs, binds,
managed volumes, or trusted CDI edits can create separate writable mounts.

The renderer exposes only the realised rootfs runtime closure through a
read-only `/nix/store` scaffold followed by one read-only member mount per store
path. Typed mount targets cannot overlap that protected tree, though a bind can
expose a selected store source elsewhere and trusted CDI may inject mounts. A
closure error fails materialisation; there is no complete-store fallback. See
the [closure-scoped store contract](closure-scoped-store.md).

## Runtime and security model

Every workload is a normal Quadlet-generated systemd service. Typed lifecycle
intent distinguishes long-running services, repeatable finite jobs, and retained
setup jobs. Dependencies, startup, networking, filesystems, CDI references, and
selected process settings each use dedicated validated contracts rather than raw
systemd or Podman passthrough.

Every resolved workload receives the shared baseline:

```text
read-only rootfs
all runtime-default capabilities dropped
no-new-privileges enabled
```

Supported relaxations are explicit in TOML and remain visible in resolved JSON
and generated Quadlet. Containers still share the host kernel and are not a
VM-equivalent isolation boundary. Config roots, host bind sources, environment
files, external units, named volumes, and CDI specifications cross distinct
trust boundaries documented in the [Threat model](threat-model.md).

## Layer responsibilities

- **TOML** expresses reviewed workload intent.
- **Graft CLI** validates, resolves references, applies semantic defaults, and
  emits deterministic JSON.
- **NixOS and Home Manager modules** mechanically build rootfs paths and render
  the selected manager's Quadlet files.
- **Quadlet** translates source units into systemd services.
- **systemd** owns activation and lifecycle.
- **Podman** creates and runs the containers.

See [Architecture and responsibilities](design.md) for the internal contracts
and [Generated Quadlet output](quadlet.md) for exact materialisation behavior.
Use the [Configuration reference](reference.md) for accepted TOML and
[Capability status](capabilities.md) for the authoritative availability matrix.
