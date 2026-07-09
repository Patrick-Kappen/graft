# Long-term vision

Graft's endgame is one portable, repository-defined environment whose components
can run locally, on explicitly selected remote hosts, or as temporary instances.
A developer or operator should describe the environment once, then place each
workload where it belongs without abandoning Nix reproducibility, typed policy,
or systemd-managed lifecycle.

This is direction, not a promise of syntax, commands, APIs, or delivery dates.
The working product today is the `rootfs-store` path described in
[Overview](overview.md). The foundation work remains the priority in
[Roadmap](roadmap.md) and the server-workload and temporary-agent tracker
[#162](https://github.com/Patrick-Kappen/graft/issues/162).

## Product thesis

A repository may need more than one runtime location:

```text
repository intent
  ├── local developer tools and services
  ├── explicitly selected remote services
  └── temporary, isolated work or agent instances
```

Graft should eventually resolve that intent into an inspectable workload graph.
It should let users choose placement deliberately, reuse immutable artifacts,
and operate all managed runtime instances through one consistent control
contract.

Graft is not trying to replace every tool in this space. Existing environment,
Dev Container, image, Kubernetes, and VM tools may remain useful integrations
or better choices for particular workloads. The intended value is a Nix-native
way to connect reproducible repository intent with typed placement and
systemd-managed Podman workloads.

## Conceptual model

The following concepts must remain separate. Keeping them separate prevents a
runtime choice from silently becoming a security, deployment, or product-policy
choice.

| Concept | Question it answers | Current state |
| --- | --- | --- |
| Workload intent | What should run and what does it need? | TOML resolves a rootfs-store container intent. |
| Artifact backend | How is that workload materialised? | Nix-store rootfs only. |
| Placement | Where should an instance run? | Local NixOS system or Home Manager user target; later explicit remote deployment. |
| Lifecycle | Is it a service, finite job, scheduled job, or temporary instance? | Basic service materialisation; broader lifecycle work is planned. |
| Runtime authority | Which system owns runtime state? | Quadlet generates units, systemd manages lifecycle, and Podman runs containers. |
| Control surface | How do people and automation inspect or request operations? | Build-time CLI resolver today; runtime CLI work is planned. |

A later workload graph may connect several components, but it must preserve
explicit dependencies, placement, ownership, policy, and provenance for each
component.

## Current foundation

Graft currently supports one deliberately narrow backend:

```text
TOML → CLI → resolved JSON → Nix rootfs → Quadlet → systemd → Podman
```

That path builds a Nix-store rootfs from declared packages and runs it through
Podman Quadlet on NixOS or Home Manager. It does not currently provide OCI
artifacts, cross-host workload graphs, interactive workspaces, a TUI, or a web
controller.

The implementation priority is to make this foundation safe and complete:

- fail-closed configuration and tested executable boundaries;
- typed long-running service, finite-job, timer, and lifecycle behavior;
- local `up`, `down`, status, logs, inspect, lint, and doctor contracts;
- secure rootless server operation, secrets, mounts, networking, and limits;
- temporary instances with ownership, cleanup, prebuilds, and fast starts;
- deterministic multi-source configuration, explicit multi-host deployment, and
  reviewable diff/promote workflows.

These deliverables are tracked in [Roadmap](roadmap.md), especially
[#126](https://github.com/Patrick-Kappen/graft/issues/126) and
[#162](https://github.com/Patrick-Kappen/graft/issues/162). They are not
superseded by this vision.

## Later directions

The following directions need separate design work after the foundation proves
its contracts. They are not available features and this document does not define
their configuration syntax.

### More artifact backends

`rootfs-store` is the current artifact backend. OCI images, existing OCI
artifacts, builds, Kubernetes YAML, and experimental Quadlet resources require
the deliberate scope decision in
[#150](https://github.com/Patrick-Kappen/graft/issues/150). Supporting OCI must
preserve reproducibility, provenance, caching, security policy, and the same
clear lifecycle ownership; it is not a reason to accept unrestricted raw
Podman configuration.

### Portable development environments

A repository-defined environment could later combine developer tooling,
workspaces, long-running services, and temporary agents. Integrations with
existing environment or Dev Container tooling are possible future design areas,
not a commitment to replace them. Interactive workspace access, IDE attachment,
port forwarding, checkout/copy behavior, and repository-aware overrides need
their own contracts; see
[#27](https://github.com/Patrick-Kappen/graft/issues/27),
[#12](https://github.com/Patrick-Kappen/graft/issues/12), and
[#159](https://github.com/Patrick-Kappen/graft/issues/159).

### Explicitly distributed environments

Current multi-host work means building, deploying, and controlling independent
workloads on explicit hosts. A later graph may deliberately place different
components locally, remotely, or in temporary instances. That is not automatic
scheduling. It requires separate decisions for connectivity, identity, secrets,
workspace data, readiness, failure handling, architecture, and policy. The
first remote lifecycle design remains
[#161](https://github.com/Patrick-Kappen/graft/issues/161).

### Shared control surfaces

The CLI is the first control surface. A future TUI or authenticated web
controller may consume the same inspectable control contract for plans, status,
logs, and approved operations. Such a controller is optional: it must not be a
required daemon on every host or a second scheduler that competes with systemd.

## Invariants

Future work must keep these boundaries intact:

- **Typed intent:** TOML expresses user intent, not unrestricted raw Nix,
  Quadlet, systemd, or Podman arguments.
- **Deterministic provenance:** resolution is deterministic and can show the
  source and policy layer responsible for a value.
- **Host authority:** host and security policy constrain repository intent;
  a repository cannot silently enable privileged host behavior.
- **Layer ownership:** the CLI resolves policy and defaults; Nix materialises;
  Quadlet generates units; systemd owns lifecycle; Podman runs containers.
- **No mandatory Graft daemon:** Graft does not require a per-host daemon or
  duplicate scheduler. Optional clients or controllers use the same explicit
  contract.
- **Immutable artifacts:** intentional changes return to reviewed declarative
  sources and rebuild into new artifacts. Graft must not copy arbitrary runtime
  binaries or package-manager output into the Nix store.
- **Explicit placement and capability:** location, privilege, network access,
  persistence, and lifetime remain reviewable choices rather than hidden
  defaults.
- **Honest isolation:** rootless containers are the preferred server path, but
  containers are not presented as equivalent to VM isolation.

## Terms that remain deliberately narrow

Some current phrases describe the implemented backend, not permanent limits:

- **No images** means `rootfs-store` currently uses `Rootfs=`, not `Image=`.
  It does not permanently rule out a reviewed OCI backend.
- **Everything is a service** means managed runtime instances use systemd as
  lifecycle authority. It does not require every future local development tool
  to be a service.
- **No `graft shell` command** remains the current command decision. It does not
  decide the later interactive-workspace access contract.
- **No required daemon** does not rule out an optional authenticated controller
  for a future web or TUI experience.

## Boundaries of this vision

This vision does not define TOML fields, commands, protocols, databases, a
scheduler, automatic placement, non-Nix host support, Dev Container
compatibility, or a mandatory central control plane. Those require separate
issues, threat modeling, compatibility evidence, and implementation work.
