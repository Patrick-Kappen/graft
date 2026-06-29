# graft

**Declarative containers from the Nix store — no Dockerfile, no image build.**

`graft` is like [direnv](https://direnv.net/) for containers. You describe a
container in a small TOML file; `graft` realises its packages in the Nix store
and runs it as a [Podman](https://podman.io/) /
[Quadlet](https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html)
unit. There is no image to build or pull: the container's root filesystem is a
minimal rootfs plus a read-only `/nix/store` mount, and the command runs from
absolute store paths.

> **Status:** early, single-developer project (v0.1.0). The fast transient flow,
> the TOML graph, and the NixOS / Home Manager modules work today. Session
> lifecycle, the workspace candidate flow, and the promote-to-PR flow are
> planned — see [the roadmap](docs/roadmap.md).

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
nix run github:zerodawn1990/graft -- --help

# Or build the binary
nix build github:zerodawn1990/graft
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

Inspect, render, and run it:

```bash
graft inspect graft.toml    # print resolved metadata as JSON
graft render graft.toml     # print the generated Quadlet unit
graft up graft.toml         # run it via a transient user Quadlet unit
```

`graft up` with no argument autodetects a config in the current directory, in
this order: `graft.toml`, `.graft.toml`, `config.toml`.

## Two routes

**Fast project flow — no NixOS rebuild.** A transient Quadlet unit is written to
`$XDG_RUNTIME_DIR/containers/systemd` and started with `systemctl --user`. Ideal
for experimenting and per-project containers.

```bash
graft up
```

**Permanent / managed flow.** Once a container earns its keep, promote it into a
TOML file managed by the NixOS or Home Manager module, reviewed through a normal
branch / PR.

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
graft config path | init [path] | show [path]   Manage the no-op example config
graft up [file.toml]                             Run a config (autodetect if omitted)
graft inspect <file.toml>                        Print resolved metadata as JSON
graft render <file.toml>                         Render Quadlet text
graft render-nixos <file.toml> <rootfs> <name>   Render with concrete store paths
graft render-nixos-units <file.toml> <name> <out-dir>
graft run <file.toml>                            Run via a temporary Quadlet unit
graft run-rootfs -- <command> [args...]          Run a command in a temporary rootfs unit
```

## Feature status

Working today:

- Go CLI with the commands above;
- strict TOML loader (unknown fields rejected);
- no-op detection (an empty config does nothing);
- `rootfs-store` Quadlet rendering and transient `systemctl --user` runs;
- Nix package build (`nix build`, `nix run`);
- NixOS and Home Manager modules with recursive `configRoot` discovery,
  `parents.*` / `children.*` graph resolution, and `packageOps`;
- runtime package strings resolved as `pkgs.<name>` closures.

Planned (see [the roadmap](docs/roadmap.md)):

- package refs/pins beyond simple `pkgs.<name>` strings;
- session state and a direnv-style shell hook (`enter` / `leave` / idle policy);
- the workspace copy / jj-candidate flow for safe agent mutations;
- the promote-to-branch/PR flow;
- persistent user Quadlet mode.

## Documentation

- [Getting started](docs/getting-started.md) — the full walkthrough.
- [Configuration reference](docs/reference.md) — every module option and TOML
  field, plus a complete annotated [`examples/reference.toml`](examples/reference.toml).
- [Vision](docs/vision.md) — what graft is aiming to become.
- [Config notes](docs/config.md) and [CLI](docs/cli.md).
- [Minimal container](docs/minimal-container.md) — the rootfs-store slice.
- [NixOS module](docs/nixos-module.md) · [Home Manager module](docs/home-manager.md).
- [Design](docs/design.md) · [Graph and runtime](docs/graph-and-runtime.md) ·
  [Runtime architecture](docs/runtime-architecture.md).
- [Security model](docs/security.md) and [security roadmap](docs/security-roadmap.md).

## A note on the name

There is an unrelated, well-known Rust project also called `graft`
([orbitinghail/graft](https://github.com/orbitinghail/graft), a storage engine).

## Contributing & security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE).
