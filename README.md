# Graft

<p align="center">
  <img src="docs/assets/graft-banner.png" alt="Graft turns TOML workload intent into a Nix-store rootfs, Podman Quadlet unit, and systemd service">
</p>

<p align="center">
  <a href="https://github.com/Patrick-Kappen/graft/actions/workflows/ci.yml"><img src="https://github.com/Patrick-Kappen/graft/actions/workflows/ci.yml/badge.svg" alt="CI status"></a>
  <a href="https://app.codecov.io/gh/Patrick-Kappen/graft"><img src="https://codecov.io/gh/Patrick-Kappen/graft/branch/main/graph/badge.svg" alt="Code coverage"></a>
  <a href="https://patrick-kappen.github.io/graft/"><img src="https://img.shields.io/badge/docs-mdBook-blue" alt="Published manual"></a>
  <a href="https://github.com/Patrick-Kappen/graft/releases"><img src="https://img.shields.io/github/v/release/Patrick-Kappen/graft?include_prereleases&amp;label=release" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="License: Apache-2.0"></a>
  <img src="https://img.shields.io/badge/status-early_MVP-yellow" alt="Status: early MVP">
</p>

**Small TOML workload definitions become Nix-store rootfs containers managed by
Podman Quadlet and systemd.** Graft provides the same typed intent path for
NixOS system containers and Home Manager user containers, without requiring a
Dockerfile or hand-written Quadlet file for the current `rootfs-store` backend.

> **Early MVP:** the rootfs-store path is working and has been validated for
> system/rootful and Home Manager user-manager workloads. Podman is rootless
> only when that user manager belongs to a non-root account. Lifecycle commands,
> secure defaults, temporary agents, and multi-host control remain active
> roadmap work. Start with the [NixOS quickstart](docs/quickstart/nixos.md) or
> [Home Manager quickstart](docs/quickstart/home-manager.md).

