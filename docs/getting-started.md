# Getting started

This guide walks you from zero to a running `graft` container, then through
composition, the two run routes, and promoting a container to a NixOS or Home
Manager module.

If you only want the elevator pitch, read the [vision](vision.md). For the
complete reference — every Nix module option and TOML field — see
[reference.md](reference.md) and the annotated
[`examples/reference.toml`](../examples/reference.toml).

## Prerequisites

- [Nix](https://nixos.org/) with flakes enabled.
- [Podman](https://podman.io/) with Quadlet (Podman 4.4+). For the transient
  flow you also need a working rootless user systemd session.

You do **not** need Docker, a registry login, or any image build tooling.

## Install

Run graft straight from the flake without installing anything:

```bash
nix run github:Patrick-Kappen/graft -- --help
```

Build the binary into `./result/bin/graft`:

```bash
nix build github:Patrick-Kappen/graft
./result/bin/graft --version   # 0.1.0
```

Or drop into a dev shell with graft and its toolchain on `PATH`:

```bash
nix develop github:Patrick-Kappen/graft
```

The rest of this guide assumes `graft` is on your `PATH`.

## Your first container

Create `graft.toml` in a project directory:

```toml
version = 1
name = "hello"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils"]
command = ["bash", "-lc", "echo hello from graft"]

[config.network]
mode = "none"
```

Inspect what graft resolves from it:

```bash
graft inspect graft.toml
```

This prints JSON metadata — the resolved name, runtime mode, packages, and
command — without touching Podman.

See the Quadlet unit graft would generate:

```bash
graft render graft.toml
```

Run it through a transient user Quadlet unit:

```bash
graft up graft.toml
```

`graft up` with no argument autodetects a config in the current directory, in
this order:

```text
graft.toml
.graft.toml
config.toml
```

## How rootfs-store mode works

`mode = "rootfs-store"` is the fast path: **no image is built**. graft renders a
Quadlet unit shaped like this:

```text
Rootfs=<minimal-rootfs>
Volume=/nix/store:/nix/store:ro
Environment=PATH=<runtime-closure>/bin
Exec=<your command>
```

The packages you list are realised as Nix store closures and exposed on the
container's `PATH`. They are never installed on the host `PATH`. If the store
paths already exist or come from a binary cache, startup is fast; otherwise Nix
builds or downloads only the missing closures.

The empty config is meaningful: **empty means no-op.** A config with no runtime,
mounts, network, or packages installs nothing and starts nothing.

```toml
version = 1
name = "example"

[config]
# no-op
```

## Anatomy of a config

A fuller config showing the common sections (see the shipped
[`examples/rootfs-store.toml`](../examples/rootfs-store.toml)):

```toml
version = 1
name = "graft"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils"]
command = ["bash", "-lc", "echo hello from graft"]

[config.container]
workingDir = "/workspace"

[config.container.environment]
GRAFT_EXAMPLE = "1"

[config.filesystem]
readOnly = true
readOnlyTmpfs = true
tmpfs = ["/tmp", "/run", "/workspace"]

[[config.filesystem.volumes]]
source = "/nix/store"
target = "/nix/store"
mode = "ro"

[config.network]
mode = "none"

[config.security]
dropCapabilities = ["all"]
noNewPrivileges = true
userns = "keep-id"

[config.resources]
memory = "1g"
pidsLimit = 512
```

| Section              | Purpose                                                |
| -------------------- | ------------------------------------------------------ |
| `version`, `name`    | Schema version and the container/unit name.            |
| `config.runtime`     | Mode, packages, command, and `packageOps`.             |
| `config.container`   | Working dir and environment.                           |
| `config.filesystem`  | Read-only root, tmpfs, volumes, devices.               |
| `config.network`     | Network mode and DNS/hosts/ports.                      |
| `config.networks`    | Extra Quadlet `.network` units to render.              |
| `config.security`    | Capabilities, seccomp, no-new-privileges, userns.      |
| `config.resources`   | Memory, PIDs, ulimits.                                  |
| `config.service`     | Selected systemd `[Service]` options.                  |
| `deploy`             | Opt-in for the NixOS / Home Manager modules.           |

The loader is strict: unknown fields are rejected, so typos fail fast.

## Composition: parents, children, package ops

Containers compose through a TOML graph instead of copy-paste. The shipped
[`examples/config-root`](../examples/config-root) tree shows the pattern.

A runtime base:

```toml
# base/runtime.toml
version = 1
name = "base/runtime"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils", "hello"]
command = ["hello"]
```

A reusable hardening base:

```toml
# base/locked.toml
version = 1
name = "base/locked"

[config.filesystem]
readOnly = true
readOnlyTmpfs = true
tmpfs = ["/tmp", "/run"]

[config.network]
mode = "none"

[config.security]
dropCapabilities = ["all"]
noNewPrivileges = true
userns = "keep-id"
```

An addon that adjusts packages and the command:

```toml
# addons/hostname.toml
version = 1
name = "addons/hostname"

[config.runtime.packageOps]
remove = ["hello"]
add = ["gnugrep"]

[[config.runtime.packageOps.replace]]
name = "hello"
with = "hostname"

[config.runtime]
command = ["hostname"]
```

The app wires them together and opts in to deployment:

```toml
# apps/demo.toml
version = 1
name = "graft-demo"

[parents]
add = ["base/runtime", "base/locked"]

[children]
add = ["addons/hostname"]

[deploy]
enable = true
target = "system"
```

### Resolution rules

- Order is `parents -> self -> children`.
- Attrsets merge recursively.
- Lists concatenate, then de-duplicate.
- Scalars and `config.runtime.command` from later layers override earlier ones.
- `parents.set` / `children.set` replace the local refs of that node;
  `parents.remove` / `children.remove` drop refs after `set`/`add`.

### Package ops

After the graph merges, `packageOps` are applied to `config.runtime.packages`:

```toml
[config.runtime]
packages = ["bashInteractive", "coreutils", "hello"]

[config.runtime.packageOps]
remove = ["coreutils"]
add = ["gnugrep"]

[[config.runtime.packageOps.replace]]
name = "hello"
with = "hostname"
```

resolves to:

```toml
packages = ["bashInteractive", "hostname", "gnugrep"]
```

Order: remove `remove` + replacement names, add replacement `with` packages, add
`add` packages, then de-duplicate.

## The two routes

### Fast project flow (no NixOS rebuild)

```bash
graft up
```

graft writes a transient Quadlet unit to
`$XDG_RUNTIME_DIR/containers/systemd` and starts it with `systemctl --user`.
Nothing is persisted to system config. This is the route for experiments and
per-project containers.

### Permanent / managed flow

When a container is worth keeping, move its TOML into a directory managed by the
NixOS or Home Manager module and review the change through a normal branch / PR.
The same TOML works in both routes.

## NixOS module

Point the module at a directory of TOML files:

```nix
{
  services.graft = {
    enable = true;
    configRoot = ./containers;
  };
}
```

The module discovers `*.toml` recursively, resolves the graph, applies
`packageOps`, generates an effective TOML in the store, and renders Quadlet to:

```text
/etc/containers/systemd/<name>.container
```

A discovered file is only managed when it opts in with a system target:

```toml
[deploy]
enable = true
target = "system"
```

Explicit files are also supported:

```nix
services.graft = {
  enable = true;
  configFiles = [ ./containers/go-dev.toml ];
};
```

See [nixos-module.md](nixos-module.md) for the full behaviour.

## Home Manager module

The Home Manager route writes rootless/user Quadlet files:

```nix
{
  imports = [ inputs.graft.homeManagerModules.default ];

  programs.graft = {
    enable = true;
    configRoot = ./containers;
  };
}
```

It uses the same resolver and writes to:

```text
~/.config/containers/systemd/<name>.container
```

It only picks up TOML with a user target:

```toml
[deploy]
enable = true
target = "user"
```

Then reload your user units:

```bash
systemctl --user daemon-reload
systemctl --user start <name>.service
```

See [home-manager.md](home-manager.md) for details.

## Hardening

graft ships no hidden policy. To lock a container down, set the fields
explicitly — or inherit a hardened base like `base/locked.toml` above:

```toml
[config.filesystem]
readOnly = true
readOnlyTmpfs = true
tmpfs = ["/tmp", "/run", "/home/agent"]

[config.network]
mode = "none"

[config.security]
dropCapabilities = ["all"]
noNewPrivileges = true
userns = "keep-id"
```

The first versions mount all of `/nix/store` read-only for speed; a
`closure-only` store-access mode is on the roadmap. See [security.md](security.md)
and [network-proxy-security.md](network-proxy-security.md).

## What's next

The session lifecycle (a direnv-style shell hook, idle/leave policy), the
workspace candidate flow for safe agent mutations, and the promote-to-PR flow
are planned. Track progress in [the roadmap](roadmap.md).
