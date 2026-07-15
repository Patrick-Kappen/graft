# Control-plane architecture

> **Status:** approved product architecture for future implementation. The
> current release does not include a worker, runtime CLI commands, a TUI, or a
> controller. Detailed lifecycle, observability, protocol, and Nix contracts
> remain in their linked design issues.

Graft's control plane will make already materialised workloads locally
manageable and observable without replacing TOML, Nix, systemd, or Podman as
their respective authorities. Every managed machine will contain the Graft CLI,
TUI, and worker binary. A central controller is optional.

The architecture separates four questions:

| Question | Authority |
| --- | --- |
| What workload behavior is intended? | Reviewed TOML resolved by Graft |
| What may exist on this host? | NixOS or Home Manager configuration and materialisation |
| What is running now? | systemd and Podman |
| Who may observe or request an operation? | Worker API authorization and host policy |

The worker coordinates typed operations across these authorities. It does not
become another declarative source or runtime scheduler.

## Component topology

Each managed host installs the same client and worker package set through Nix:

```text
normal local user
  ├── graft CLI ─┐
  └── graft TUI ─┼── typed local API
                 │
        ┌────────┴────────┐
        │                 │
  user worker       system worker
  own user scope    system scope
        │                 │
  user systemd                 system systemd
  non-root → rootless Podman   rootful Podman
  UID 0 → rootful Podman
        └──────────────┬──────────────┘
                 │
       journald / cgroups / storage
```

The binaries are installed on every managed machine. Workers may be activated
by systemd sockets rather than remaining busy when no client or controller is
connected.

A later controller uses an authenticated remote form of the same semantic API:

```text
                      optional controller
                    inventory / coordination
                              │
                authenticated typed protocol
                 ┌────────────┼────────────┐
                 │            │            │
              worker A     worker B     worker C
                 │            │            │
             systemd       systemd       systemd
             Podman        Podman        Podman
```

Controller loss must not stop workloads or prevent local CLI and TUI use.

## Split worker authority

Graft will not use one ambiently privileged process to control every system and
user manager. The local authority is split:

- the **system worker** serves system-target workloads and the system manager;
- a **user worker** serves only the owning account's user-target workloads and
  user manager.

This follows the existing deployment boundary. A user worker must not control
another account or the system manager. The system worker must not silently
impersonate users or discover user buses from ambient host state.

User scope does not guarantee rootless execution. A worker owned by a non-root
account reaches that account's rootless Podman runtime. A user worker owned by
UID 0 reaches a rootful runtime with host-root authority even though it still
uses the user manager rather than the system manager. Design and test matrices
must therefore keep system/rootful, non-root user/rootless, and root-owned
user/rootful contexts separate.