[Published manual](https://patrick-kappen.github.io/graft/) ·
[Current roadmap](docs/roadmap.md) ·
[Long-term vision](docs/vision.md) ·
[Releases](https://github.com/Patrick-Kappen/graft/releases) ·
[Security](SECURITY.md) ·
[Contributing](CONTRIBUTING.md)

## From TOML to a service

A current, public example uses only `bash` from the host's pinned nixpkgs plus
Graft's built-in `graft-pause`:

```toml
version = 1
name = "graft-example"

[config.runtime]
packages = ["bash"]
command = ["bash", "-c", "echo graft-example-ready; exec /bin/graft-pause"]
```

Nix materialises the package closure and Graft renders the relevant Quadlet
intent:

```ini
[Container]
ContainerName=graft-example
Rootfs=/nix/store/...-graft-graft-example-env:O
Exec="bash" "-c" "echo graft-example-ready; exec /bin/graft-pause"
Volume=/nix/store:/nix/store:ro
```

After activation and an explicit manual start, it is an ordinary systemd
service and its journal contains:

```text
graft-example-ready
```

The complete examples include flake wiring, host prerequisites, Git tracking,
activation, status, logs, stop, cleanup, rendered output, and automated drift
checks:

- [NixOS system/rootful example](docs/quickstart/nixos.md)
- [Home Manager non-root user/rootless example](docs/quickstart/home-manager.md)

## Why Graft?

- **Typed TOML intent:** workload authors do not need to write per-workload Nix
  modules, raw Quadlet, or Podman command lines.
- **Nix-built rootfs:** `graft-pause` comes from the host-selected Graft
  package, while other names resolve from the target host's pinned `pkgs`; the
  running container performs no package installation.
- **Systemd-native lifecycle:** Quadlet generates normal systemd services instead
  of introducing a separate container supervisor.
- **Typed dependencies:** validated workload and explicit external-unit
  relationships cover common activation, ordering, and lifecycle coupling without
  raw `[Unit]` maps.
- **System and user scope:** one intent model targets the NixOS system manager
  or the current Home Manager account's user manager.
- **Explicit hardening:** optional capability drops, no-new-privileges, and a
  read-only container rootfs narrow upstream defaults without hidden
  relaxations.
- **Explicit host policy:** Graft generates a read-only `/nix/store` bind for
  `rootfs-store`, but explicit volumes may overlap it. Graft does not silently
  enable Podman, linger, firewall rules, accounts, user-specified host mounts,
  or privileged capabilities.
- **Minimal defaults:** no default shell, `coreutils`, restart policy, or
  autostart is hidden in the workload.

## Architecture

```text
TOML intent
  ↓
graft resolver → deterministic JSON
  ↓
NixOS / Home Manager materialisation
  ↓
Nix-store rootfs + Quadlet .container
  ↓
systemd service → Podman container
```

The CLI owns validation, semantic decisions, and defaults represented in
resolved JSON. Nix modules remain mechanical materialisers; absent
`deploy.enable` is the documented rule to render. Quadlet generates units,
systemd owns lifecycle, and
Podman runs the containers. See [Design](docs/design.md) and
[Quadlet output](docs/quadlet.md) for the contracts.

## Scope: now, next, and later

| Horizon | Status |
| --- | --- |
| **Available now** | Fail-closed TOML-to-JSON resolution; Nix-store rootfs; NixOS system and Home Manager user materialisation; explicit packages and commands; selected identity, environment, filesystem, network, secure defaults and typed relaxations, lifecycle, startup, and typed dependency fields. |
| **Active roadmap** | Typed timers; `up`/`down`, status and logs; secure rootless defaults; secrets, mounts, networking, limits, temporary instances, deterministic merging, and explicit multi-host deployment. |
| **Long-term vision** | Repository-defined environments whose components may be placed locally, on explicit remote hosts, or in temporary instances; reviewed OCI and development-environment integrations; possible TUI or optional web control surface. |

Only the first row describes current functionality. See the
[Roadmap](docs/roadmap.md) for active delivery and [Vision](docs/vision.md) for
the explicitly non-committed endgame.

## Requirements and security status

Graft currently targets Linux hosts with Nix, systemd, and Podman with Quadlet
support. NixOS handles system/rootful materialisation; Home Manager handles the
current account's user-manager materialisation. Podman is rootless only for a
non-root Home Manager account; a root-owned user manager retains root authority.
Graft does not enforce the account UID. The host remains responsible for Podman
setup, rootless prerequisites, user linger, firewall/DNS policy, accounts, and
other host configuration.

Rootless under a non-root account is the preferred direction for unattended
server workloads. System targets and root-owned user targets are rootful, and
containers share the host kernel: they are not
presented as VM-equivalent isolation. Read the current
[Threat model and trust boundaries](docs/threat-model.md) before selecting a
target or config source. Secure defaults remain active work in
[Security hardening](docs/roadmap.md#security-hardening). Report suspected
vulnerabilities privately through the [security policy](SECURITY.md), never in a
public issue.

## When Graft fits

Graft is aimed at NixOS users who want small, reviewable workload intent,
Nix-provided packages, and systemd-managed Podman services without repeating
Nix or Quadlet boilerplate for every workload.

Choose a different tool when you need full upstream Quadlet control today, a
mature interactive development environment, OCI/Compose compatibility, a
remote-workspace platform, Kubernetes scheduling, or a stronger VM isolation
boundary.

## Related approaches

| Approach | Where it is stronger or different |
| --- | --- |
| [Podman Quadlet](docs/capabilities.md#tested-upstream-context) | Direct access to the full upstream unit format; Graft intentionally exposes a smaller typed intent model and records its tested upstream version. |
| [`quadlet-nix`](https://github.com/SEIAROTg/quadlet-nix) and [the mirkolenz implementation](https://github.com/mirkolenz/quadlet-nix) | Broader direct Quadlet coverage through Nix today; Graft uses TOML intent and automatically builds its current rootfs from package names. |
| [Home Manager Podman](https://nix-community.github.io/home-manager/options.xhtml#opt-services.podman.enable) | Native Nix configuration for user-scoped Podman resources; Graft shares one TOML model across NixOS and Home Manager. |
| [compose2nix](https://github.com/aksiksi/compose2nix) and [Arion](https://github.com/hercules-ci/arion) | OCI image and Compose workflows; Graft does not currently implement an OCI backend. |
| [devenv](https://github.com/cachix/devenv), [Devbox](https://github.com/jetify-com/devbox), and [Flox](https://github.com/flox/flox) | Mature reproducible development-environment workflows; Graft is not yet a complete interactive devenv replacement. |
| [DevPod](https://github.com/loft-sh/devpod) and [Coder](https://github.com/coder/coder) | Mature remote workspaces and IDE connectivity; Graft's placement and workspace direction is long-term vision only. |
| [NixOS containers](https://nixos.org/manual/nixos/stable/#ch-containers) and [microvm.nix](https://github.com/microvm-nix/microvm.nix) | Full OS containers or stronger VM isolation; prefer a VM boundary for workloads that must not share the host kernel. |

Graft is not a fork of these projects. Its current distinction is the
TOML → Nix-store rootfs → Quadlet → systemd path; its future placement model is
recorded separately rather than advertised as implemented.

## Documentation

- **Get started:** [NixOS quickstart](docs/quickstart/nixos.md) ·
  [Home Manager quickstart](docs/quickstart/home-manager.md)
- **Understand the system:** [Overview](docs/overview.md) ·
  [Design](docs/design.md) · [Quadlet output](docs/quadlet.md) ·
  [Typed dependencies](docs/dependencies.md)
- **Configure it:** [Reference](docs/reference.md) ·
  [Explicit hardening](docs/hardening.md) ·
  [Capability status](docs/capabilities.md) ·
  [Supported JSON Schema](crates/graft/schema/graft-v1.schema.json)
- **Track direction:** [Roadmap](docs/roadmap.md) ·
  [Vision](docs/vision.md) · [Non-goals](docs/non-goals.md)
- **Security:** [Threat model and trust boundaries](docs/threat-model.md) ·
  [Security policy and private reporting](SECURITY.md)
- **Contribute:** [Contribution guide](CONTRIBUTING.md) ·
  [Development checks](docs/development.md)

## Contributing and license

Graft is early software. Bug reports, documentation corrections, design
feedback, and focused pull requests are welcome. Read the
[Contribution guide](CONTRIBUTING.md) and
[Development checks](docs/development.md) before implementing behavior changes.

Licensed under [Apache-2.0](LICENSE).
