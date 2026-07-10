# Graft

<p align="center">
  <img src="assets/graft-banner.png" alt="Graft turns TOML workload intent into a Nix-store rootfs, Podman Quadlet unit, and systemd service">
</p>

**Graft turns small TOML workload definitions into Nix-store rootfs containers
managed by Podman Quadlet and systemd.** The same typed intent path supports
NixOS system containers and Home Manager user containers.

> **Early MVP:** the current `rootfs-store` path works for system/rootful and
> user/rootless workloads. Lifecycle commands, broader security policy,
> temporary agents, and multi-host control remain roadmap work.

## Start here

Choose the scope that owns the generated systemd service:

- [NixOS system-container quickstart](quickstart/nixos.md) — system manager,
  rootful Podman;
- [Home Manager user-container quickstart](quickstart/home-manager.md) — user
  manager, rootless Podman.

Both paths include flake wiring, host prerequisites, a public package-only TOML
workload, Git tracking, activation, status, logs, stop, cleanup, expected
Quadlet output, and automated drift validation.

## What the current path does

A workload declares package and command intent:

```toml
version = 1
name = "graft-example"

[config.runtime]
packages = ["bash"]
command = ["bash", "-c", "echo graft-example-ready; exec /bin/graft-pause"]
```

Graft resolves that intent and Nix materialises it:

```text
TOML
  → deterministic resolved JSON
  → Nix-store rootfs
  → Quadlet .container
  → systemd service
  → Podman container
```

The generated service uses a Nix store rootfs and logs:

```text
graft-example-ready
```

The [Overview](overview.md), [Design](design.md), and
[Quadlet output](quadlet.md) chapters explain each boundary.

## Host requirements

The current path requires Linux, Nix, systemd, and Podman with Quadlet support.
Graft materialises workload output; it does not silently enable Podman, rootless
overlay support, user linger, firewall/DNS policy, accounts, or other host
configuration.

NixOS owns system/rootful materialisation. Home Manager owns user/rootless
materialisation. Rootless is the preferred direction for unattended server
workloads, but containers still share the host kernel and are not a
VM-equivalent isolation boundary.

## Current, planned, and vision

| Horizon | Status |
| --- | --- |
| **Available now** | Rootfs-store materialisation for NixOS and Home Manager, selected typed container fields, and manual systemd lifecycle. |
| **Active roadmap** | Contract hardening, services/jobs/timers, lifecycle CLI, secure rootless policy, temporary instances, deterministic merging, and explicit multi-host deployment. |
| **Long-term vision** | Portable repository environments with deliberate local, remote, or temporary placement and possible additional artifact/control integrations. |

Only **Available now** is implemented. Read [Roadmap](roadmap.md) for active
delivery and [Long-term vision](vision.md) for future direction that has no
promised syntax or schedule.

## Use the manual

- **Configure workloads:** [Reference](reference.md) and the
  [annotated TOML source](https://github.com/Patrick-Kappen/graft/blob/main/examples/reference.toml)
- **Understand output:** [Overview](overview.md), [Design](design.md), and
  [Quadlet output](quadlet.md)
- **Understand boundaries:** [Non-goals and deferred scope](non-goals.md)
- **Track direction:** [Roadmap](roadmap.md) and [Long-term vision](vision.md)
- **Contribute:** [Repository contribution entry point](https://github.com/Patrick-Kappen/graft/contribute)
  and [Development](development.md)
- **Security:** open the [Repository security page](https://github.com/Patrick-Kappen/graft/security)
  and choose **Report a vulnerability** for private reporting

Security hardening and the final threat model remain active work. See
[Security hardening](roadmap.md#security-hardening) and
[issue #127](https://github.com/Patrick-Kappen/graft/issues/127) before treating
an alpha workload as a strong isolation boundary. Never disclose a suspected
vulnerability or secret in a public issue.