The [Local worker and API contract](worker-api.md) fixes the process authority,
manifest, transport, framing, authorization, limits, recovery, and adapter
boundaries. Concrete socket paths, service identities, hardening, and any
privileged cross-scope administrative operation remain in [#242].

## One local client experience

A normal local CLI or TUI may present every scope that the caller is authorized
to inspect:

```text
system/database          running   420 MiB
system/reverse-proxy     running    85 MiB
user/zerodawn/qdrant     running   1.2 GiB
user/zerodawn/dev        stopped         -
```

The client connects to the caller's user worker and, when host policy permits,
the system worker. It does not need to run permanently as root.

Authorization principles:

- an account may manage its own user workloads directly;
- access to other users is denied unless a future explicit administrative
  contract says otherwise;
- system-scope observation follows host policy and may be granted separately
  from mutation;
- system `up`, `down`, and `restart` require explicit authorization, such as an
  approved polkit interaction;
- elevation is per operation, not a reason to run the whole TUI as root;
- every response and display keeps host, scope, and workload identity visible;
- an unqualified name present in multiple accessible scopes is ambiguous and
  fails closed.

The exact CLI syntax and authorization actions remain in [#135] and [#240].

## Declarative discovery manifest

The worker must not scan arbitrary TOML directories, Quadlet search paths, or
Podman containers and then reconstruct Graft intent. Nix materialisation will
instead publish a read-only, generation-specific manifest derived from resolved
workloads.

The detailed schema remains for [#240] and [#242], but it needs enough identity
and provenance to bind an operation safely, including:

- workload name and explicit target/scope;
- source TOML identity or digest without secret content;
- Graft source-unit and generated-service identity;
- materialisation generation or artifact identity;
- rootfs and closure identity needed for inspection;
- lifecycle and observability capabilities supported by that workload;
- Graft/API version compatibility metadata.

The manifest is evidence produced from TOML and Nix, not a second desired-state
database. A missing, stale, malformed, shadowed, or identity-mismatched manifest
must fail mutation with an actionable diagnostic. Detection of foreign search
path overrides remains part of [#171].

## Shared typed API

CLI, TUI, and controller must consume the same semantic operations:

```text
clients
  ↓ typed request, response, progress, and event contracts
worker
  ├── materialisation manifest adapter
  ├── systemd D-Bus adapter
  ├── journald adapter
  ├── Podman adapter
  ├── cgroup metrics adapter
  └── bounded storage accounting adapter
```

The first operation groups are:

- lifecycle: `up`, `down`, and `restart`;
- state: status and typed inspection snapshots;
- logs: bounded query and follow streams with journal cursors;
- metrics: CPU, memory, PID/cgroup, restart, rootfs, overlay, and volume data;
- events: lifecycle and availability changes with ordering metadata;
- capabilities: component/API versions and supported operation sets.

The [Local worker and API contract](worker-api.md) chooses bounded
length-prefixed JSON framing and a typed versioned envelope. The
[Local lifecycle operations](lifecycle-operations.md) contract fixes `up`,
`down`, and `restart`; final observability fields remain in [#137]. The protocol
rejects unknown mutation intent, validates bounds, versions explicitly, and
returns typed errors rather than raw backend output.

## Backend responsibility

The worker adapts requests without taking over backend authority:

| Backend | Worker use | Still authoritative for |
| --- | --- | --- |
| Materialisation manifest | Bind workload identity and provenance | TOML/Nix intent and artifacts |
| systemd D-Bus | Lifecycle requests, unit state, results, cgroups | Activation, ordering, restart, and service lifecycle |
| journald | Bounded log queries and cursor-based following | Journal retention and log records |
| Podman | Container identity, runtime detail, stats, storage metadata | Container runtime behavior |
| cgroups | CPU, memory, PID, and pressure observations | Kernel accounting and enforcement |
| Nix/rebuild tooling | Future explicit deployment integration only | Build, generation, activation, and rollback policy |

No API accepts arbitrary shell commands, Nix expressions, D-Bus methods, Podman
arguments, systemd properties, host paths, or Quadlet fragments. Backend errors
are classified and may include redacted diagnostics, but do not widen the
request contract.

## State model

A single `running` boolean cannot explain Graft state. Every snapshot keeps
these layers distinct:

1. **Declared** — reviewed TOML identity expected by the selected host config.
2. **Resolved** — validated typed intent and concrete workload relationships.
3. **Materialised** — Nix generation, rootfs, closure, manifest, and Quadlet
   source available on the host.
4. **Generated** — Quadlet generated the expected systemd service without known
   shadowing or drift.
5. **Manager** — systemd load, active, sub, result, enablement relationship,
   invocation, and restart state.
6. **Runtime** — Podman container identity, state, exit information, and runtime
   resources.
7. **Observed** — timestamped logs, metrics, health, and storage measurements.

A layer may be missing, stale, unavailable, unauthorized, or inconsistent.
Clients must show that state rather than collapse it into `stopped` or infer a
repair. The worker never edits TOML or triggers a rebuild to make layers agree.

## Worker operational state

The worker may hold only operational data needed to serve bounded requests:

- current snapshots and short-lived caches;
- in-flight operation and cancellation state;
- stream cursors and sequence metadata;
- bounded rate-limit and backpressure state;
- audit records emitted to an approved host sink such as journald.

It must not persist desired workload configuration. A worker restart rebuilds
its view from the materialisation manifest and authoritative backends. Cache
loss cannot change what should run.

The controller may later retain host inventory, last observations, events,
audit history, and deployment execution results. Such records are operational
history, not permission to recreate or mutate workloads without current
TOML/Nix provenance and local worker authorization.

## Local availability and failure behavior

Local lifecycle and observability remain independent of the controller. Defined
failure categories must include:

- worker unavailable or API-incompatible;
- unauthorized scope or action;
- missing, stale, malformed, or ambiguous manifest identity;
- systemd manager or user bus unavailable;
- generated unit missing, shadowed, or failed;
- Podman unavailable or runtime identity mismatched;
- logs rotated or cursor expired;
- metrics unsupported, stale, or temporarily unavailable;
- bounded request, stream, or storage-accounting limit exceeded;
- concurrent, duplicate, cancelled, or interrupted mutation.

Read-only partial results may identify unavailable layers. Mutation must fail
closed before acting when workload identity, scope, authorization, or intended
operation is ambiguous. Idempotency and concurrency state machines remain for
[#135] and [#240].

## Optional controller

The controller adds multi-host aggregation and coordination:

- host and workload inventory;
- combined state, metrics, events, and logs;
- authorized remote lifecycle requests;
- deployment plans, progress, verification, and rollback coordination;
- audit and compatibility visibility.

It is not a scheduler, host-policy authority, or required runtime dependency.
Workers revalidate every remote request against local identity, API capability,
and Nix-installed policy. The authenticated protocol, enrollment, revocation,
stream recovery, and controller persistence boundaries remain in [#245].
Multi-host deployment state machines remain in [#161] and [#174].

## Deployment boundary

The first control-plane implementation manages only already materialised
workloads. `up`, `down`, and `restart` never imply a NixOS or Home Manager
rebuild.

A future explicit authoring and deployment flow may integrate with an approved
automatic rebuild tool:

```text
TUI or controller proposes typed TOML change
  ↓ reviewable diff + validation + explicit authorization
repository records declarative change
  ↓ approved rebuild tool builds and activates a generation
worker observes and verifies the new manifest and runtime result
  ↓ report success or retain/restore the previous generation
```

Ordinary lifecycle and observability requests cannot enter this flow. The
worker never silently edits TOML, commits repository changes, invokes arbitrary
Nix commands, or treats runtime state as configuration. Detailed authoring,
approval, rebuild, and rollback contracts require separate future design.

## Security impact and capability classification

This design deliberately expands Graft from deterministic build-time resolution
to two host-aware capabilities: read-only runtime observation and explicitly
authorized lifecycle mutation. It does not expand TOML workload authority,
relax generated workload policy, or grant a generic host-control surface. The
new authority belongs to authenticated API clients and remains bounded by the
effective system or user target, account UID, materialisation identity, typed
operation, and host policy.

The design affects these current threat-model invariants:

- **GRAFT-TM-01:** unknown versions, operations, fields, and explicit unsupported
  request intent must fail closed;
- **GRAFT-TM-02:** the API must preserve the prohibition on raw Quadlet, Podman,
  systemd, host-command, and equivalent backend passthrough;
- **GRAFT-TM-03:** backend-controlled logs, paths, labels, metrics, errors, and
  other text must not inject protocol fields or terminal control output;
- **GRAFT-TM-04:** manifest and workload relationships must bind to one explicit
  source set, host, generation, target, and concrete identity;
- **GRAFT-TM-05:** `user` remains manager scope rather than proof of a non-root
  UID, so system/rootful, user/rootless, and root-owned user/rootful authority
  stay distinct;
- **GRAFT-TM-06:** explicit runtime lifecycle requests do not alter declarative
  startup relationships or make materialisation imply startup;
- **GRAFT-TM-07:** rootfs and closure identity is observed through the manifest
  and cannot become arbitrary package, store-path, or mount selection;
- **GRAFT-TM-09:** stop and restart operations do not implicitly remove mounted
  state, persistent data, workspaces, or foreign units; and
- **GRAFT-TM-13:** lifecycle requests cannot relax resolved read-only,
  capability, or no-new-privileges policy.

Operational capability classification:

| Operation | Class | Reason |
| --- | --- | --- |
| Own-user status, metrics, logs, and inspect | First-class, potentially sensitive observation | It is read-only but may expose application data, paths, resource behavior, and failures available to that account. |
| Own-user `up`, `down`, and `restart` | First-class typed runtime mutation | It controls only an explicitly identified workload within the caller's existing user-manager authority. |
| System observation | Dangerous, host-policy controlled | Even read-only system metadata and logs may disclose cross-service or host information. |
| System `up`, `down`, and `restart` | Dangerous privileged operation | It mutates rootful system services and requires explicit per-operation authorization rather than ambient TUI privilege. |
| Remote controller mutation | Dangerous and planned | It extends mutation authority across a network and requires enrollment, mutual authentication, replay protection, audit, and local revalidation. |
| TOML mutation or rebuild activation | Dangerous and deferred | It changes declarative intent or host generations and requires a separately reviewed authoring and deployment contract. |
| Raw shell, Nix, systemd, D-Bus, Podman, Quadlet, path, or arbitrary RPC input | Forbidden | It bypasses typed policy and turns the worker into a generic privileged execution proxy. |

## Security boundaries

The control plane adds authority-bearing local and remote inputs. Its threat
model must cover:

- hostile or compromised local clients;
- same-user access to a user socket;
- system observation versus mutation authorization;
- confused-deputy and cross-scope requests;
- stale manifests and workload-name reuse;
- malicious backend text, logs, labels, paths, and metric values;
- unbounded log, event, metric, or storage requests;
- controller compromise, replay, downgrade, and partial connectivity;
- audit redaction and secret/path exposure;
- worker replacement, socket spoofing, and version mismatch.

Local Unix peer credentials identify a process but do not alone define what it
may do. Host policy maps authenticated peers to typed capabilities. Remote
transport additionally requires mutual authentication, encryption, replay
resistance, explicit enrollment, rotation, revocation, and local revalidation.

## Non-goals for the first implementation

The first local control plane does not provide:

- TOML editing or automatic rebuilds;
- a central controller or remote transport;
- automatic placement, scheduling, or reconciliation;
- arbitrary host command execution;
- management of foreign systemd or Podman workloads;
- cross-user impersonation;
- a long-term metrics database;
- secret transport;
- implicit persistent-data deletion;
- replacement of systemd, Podman, journald, Nix, or existing host policy.

## Design and implementation sequence

1. Approve this umbrella architecture in [#232].
2. Specify the worker, local API, authorization, and manifest contract in
   [#240].
3. Specify observability semantics in [#137], composing with the approved
   [local lifecycle contract](lifecycle-operations.md).
4. Specify Nix installation, socket, service, and ownership policy in [#242].
5. Implement the worker, lifecycle operations, and CLI integration in [#241]
   and [#136].
6. Design and implement the TUI in [#243] and [#244].
7. Design the authenticated controller protocol in [#245].
8. Design and implement multi-host deployment through [#161], [#246], and
   [#174].

[#135]: https://github.com/Patrick-Kappen/graft/issues/135
[#136]: https://github.com/Patrick-Kappen/graft/issues/136
[#137]: https://github.com/Patrick-Kappen/graft/issues/137
[#161]: https://github.com/Patrick-Kappen/graft/issues/161
[#171]: https://github.com/Patrick-Kappen/graft/issues/171
[#174]: https://github.com/Patrick-Kappen/graft/issues/174
[#232]: https://github.com/Patrick-Kappen/graft/issues/232
[#240]: https://github.com/Patrick-Kappen/graft/issues/240
[#241]: https://github.com/Patrick-Kappen/graft/issues/241
[#242]: https://github.com/Patrick-Kappen/graft/issues/242
[#243]: https://github.com/Patrick-Kappen/graft/issues/243
[#244]: https://github.com/Patrick-Kappen/graft/issues/244
[#245]: https://github.com/Patrick-Kappen/graft/issues/245
[#246]: https://github.com/Patrick-Kappen/graft/issues/246
