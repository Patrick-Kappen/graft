# Graft manual

Graft turns typed TOML workload intent into Nix-store rootfs containers managed
by Podman Quadlet and systemd. This manual documents the current alpha contract,
its security boundaries, and the difference between implemented behavior and
future direction.

## Choose a quickstart

Start with the manager that should own the generated service:

- [NixOS quickstart](quickstart/nixos.md) — a system-managed, rootful Podman
  container;
- [Home Manager quickstart](quickstart/home-manager.md) — a user-managed
  container that is rootless when the Home Manager account is non-root.

Both quickstarts use tracked, schema-validated examples and cover prerequisites,
flake wiring, activation, manual startup, inspection, and removal.

## What Graft does today

The current `rootfs-store` path is:

```text
TOML intent
  → validated resolved JSON
  → NixOS or Home Manager materialisation
  → Nix-store rootfs + Quadlet .container file
  → systemd service
  → Podman container
```

The implemented contract includes:

- explicit `system` or `user` targets;
- Nix-provided packages and argv commands;
- typed lifecycle, startup, dependency, network, filesystem, CDI, and selected
  container settings;
- a read-only rootfs, dropped capabilities, and no-new-privileges by default;
- explicit typed relaxations where the current contract permits them;
- no implicit autostart or hidden restart policy.

Graft currently exposes the complete host `/nix/store` read-only so rootfs
symlinks resolve. Replacing that mount with mandatory per-workload closure
exposure is an approved design, not yet implemented. Containers share the host
kernel and are not a VM-equivalent isolation boundary.

## Find the right chapter

### Configure and operate a workload

Use the [Configuration reference](reference.md) for accepted TOML. The chapters
on [lifecycle](lifecycle.md), [startup](activation.md),
[dependencies](dependencies.md), [networking](networking.md),
[filesystems](filesystem-policy.md), [CDI](cdi.md), and
[hardening](hardening.md) explain the corresponding typed contracts.

### Understand the pipeline

Read the [Overview](overview.md) for the conceptual flow, then
[Architecture and responsibilities](design.md) for layer ownership and
[Generated Quadlet output](quadlet.md) for materialisation details.

### Evaluate security and availability

The [Threat model](threat-model.md) states current guarantees, trust boundaries,
and residual risks. [Capability policy](capability-policy.md) classifies
first-class, dangerous, and forbidden authority. [Capability status](capabilities.md)
is the authoritative current/planned/deferred matrix.

### Understand project direction

[Roadmap](roadmap.md) describes active delivery. [Long-term vision](vision.md)
is non-committed direction. [Non-goals](non-goals.md) records deliberate current
exclusions. The [closure-scoped store design](closure-scoped-store.md) is an
approved future implementation contract and is labelled accordingly.

## Host responsibility

Graft materialises workload output. It does not silently configure Podman,
rootless overlay support, systemd user linger, accounts, firewall or DNS policy,
or other host prerequisites. Review the relevant quickstart and threat model
before selecting a target or trusting a configuration source.

For private vulnerability reporting, use the repository's
[security page](https://github.com/Patrick-Kappen/graft/security) and select
**Report a vulnerability**. Contributors should start with
[Development](development.md) and the repository
[contribution guide](https://github.com/Patrick-Kappen/graft/blob/main/CONTRIBUTING.md).
