# Getting started

This guide walks you from zero to a running `graft` container — through the
managed path (TOML in git → nixos-rebuild → systemd), the dev path for local
testing, composition, and how to harden containers.

If you only want the elevator pitch, read the [vision](vision.md). For the
complete reference — every Nix module option and TOML field — see
[reference.md](reference.md) and the annotated
[`examples/reference.toml`](../examples/reference.toml).

## Prerequisites

- [Nix](https://nixos.org/) with flakes enabled.
- [Podman](https://podman.io/) with Quadlet (Podman 4.4+).

You do **not** need Docker, a registry login, or any image build tooling.

## Install

Run graft straight from the flake without installing anything:

```bash
nix run github:zerodawn1990/graft -- --help
```

Build the binary into `./result/bin/graft`:

```bash
nix build github:zerodawn1990/graft
./result/bin/graft --version   # 0.1.0
```

Or drop into a dev shell with graft and its toolchain on `PATH`:

```bash
nix develop github:zerodawn1990/graft
```

The rest of this guide assumes `graft` is on your `PATH`.

## How graft works

graft blueprints are TOML files that live in your git repository alongside your
NixOS configuration. They are processed at **build time** — never at runtime:

```
TOML blueprints in git
        │
        ▼  nixos-rebuild switch / home-manager switch
Quadlet units in the Nix store
        │
        ▼  systemd daemon-reload (automatic)
systemd service units deployed on the host
        │
        ▼  graft up / down / attach …
Running containers (Podman / Quadlet)
```

The CLI (`graft`) is a **control interface** for those pre-deployed units. It
wraps `systemctl` and `podman`. It does not read TOML files at runtime on the
managed path.

For local validation before committing and rebuilding, use `graft run --as`
(the dev path — described in [Two paths](#two-paths) below).

## Your first blueprint

Create `containers/hello.toml` in your NixOS config repo:

```toml
version = 1
name = "hello"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils"]
command = ["bash", "-lc", "echo hello from graft"]

[config.network]
mode = "none"

[deploy]
enable = true
target = "system"
```

Add it to your NixOS configuration:

```nix
services.graft = {
  enable = true;
  configRoot = ./containers;
};
```

Then rebuild:

```bash
sudo nixos-rebuild switch
```

graft renders a Quadlet unit into the store and deploys it to
`/etc/containers/systemd/`. Now control it with the CLI:

```bash
graft up hello-1      # start
graft logs hello-1    # view logs
graft down hello-1    # stop
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
| `version`, `name`    | Schema version and the blueprint ID.                   |
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

## Two paths

### Managed path (primary)

TOML blueprints live in your NixOS config repo. The module discovers them at
`nixos-rebuild switch` time, resolves the composition graph, renders Quadlet
units into the store, and deploys them to `/etc/containers/systemd/`.

```bash
# Deploy
sudo nixos-rebuild switch

# Operate
graft up hello-1       # start a deployed instance
graft down hello-1     # stop it
graft attach hello-1   # attach to its tmux session
graft list             # list running graft containers
graft logs hello-1     # view logs
graft hello-1          # start-or-attach shortcut
```

The CLI wraps `systemctl` (NixOS system) or `systemctl --user` (Home Manager).
It does not read TOML files at runtime.

### Dev path (validate before committing)

Use `graft run --as` to test a blueprint locally without running
`nixos-rebuild`. This is the only command that reads a TOML file at runtime.

```bash
graft run containers/hello.toml --as hello-test
```

`--as` is required: it sets the instance name. The unit is written to
`$XDG_RUNTIME_DIR/containers/systemd` and removed when the container stops.

**Future — worktree auto-naming:** when graft detects a git worktree, the
instance name will be derived automatically from the worktree name:

```bash
# in worktree 'feature-x' (future):
graft run containers/hello.toml              # → hello-feature-x  (auto)
graft run containers/hello.toml --as debug   # → hello-debug       (override)
```

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
tmpfs = ["/tmp", "/run", "/home/user"]

[config.network]
mode = "none"

[config.security]
dropCapabilities = ["all"]
noNewPrivileges = true
userns = "keep-id"
```

The first versions mount all of `/nix/store` read-only for speed; a
`closure-only` store-access mode is planned. See [security.md](security.md).

## What's next

See [vision.md](vision.md) for the longer-term direction. For the full TOML
field reference, see [reference.md](reference.md) and
[`examples/reference.toml`](../examples/reference.toml).
