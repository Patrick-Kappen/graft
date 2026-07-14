# Graft

<p align="center">
  <img src="docs/assets/graft-banner.png" alt="Graft turns TOML workload intent into a Nix-store rootfs, Podman Quadlet unit, and systemd service">
</p>

<p align="center">
  <a href="https://github.com/Patrick-Kappen/graft/actions/workflows/ci.yml"><img src="https://github.com/Patrick-Kappen/graft/actions/workflows/ci.yml/badge.svg" alt="Manual quality checks"></a>
  <a href="https://app.codecov.io/gh/Patrick-Kappen/graft"><img src="https://codecov.io/gh/Patrick-Kappen/graft/branch/main/graph/badge.svg" alt="Code coverage"></a>
  <a href="https://graft.kappen.io/docs/"><img src="https://img.shields.io/badge/docs-manual-blue" alt="Published manual"></a>
  <a href="https://github.com/Patrick-Kappen/graft/releases"><img src="https://img.shields.io/github/v/release/Patrick-Kappen/graft?include_prereleases&amp;label=release" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="License: Apache-2.0"></a>
  <img src="https://img.shields.io/badge/status-early_alpha-yellow" alt="Status: early alpha">
</p>

**Declare a small container workload in typed TOML. Let Nix build its rootfs,
Quadlet generate its unit, and systemd manage its lifecycle.**

Graft gives NixOS system containers and Home Manager user containers the same
validated intent model—without a Dockerfile, runtime package installation, or a
hand-written Quadlet file.

[Start with NixOS](docs/quickstart/nixos.md) ·
[Start with Home Manager](docs/quickstart/home-manager.md) ·
[Read the manual](https://graft.kappen.io/docs/) ·
[See the roadmap](docs/roadmap.md)

## Intent in, service out

A complete workload selects its authority explicitly and declares only what it
needs:

```toml
version = 1
name = "graft-example"

[deploy]
target = "system"

[config.runtime]
packages = ["bash"]
command = ["bash", "-c", "echo graft-example-ready; exec /bin/graft-pause"]
```

During a NixOS rebuild or Home Manager activation, Graft follows one typed path:

```text
TOML intent
  → validated resolved JSON
  → Nix-built rootfs
  → Quadlet .container file
  → systemd service
  → Podman container
```

After an explicit start, the example is an ordinary `graft-example.service` and
logs `graft-example-ready`. The running container installs nothing: `bash` and
Graft's built-in `graft-pause` come from the host's pinned Nix package set.

## Why Graft

- **Typed intent, not passthrough.** Graft validates supported workload concepts
  instead of accepting arbitrary Podman, Quadlet, systemd, or Nix fragments.
- **Nix-built rootfs.** Package changes are realised declaratively before the
  container starts; no image pull or in-container package manager is required.
- **Systemd-native runtime.** Quadlet produces normal services, so Graft does
  not add another supervisor.
- **One model, two scopes.** An explicit target selects the NixOS system manager
  or the current Home Manager account's user manager.
- **Secure baseline.** Workloads default to a read-only rootfs, dropped runtime
  capabilities, and no-new-privileges. Supported relaxations remain explicit.
- **No hidden lifecycle policy.** Graft adds no default autostart or restart
  policy. Typed startup, lifecycle, dependency, and network intent stays
  visible in TOML.

## Available today

The current `rootfs-store` backend supports:

- NixOS system/rootful and Home Manager user-manager materialisation;
- packages, argv commands, identity, working directory, and environment;
- long-running services, finite jobs, setup jobs, and explicit startup;
- typed workload dependencies and selected container networking;
- read-only-by-default binds, managed volumes, and bounded tmpfs mounts;
- qualified CDI resource references and typed hardening controls;
- generated schema validation and fail-closed rejection of reserved fields.

The [configuration reference](docs/reference.md) documents accepted TOML, while
[capability status](docs/capabilities.md) is the authoritative boundary between
current, planned, deferred, and forbidden behavior.

## Start with a tested path

The complete quickstarts include host prerequisites, flake wiring, tracked
examples, activation, manual startup, status, logs, cleanup, expected Quadlet
output, and drift checks:

- [NixOS system-container quickstart](docs/quickstart/nixos.md)
- [Home Manager user-container quickstart](docs/quickstart/home-manager.md)

Graft does not silently configure Podman, rootless overlay support, systemd user
linger, accounts, firewall rules, or DNS policy. A user target is rootless only
when its Home Manager account is non-root.

## Scope and security

Graft is early-alpha software. Containers share the host kernel and are not a
VM-equivalent security boundary. The current backend also exposes the complete
host `/nix/store` read-only so rootfs symlinks resolve; mandatory closure-scoped
exposure is [designed but not yet implemented](docs/closure-scoped-store.md).
Review the [threat model](docs/threat-model.md) before selecting a target or
trusting configuration input.

Graft fits small Nix-native services and development workloads that benefit from
reviewable TOML and systemd ownership. Choose direct Quadlet when full upstream
option coverage matters more than a narrow typed contract, Compose tooling for
OCI/Compose workflows, or a VM boundary when workloads must not share the host
kernel. See [non-goals and deferred scope](docs/non-goals.md) for the deliberate
product boundary.

## Project

- [Manual](https://graft.kappen.io/docs/)
- [Roadmap](docs/roadmap.md) and [long-term vision](docs/vision.md)
- [Security policy](SECURITY.md) and [private reporting](https://github.com/Patrick-Kappen/graft/security)
- [Contribution guide](CONTRIBUTING.md) and [development checks](docs/development.md)
- [Apache-2.0 license](LICENSE)
