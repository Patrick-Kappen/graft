# graft

[![CI](https://github.com/Patrick-Kappen/graft/actions/workflows/check.yml/badge.svg)](https://github.com/Patrick-Kappen/graft/actions/workflows/check.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![NixOS](https://img.shields.io/badge/NixOS-module-5277C3?logo=nixos&logoColor=white)](docs/nixos-module.md)
[![Home Manager](https://img.shields.io/badge/Home%20Manager-module-6586c8)](docs/home-manager.md)

**Declarative containers from the Nix store — no Dockerfile, no image build.**

`graft` is like [direnv](https://direnv.net/) for containers. You describe a
container in a small TOML file; `graft` realises its packages in the Nix store
and runs it as a [Podman](https://podman.io/) /
[Quadlet](https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html)
unit. There is no image to build or pull: the container's root filesystem is a
minimal rootfs plus a read-only `/nix/store` mount, and the command runs from
absolute store paths.

> **Status:** early, single-developer project (v0.1.0). The fast transient flow,
> the TOML graph, NixOS / Home Manager modules, home session isolation, shadow
> mounts, and the diff/promote/reset flow all work today. See
> [vision.md](docs/vision.md) for where things are heading.

---

## Why graft

The gap this project fills:

```text
Nix store-backed runtime closures
+ Podman/Quadlet lifecycle
+ container-only tools (no host PATH pollution)
+ a TOML graph for composition
+ fast, dynamic project containers
+ optional promotion to permanent config
```

…without requiring any of:

- a Dockerfile;
- an OCI image build or pull;
- hand-written Quadlet units;
- a NixOS rebuild for the fast, project-local path.

Packages live in `/nix/store` and on the container's `PATH` — never on the host
`PATH`.

```text
package in /nix/store        yes
package on host PATH         no
package on container PATH    yes
```

## How it works

For `rootfs-store` mode, no container image is built. Instead `graft` produces:

```text
Rootfs=<minimal-rootfs>
Volume=/nix/store:/nix/store:ro
Environment=PATH=<runtime-closure>/bin
Exec=<configured command>
```

If the store paths already exist (or come from a binary cache), startup is fast.
Otherwise Nix builds or downloads only the missing closures.

## Quick start

You need [Nix](https://nixos.org/) (with flakes) and Podman.

```bash
# Run graft straight from the flake
nix run github:Patrick-Kappen/graft -- --help

# Or build the binary
nix build github:Patrick-Kappen/graft
./result/bin/graft --help
```

Write a `graft.toml`:

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

Inspect and render it:

```bash
graft inspect graft.toml    # print resolved metadata as JSON
graft render graft.toml     # print the generated Quadlet unit
```

Or test it locally before committing (dev path):

```bash
graft run graft.toml --as hello-test
```

## Two paths

**Managed path (primary).** TOML blueprints live in your NixOS config repo and
are processed at `nixos-rebuild switch` time. The module resolves the
composition graph, renders Quadlet units into the store, and deploys them. The
CLI then operates on pre-deployed instances — it does not read TOML at runtime.

```nix
# NixOS: system-target containers -> /etc/containers/systemd
services.graft = {
  enable = true;
  configRoot = ./containers;
};
```

```nix
# Home Manager: rootless user Quadlet -> ~/.config/containers/systemd
programs.graft = {
  enable = true;
  configRoot = ./containers;
};
```

A discovered TOML file is only picked up by a module when it opts in:

```toml
[deploy]
enable = true
target = "system"   # or "user" for Home Manager
```

After `nixos-rebuild switch`:

```bash
graft up my-agent-1       # start a deployed instance
graft down my-agent-1     # stop it
graft attach my-agent-1   # attach to its tmux session
graft list                # list running containers
graft my-agent-1          # start-or-attach shortcut

# All managed-path commands also accept --host to operate on a remote machine:
graft --host server.example.com up my-agent-1
```

**Dev path (validate before committing).** `graft run --as` reads a TOML
directly, renders a transient Quadlet unit, and starts it. Use this to
validate a blueprint locally before running `nixos-rebuild`.

```bash
graft run my-agent.toml --as my-agent-test
```

`--as` is required and sets the instance name. The unit is removed when the
container stops.

## Composition: parents, children, package ops

Containers compose through a TOML graph instead of copy-paste. A child can pull
in a base and declare only the differences:

```toml
version = 1
name = "my-app"

[parents]
add = ["base/runtime", "base/locked"]

[config.runtime.packageOps]
remove = ["hello"]
add = ["gnugrep"]

[[config.runtime.packageOps.replace]]
name = "hello"
with = "hostname"
```

Resolution order is `parents -> self -> children`: attrsets merge recursively,
lists concatenate and de-duplicate, and scalars from later layers win. See
[`examples/config-root`](examples/config-root) for a complete tree.

## Commands

```text
Managed-path (instance operations — no TOML reading):
graft [--host <address>] up <instance>           Start a deployed container
graft [--host <address>] down <instance>         Stop a running container
graft [--host <address>] attach <instance>       Attach to the container's tmux session
graft [--host <address>] logs <instance> [--denied]  Show logs (--denied: egress blocks only)
graft [--host <address>] list                    List running graft-managed containers
graft [--host <address>] <instance>              Start-or-attach shortcut
graft [--host <address>] stop <instance>         Stop and remove a transient/dev unit

Dev path (reads TOML at runtime):
graft run <file.toml> --as <instance>            Render and start a transient container

Plumbing / module support:
graft inspect <file.toml>                        Print resolved metadata as JSON
graft render <file.toml>                         Render Quadlet text to stdout
graft render-nixos <file.toml> <rootfs> <name>   Render with concrete store paths
graft render-nixos-units <file.toml> <name> <out-dir>
                                                 Render all Quadlet units to a directory
graft nix-bake <dir>                             Generate a buildNpmPackage Nix snippet

Config management:
graft config path | init [path] | show [path]    Manage the config file
```

## Feature status

Working today:

- Go CLI with managed-path instance operations (`up`, `down`, `attach`, `list`,
  `logs`) and dev-path `run --as`;
- strict TOML loader (unknown fields rejected);
- no-op detection (an empty config does nothing);
- `rootfs-store` Quadlet rendering;
- Nix package build (`nix build`, `nix run`);
- NixOS and Home Manager modules with recursive `configRoot` discovery,
  `parents.*` / `children.*` graph resolution, and `packageOps`;
- runtime package strings resolved as `pkgs.<name>` closures.

Planned:

- package refs/pins beyond simple `pkgs.<name>` strings;
- a direnv-style shell hook (`enter` / `leave` / idle policy);
- worktree auto-naming for `graft run` (derive instance name from git worktree);
- remote session review/promote support.

## Documentation

- [Getting started](docs/getting-started.md) — the full walkthrough.
- [Configuration reference](docs/reference.md) — every module option and TOML
  field, plus a complete annotated [`examples/reference.toml`](examples/reference.toml).
- [Vision](docs/vision.md) — what graft is aiming to become.
- [Config notes](docs/config.md) and [CLI](docs/cli.md).
- [NixOS module](docs/nixos-module.md) · [Home Manager module](docs/home-manager.md).
- [Design](docs/design.md).
- [Security model](docs/security.md) and [security roadmap](docs/security-roadmap.md).

## A note on the name

There is an unrelated, well-known Rust project also called `graft`
([orbitinghail/graft](https://github.com/orbitinghail/graft), a storage engine).

## Contributing & security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE).
