# Nix worker integration

> **Status:** approved design for future implementation. The current release
> materialises workloads but does not ship the worker or TUI described here.
> Worker implementation remains in [#241]; this document fixes how NixOS and
> Home Manager will install, isolate, authorize, activate, upgrade, and roll back
> the local components.

This contract supplies the concrete host-policy boundary required by the
[control-plane](control-plane.md), [worker API](worker-api.md),
[lifecycle](lifecycle-operations.md), and
[observability](observability.md) designs. TOML remains workload intent. Nix
remains package, host-policy, artifact, and current-generation authority. The
Nix modules do not implement runtime business logic.

## Objectives

The integration must:

- install the CLI, TUI, and applicable local worker from one Nix package;
- fix each worker's target, effective UID, manager, runtime, socket, manifest,
  lock, interlock, and authorization context before it accepts clients;
- keep system, non-root user, and UID-0 user authority distinct;
- expose no network listener and require no controller;
- publish deterministic read-only discovery manifests instead of allowing
  ambient TOML, Quadlet, unit, or container discovery;
- coordinate activation with lifecycle submission and durable `/run`
  interlocks;
- provide explicit socket ownership, operation authorization, audit, hardening,
  restart, idle, upgrade, rollback, and failure behavior;
- keep secrets out of TOML, generated workload units, environment variables,
  command arguments, the Nix store, manifests, logs, and endpoint descriptors;
  and
- test observable installed artifacts and runtime effects, not merely module
  evaluation success.

It must not add a remote controller transport, enrollment flow, hidden rebuild,
cross-user worker, raw backend access, persistent desired-state database, or
worker-managed login/linger policy.

## Component and option boundary

The future complete package contains these public executables:

```text
graft
graft-tui
graft-worker
graft-pause
```

The exact executable names become package tests before implementation. The CLI,
TUI, and worker must come from the same selected package so a Nix generation
cannot silently combine unrelated protocol implementations.

### NixOS

The existing option remains the root, with one required stable host identity:

```nix
services.graft = {
  enable = true;
  package = pkgs.graft;
  hostId = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
};
```

Once [#241] provides the binaries, enabling the complete integration:

- installs CLI and TUI in `environment.systemPackages`;
- retains existing system workload materialisation;
- publishes the system manifest and endpoint descriptor;
- creates the fixed system group and runtime paths;
- installs the system worker service/socket and polkit actions; and
- installs the activation hook that shares the lifecycle lock.

Before that implementation lands, the current module continues to materialise
only workloads and must not install a unit referencing a nonexistent worker.
The implementation transition must be explicit and tested; documentation cannot
claim these components are available early.

### Home Manager

The existing option remains the root, with the same stable host identity:

```nix
programs.graft = {
  enable = true;
  package = pkgs.graft;
  hostId = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
};
```

Once implemented, enabling the complete integration:

- installs CLI and TUI in `home.packages`;
- retains existing user workload materialisation;
- publishes the user manifest and endpoint descriptor;
- creates the private runtime tree through user tmpfiles;
- installs the user worker service/socket; and
- installs the Home Manager activation hook that shares the user lock.

Home Manager does not alter system groups, system polkit, linger, login sessions,
or another account's manager.

### Canonical host identity

`hostId` is a non-secret canonical lowercase RFC 9562 UUIDv7 string and is the
only host identity published in worker manifests/descriptors. Empty values,
uppercase, braces, non-canonical hyphen placement, invalid hex, a version nibble
other than `7`, a variant other than binary `10`, or trailing data fail
evaluation. It is configured fleet identity, not a
hostname, raw `/etc/machine-id`, boot ID, network address, or value generated at
each evaluation.

`services.graft.hostId` is required when the NixOS worker integration is enabled.
An integrated Home Manager module inherits exactly that value through `osConfig`
and fails evaluation if an explicit `programs.graft.hostId` differs. Standalone
Home Manager has no NixOS source and therefore requires an explicit `hostId` when
the user worker is enabled. Nix-generated system/user descriptors and manifests
carry the same UUID; clients/workers refuse local aggregation or provenance
binding when values differ. Runtime endpoint validation also compares the user
value with the installed system descriptor when that authorized descriptor is
present.

### Package validation

Nix evaluation fails when workload roots or worker integration are selected
without a package. Build-time package checks require each enabled executable at
the exact `lib.getExe'` path and verify its declared protocol/manifest producer
metadata. Missing binaries, empty metadata, duplicate component identities, or
an incompatible manifest schema fail before activation.

The modules pass fixed paths and typed policy values as arguments generated by
Nix. They do not pass secret values or accept free-form worker/backend arguments.

## Fixed worker topology

A worker process has exactly one row from this table for its lifetime:

| Context | Manager | Podman authority | Effective identity | Manifest target |
| --- | --- | --- | --- | --- |
| System | System manager | Rootful system runtime | UID 0 | `system` |
| Non-root user | Owning user manager | Rootless user runtime | Owning non-zero UID | `user` |
| UID-0 user | UID-0 user manager | Rootful UID-0 user runtime | UID 0 | `user` |

The system and UID-0 user rows remain distinct despite sharing UID 0. Their
manager, socket, manifest, source-unit search path, generated service, runtime
connection, lock, interlock tree, endpoint identity, and audit context differ.
Neither worker accepts a client field that changes row.

## Installed identities and paths

### System context

| Purpose | Fixed value |
| --- | --- |
| Service | `graft-system-worker.service` |
| Socket unit | `graft-system-worker.socket` |
| Socket | `/run/graft/system/worker.sock` |
| Runtime directory | `/run/graft/system` |
| Generation pointer | `/etc/graft/current` |
| Manifest | `/etc/graft/current/manifest.json` |
| Endpoint descriptor | `/etc/graft/current/endpoint.json` |
| Activation lock | `/run/graft/system/activation.lock` |
| Interlock directory | `/run/graft/system/interlocks` |
| Socket access group | `graft` |
| Service user/group | `root:root` |
| Audit context | system journal |

### User context

| Purpose | Fixed value |
| --- | --- |
| Service | `graft-user-worker.service` |
| Socket unit | `graft-user-worker.socket` |
| Socket | `$XDG_RUNTIME_DIR/graft/user/worker.sock` |
| Runtime directory | `$XDG_RUNTIME_DIR/graft/user` |
| Generation pointer | `$XDG_CONFIG_HOME/graft/current` |
| Manifest | `$XDG_CONFIG_HOME/graft/current/manifest.json` |
| Endpoint descriptor | `$XDG_CONFIG_HOME/graft/current/endpoint.json` |
| Activation lock | `$XDG_RUNTIME_DIR/graft/user/activation.lock` |
| Interlock directory | `$XDG_RUNTIME_DIR/graft/user/interlocks` |
| Socket owner | owning effective UID and primary GID |
| Service identity | owning effective UID and primary GID |
| Audit context | owning user journal |

`$XDG_RUNTIME_DIR` and `$XDG_CONFIG_HOME` above describe the Nix-installed
account paths. For this Linux/systemd integration, `%t` must resolve to canonical
`/run/user/<effective-uid>`; the worker validates that equality at startup. The
worker does not trust client environment variables to resolve paths. Its service
receives fixed expanded paths from the owning manager/Nix generation. Empty, relative, non-normalized, wrong-owner, or scope-mismatched
generation pointers fail closed. The fixed `current` pathname may be exactly one
Nix-managed symlink to an immutable directory in the Nix store. That directory
contains regular `manifest.json` and `endpoint.json` files; neither may be a
symlink. Symlinked components before `current`, another symlink at the resolved
store directory, additional chains, non-store targets, unexpected entries, and
wrong target/file type or owner fail closed.

No option permits overriding service names, socket paths, lock paths, interlock
paths, manager address, Podman connection, or manifest target. Stable client
endpoint discovery uses the descriptors below, not an arbitrary privileged
socket environment variable.

## Runtime directory ownership

NixOS tmpfiles creates:

```text
/run/graft                    0755 root root
/run/graft/system             0750 root graft
/run/graft/system/interlocks  0700 root root
```

The lock is created without following symlinks as `0600 root:root`. The system
socket is `0660 root:graft`. Interlock temporary/final files remain `0600
root:root` and never inherit socket-group readability.

User tmpfiles creates:

```text
$XDG_RUNTIME_DIR/graft                    0700 <uid> <primary-gid>
$XDG_RUNTIME_DIR/graft/user               0700 <uid> <primary-gid>
$XDG_RUNTIME_DIR/graft/user/interlocks    0700 <uid> <primary-gid>
```

The user lock, socket, and interlocks are `0600` and owned by that account.
Creation rejects pre-existing symlinks, non-directories, wrong ownership, and
broader permissions. Runtime trees disappear at runtime-directory/boot teardown;
this does not authorize clearing ambiguous interlocks while the context still
exists.

## Socket activation

Both contexts use one Unix `SOCK_STREAM` listening socket with:

```text
Accept=no
RemoveOnStop=yes
```

The owning manager passes exactly one activated descriptor to one worker. The
worker rejects zero, multiple, wrong-type, or unexpected-name descriptors. It
does not bind a fallback path.

The socket unit may start independently. The service starts on the first
connection and accepts all peers itself so worker-wide limits remain shared. A
service crash leaves socket ownership with systemd and starts a new worker epoch
on the next accepted activation/restart.

### System socket admission

The socket directory and `0660 root:graft` mode are the first admission layer.
The `graft` group is created as a system group with no default members. A typed
Nix list may add existing local users to it. Empty names, unknown users,
duplicates, UID 0 aliases, and free-form group replacement fail evaluation.
Administrators may instead add users through their normal NixOS user definitions;
that remains explicit host policy.

Group membership allows a process to connect and spend connection-level quota.
It grants no operation by itself. Accepted peer UID/GID/PID and current subject
are still authenticated and authorized below.

### User socket admission

The user socket is private `0600`. The worker accepts only a peer whose effective
UID from kernel peer credentials equals the worker effective UID. Supplementary
groups, a client-provided UID, root ownership of a process namespace, or socket
file access alone cannot substitute for that equality.

A user worker exists only while its user manager and runtime directory exist.
Graft does not enable linger, synthesize a runtime directory, start a user
manager, or contact another user's socket. Local clients return typed
`worker_unavailable` when the context is absent.

### No remote listener

No NixOS/Home Manager option in this design accepts an address, port, TLS
listener, SSH command, proxy command, or controller endpoint. Installation opens
no firewall port. Future controller transport/enrollment requires [#245] and
[#246] and cannot reuse this local socket policy implicitly.

## Endpoint descriptors

Nix publishes non-secret canonical JSON descriptors so clients need no arbitrary
socket environment variable. A descriptor contains only:

- descriptor schema version;
- host identity issued by Nix policy;
- context `system` or `user`;
- typed socket address: absolute system path or fixed Linux user-runtime-relative
  suffix;
- expected worker/API compatibility range;
- package producer identity; and
- descriptor generation/digest.

System and user descriptors are independently readable according to their
context. The system descriptor encodes
`absolute("/run/graft/system/worker.sock")`. The user descriptor encodes
`linux_user_runtime_relative("graft/user/worker.sock")`; it contains neither an
expanded path nor effective UID. A Linux user client derives exactly
`/run/user/<geteuid()>/graft/user/worker.sock` from its kernel effective UID,
rejects overflow/non-canonical UID formatting and path components, and verifies
the directory/socket ownership, mode, type, and authenticated worker handshake.
It never reads `$XDG_RUNTIME_DIR` or another ambient variable for privileged
endpoint selection.

The user worker likewise binds its actual effective UID at startup, proves its
manager `%t` equals canonical `/run/user/<effective-uid>`, returns the UID through
the authenticated handshake/observation envelope, and requires exact peer-UID
equality. A client may aggregate both configured descriptors, but it preserves
scope and rejects duplicate/ambiguous workload names as specified by the API.
The descriptor is discovery data, not proof that a socket peer is genuine:
clients also verify socket type, ownership, mode, context, handshake, and
protocol identity.

No descriptor contains credentials, bearer tokens, environment values, raw
Nix paths not otherwise public, controller state, or backend endpoints.

## System authorization

The system worker uses fixed polkit action identifiers:

```text
io.github.patrick-kappen.graft.system.observe
io.github.patrick-kappen.graft.system.inspect
io.github.patrick-kappen.graft.system.lifecycle
```

The client never supplies an action identifier. Worker request types map to one
compiled identifier:

| Worker capability | Action |
| --- | --- |
| Discovery, summary status, approved fast metrics, status/events follow | `observe` |
| Full status/inspect, logs query/follow, storage accounting | `inspect` |
| `up`, `down`, `restart`, operation-result/progress for mutation | `lifecycle` |

The default installed policy is:

| Subject | Observe | Inspect | Lifecycle |
| --- | --- | --- | --- |
| Root peer in system context | allow | allow | allow |
| Explicit `graft` socket-group peer | allow | administrator authentication | administrator authentication per operation |
| Any other local peer | deny before socket/API work | deny | deny |

“Per operation” means a lifecycle request performs a current polkit check after
peer authentication and before audit/manifest/backend mutation. Authentication
caching controlled by polkit cannot become worker authorization state: every
request revalidates subject PID start identity, UID, action mapping, and
authorizer result. Stale/reused PID, vanished process, changed credentials,
missing agent, denied/cancelled prompt, timeout, malformed reply, or unavailable
polkit returns a typed denial and starts no backend mutation.

Root bypass is limited to a kernel-authenticated UID-0 peer on the system socket;
it is not selected by JSON or group name. A UID-0 user-context client connecting
to the system socket is still represented by its actual peer and receives
system authority only under this rule; that does not merge worker contexts.

Nix may lower defaults to deny or require administrator authentication. It may
not make inspect/lifecycle weaker than observe, grant lifecycle from connection
alone, add client-selected action IDs, or install an allow-all policy. Exact
policy enums and ordering are schema-tested.

## User authorization

A user worker authorizes its exact same-UID peer for approved discovery,
observability, and lifecycle operations on current manifest-bound `user`
workloads. It never invokes system polkit to widen authority and never accesses
the system manager.

Operation authorization remains separate from authentication:

- disabled workloads cannot mutate;
- stale/wrong generation and scope fail;
- unsupported capabilities fail;
- manifest/unit/runtime provenance is revalidated;
- limits and interlocks apply; and
- UID 0 is reported as rootful user authority.

Cross-user inspection or lifecycle is unavailable in version 1, including for a
normal administrative account. A future administrative contract cannot be
implemented by changing the worker UID in a client request.

## Declarative manifest

Nix materialisation produces one canonical manifest per context. The manifest is
built from already resolved workload records; the worker does not parse ambient
TOML or infer desired state from generated artifacts.

### Envelope

The manifest envelope contains:

- manifest schema version;
- minimum/maximum compatible worker API version;
- package producer name/version/build identity;
- non-secret Nix host identity;
- exact target/manager context, without a build-time user UID;
- generation ID and full manifest digest;
- deterministic workload count; and
- sorted workload records.

Version 1 uses canonical JSON bytes and SHA-256 with this non-circular rule:

1. remove top-level `generationId` and `manifestDigest` from the manifest;
2. canonicalize the remaining manifest, which already includes producer
   compatibility metadata;
3. set `manifestDigest` to lowercase hexadecimal SHA-256 of those bytes; and
4. set `generationId` equal to that digest.

Neither field is represented as null in the preimage; both keys are absent.
Wall-clock time and mutable host state are excluded. A verifier removes exactly
those two fields, recomputes the digest, checks both stored values, and rejects
unknown digest algorithms or encodings.

### Workload record

Every record contains the typed fields required by worker/lifecycle/observability
contracts:

- manifest-issued workload ID;
- workload name and explicit target;
- enabled/disabled state;
- lifecycle kind and startup intent;
- non-secret source TOML identity/digest;
- resolved intent and dependency-graph digests;
- Quadlet source-unit identity;
- expected generated-service identity;
- materialised artifact/provenance identity;
- rootfs and closure identities;
- typed dependency service identities needed by activation scans;
- supported lifecycle operations;
- supported observability layers/metrics/storage classes; and
- required backend/producer compatibility metadata.

Records never contain environment values, secrets, credential references,
command lines, raw unit/Quadlet text, arbitrary systemd properties, Podman
arguments, absolute private TOML paths, client-selectable backend IDs, or
controller credentials.

### Determinism and validation

Build-time checks enforce:

- canonical serialization and stable workload ordering;
- unique workload IDs, names, source units, and generated services within scope;
- target/context equality;
- type/cardinality/size limits;
- non-empty IDs and digests;
- exact correspondence between manifest workload records and materialised
  enabled/disabled Graft inputs;
- exact correspondence between each enabled record and generated Quadlet source;
- package/API/schema compatibility; and
- absence of forbidden secret/environment/credential field classes.

The real materialisation outputs feed these checks; tests do not duplicate a
simplified manifest generator.

## Current-generation publication

Each context builds one immutable Nix store directory:

```text
<generation>/manifest.json
<generation>/endpoint.json
```

Both files carry the same host ID, context, manifest `generationId`, producer
identity, and compatibility metadata. The endpoint also carries the exact
`manifestDigest`. Its own `endpointDigest` is lowercase hexadecimal SHA-256 of
canonical endpoint JSON with only `endpointDigest` omitted. Thus endpoint
identity is non-circular and cryptographically binds the manifest generation it
advertises; immutable same-directory publication binds the pair. Build checks
independently recompute both preimages/digests and reject any mismatch. The fixed
atomic pointer is:

```text
/etc/graft/current
$XDG_CONFIG_HOME/graft/current
```

The worker/client opens that one configured pointer, validates the resolved
immutable store directory and both regular children, then retains descriptors
and parsed snapshots from that same opened directory for an operation. It never
resolves `current` independently for each file, follows another chain, or
searches neighboring generations.

Publication occurs only inside the activation critical section below. The
implementation replaces generic `environment.etc`/`xdg.configFile` publication
for Graft-owned Quadlet artifacts and this generation pointer with one
lock-aware activation publisher; two independent activation mechanisms may not
own the same paths. Activation atomically replaces only `current` after all
incoming artifacts/provenance pass. A failed activation before replacement
leaves the old pointer; a failure after replacement atomically restores the old
pointer before releasing the lock and revalidates its pair. Thus endpoint and
manifest cannot advertise different generations. Merely building a Nix
generation does not make it current.

## Shared activation and submission lock

The concrete advisory lock files are:

```text
/run/graft/system/activation.lock
$XDG_RUNTIME_DIR/graft/user/activation.lock
```

The implementation uses Linux open flags that reject symlinks, validates regular
file type/owner/mode, and takes an advisory `flock`. System activation and system
worker both run as root. Home Manager activation and user worker both run as the
owning effective UID. No client can supply the path or descriptor.

Lock modes are:

- activation: exclusive;
- worker submission: shared activation lock plus the worker's independent
  per-workload mutation lock.

The shared mode permits independently serialized worker requests only where the
worker contract allows; it never replaces per-workload/operation locks. Lock
acquisition has bounded deadlines and interruption handling. Failure, wrong
ownership, wrong type, unsupported filesystem locking, or timeout fails closed.

### Activation critical section

NixOS/Home Manager activation holds the exclusive lock across this exact order:

1. validate incoming manifest/artifact schema and context;
2. load and validate every retained interlock;
3. construct the union of current and incoming Graft service/dependency
   identities;
4. query the fixed manager and perform the manager-wide quiescence scan;
5. stop on any queued job, transition, retry delay, cleanup, ambiguous manager
   epoch, unavailable backend, or unsafe interlock;
6. replace context-owned Quadlet/artifact references;
7. request the correct manager generator reload/`daemon-reload`;
8. verify expected generated services and provenance with bounded deadlines;
9. atomically publish the one manifest/endpoint generation pointer;
10. notify/restart the worker only after publication; and
11. release the lock.

A failure after artifact replacement but before manifest publication restores
the previous coherent artifact/reference set or fails activation while retaining
an explicit recovery marker; it never publishes a mixed generation as current.
The implementation issue must choose transaction primitives and prove failure
points with integration tests rather than relying on shell command success.

Activation does not start disabled workloads, stop workloads merely removed from
incoming intent, clear interlocks, create a user session, enroll a controller,
or perform lifecycle retries.

### Worker submission critical section

The worker takes submission mode only after ordinary authentication,
authorization, parsing, and preliminary observation. Under the lock it discards
those preliminary decisions and redoes:

1. manifest reference/schema/digest/context/generation validation;
2. manager epoch and selected-unit state/job lookup;
3. loaded source/generated service provenance validation;
4. lifecycle matrix/precondition evaluation;
5. operation/interlock capacity validation;
6. durable `prepared` interlock creation for non-`no_change` work;
7. operation-ID acceptance; and
8. verified manager-work attachment/submission through acceptance or rejection.

It then releases the activation lock before terminal observation. Manifest
publication cannot interleave with the identity/manager acceptance boundary.

## Durable runtime interlocks

Interlocks use the fixed private directories above and the bounded schema from
the lifecycle contract. They are non-secret operational safety records, not
workload desired state.

Creation uses same-directory temporary files, owner-only mode, complete bounded
serialization, file sync, atomic no-replace rename, and directory sync before
operation acceptance. Updates use an equally durable replace sequence. The
worker validates regular-file type, owner, mode, link count, filename/record
identity, schema, size, count, boot/context/manager identity, and checksum before
use.

The exact fixed maxima remain:

```text
256 interlocks per worker
4 KiB encoded per record
1 MiB total encoded interlock bytes per worker
16 concurrent reconciliation queries
healthy complete reconciliation sweep within 2 minutes
```

Nix policy cannot alter the coupled safety/liveness tuple. Invalid, oversized,
unknown, duplicate, wrong-owner, unreadable, or ambiguous records block
activation and matching lifecycle mutation. They are never ignored or
quarantined automatically.

### Administrator recovery

There is no normal CLI force/delete operation. Recovery is an explicit local
administrator procedure delivered by the implementation issue. It must:

1. hold the exclusive activation lock;
2. stop or prove terminal every exact correlated manager job/invocation/service;
3. prove no retry, cleanup, late action, or cgroup process remains;
4. verify boot, manager, manifest, unit, and record identity;
5. archive bounded non-secret evidence to the journal;
6. remove only the exact proven record and sync its directory; and
7. rerun reconciliation/quiescence before activation or mutation resumes.

Timeout, worker restart, package upgrade, rollback, cache eviction, unknown
result, missing client, or administrator preference alone never clears a record.
A future recovery command requires a separate reviewed typed contract.

## Backend context installation

Nix installs dependencies and policy; it does not implement adapter behavior.

### systemd

The system worker connects only to the system bus/manager. A user worker connects
only to its inherited owning user bus/manager. No client bus address is accepted.
Manager availability and epoch checks remain runtime worker responsibilities.

### Podman

The system worker uses only the rootful system API endpoint
`/run/podman/podman.sock`, owned by system `podman.socket`. Its service has fixed
`Wants=podman.socket` and `After=podman.socket`; socket activation starts only
that local Unix API, never a network listener.

A non-root or UID-0 user worker uses only
`$XDG_RUNTIME_DIR/podman/podman.sock`, owned by `podman.socket` in that same user
manager. Its service has fixed user-manager `Wants=podman.socket` and
`After=podman.socket`. UID 0 remains rootful but still uses the UID-0 user
manager/socket rather than `/run/podman/podman.sock`.

Nix installs/enables the applicable socket unit with the worker context. A
Podman socket start failure does not stop the worker; runtime-dependent fields
and operations return typed unavailability. Graft does not start a user manager
or grant cross-user runtime access. Empty, missing,
wrong-owner, wrong-type, symlinked, non-socket, manager-mismatched, or
incompatible endpoints make the Podman adapter unavailable. A client cannot
provide a URI, socket, connection name, container ID, or storage path, and no CLI
fallback is attempted.

### Journald and audit

The system worker writes to the system journal and reads only manifest-bound
system-unit records. A user worker writes to/reads its user journal context. A
mandatory system mutation does not reach manifest/backend work unless the
bounded audit sink accepted the denial or authorized-attempt event required by
the API contract. Sink unavailable/saturated/I/O failure returns authorizer/audit
unavailability and fails closed.

Audit records use typed fields and include principal/context/action/workload
identity, authorization decision, operation/request identity, phase, outcome,
and worker/manager/manifest epochs where applicable. They omit secrets,
credential paths, raw backend text, command/environment values, log messages,
and unrestricted client data. Rate limiting cannot suppress required individual
mutation decisions/outcomes.

## Service hardening

### System worker

The system worker runs as root because it owns system-manager/rootful-runtime
authority and root-owned durable interlocks. `DynamicUser` and an ambient
cross-user helper are forbidden. Initial hardening is:

```text
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=read-only
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
ProtectClock=yes
ProtectHostname=yes
ProtectProc=invisible
ProcSubset=pid
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
RestrictRealtime=yes
RestrictNamespaces=yes
SystemCallArchitectures=native
UMask=0077
```

The implementation validates each directive against systemd D-Bus, journald,
Podman API, cgroup read, socket activation, and interlock writes. It grants
explicit read/write paths only where proven necessary, initially the private
system runtime tree. It does not add blanket `/run`, `/var`, `/etc`, `/home`,
Podman storage, Nix store, cgroup, device, or host filesystem write access.

Required Unix/D-Bus/API sockets are allowlisted by service policy where systemd
supports it. Capability bounding starts empty unless controlled integration
tests prove one is required; any addition needs a specific documented reason and
negative test. Hardening that breaks a required adapter is adjusted narrowly,
not disabled wholesale.

### User worker

The user worker has no elevation, setuid helper, supplementary system-runtime
group, or system polkit widening. It inherits only its owning user manager/runtime
context and writes only its private runtime tree. It uses the compatible subset
of the system hardening above, including strict filesystem protection, private
temporary files, no-new-privileges, restrictive umask, no realtime, and no
unneeded namespaces.

Home Manager evaluation/runtime tests distinguish non-root user and UID-0 user
behavior. A directive unavailable in a supported user-manager/systemd version is
a typed compatibility decision, never silently omitted by broad version checks.

## Restart, idle, and backend failure

The service is socket activated with this fixed version-1 restart policy in both
manager contexts:

```text
Restart=on-failure
RestartSec=2s
StartLimitIntervalSec=60s
StartLimitBurst=5
```

Clean worker exit is not restarted by the service, while the socket can activate
it again. Five failed starts within the rolling 60-second manager interval hit
the start limit; further activation fails until systemd permits/reset-failed is
performed under ordinary administrator policy. No worker code resets its own
limit. A failed service leaves the socket managed but clients receive a bounded
connection/protocol failure until a permitted restart succeeds.

Every process start creates a new worker epoch. Before accepting mutation, the
worker:

1. validates runtime paths and current manifest;
2. loads every interlock within fixed limits;
3. obtains the fixed manager epoch;
4. reconciles retained records; and
5. marks mutation unavailable wherever reconciliation remains ambiguous.

Read-only capabilities may report partial/backend-unavailable state according to
the observability contract. They cannot bypass unsafe records for mutation.

Idle exit is disabled in protocol/integration version 1 and there is no Nix
option or advertised timeout. After first socket activation, a healthy worker
remains active until explicit manager stop, package/generation replacement,
manager/session teardown, clean administrative shutdown, or failure. This keeps
reconciliation, interlock, audit, and backend-availability work resident. A
future idle policy requires a separately reviewed contract and cannot be enabled
by passing an extra worker argument.

## Secrets and credentials

Version 1 installs no controller, enrollment, remote transport, client
certificate, bearer token, or workload-secret interface. There is therefore no
generic `credentials`, `environment`, or `extraArgs` option.

Future controller credentials require the approved [#245]/[#246] transport and
may be referenced only through typed Nix options whose values are runtime secret
paths produced by an approved mechanism such as sops-nix. The service consumes
them through systemd `LoadCredential=` or equivalent private credential
facilities. Secret bytes never enter:

- the Nix store/derivation arguments;
- TOML or resolved JSON;
- manifests or endpoint descriptors;
- Quadlet/workload units;
- environment variables;
- process arguments;
- logs/audit/errors; or
- worker caches/interlocks.

Nix/runtime checks require regular non-symlink files, expected owner, owner-only
readability, bounded size, and exact credential purpose. Missing, empty,
wrong-owner, broad-mode, stale-generation, malformed, or unavailable credentials
fail enrollment/remote functionality closed without affecting local clients.
No such options are added before the protocol exists.

## Upgrade and rollback

Package, system/user units, endpoint policy, polkit policy, materialised
artifacts, and manifest are one Nix/Home Manager generation. The manifest and
endpoint descriptor declare producer and API compatibility. Build/evaluation
reject known incompatibility; runtime negotiation rejects unknown/newer
incompatibility before detailed requests or mutation.

### Upgrade

Activation first passes the lock/interlock/quiescence contract. It then installs
and verifies the coherent incoming artifact/manifest set and restarts/reloads the
worker only after publication. Existing accepted lifecycle work is never
silently transferred as a successful result. Interlocks remain in `/run`; the
new worker epoch reconciles them before mutation.

A package upgrade may interrupt clients with typed `worker_shutdown` where a
frame can safely be delivered. Clients reconnect through the stable socket and
renegotiate. Journal cursors may survive; worker sequences, pages, metric
baselines, operation caches, and snapshots do not.

### Rollback

NixOS generation rollback or Home Manager generation rollback applies the same
activation lock, incoming/current union scan, interlock validation, provenance
checks, and publication order as an upgrade. It does not bypass compatibility
because the target is older.

Rollback restores the package, units/policy, Quadlet artifacts, endpoint
descriptor, and manifest as one coherent generation. It cannot adopt a currently
running same-named service/container whose source/provenance differs. Unsafe
manager work or ambiguous interlocks block rollback with an actionable failure;
operators use the separately reviewed recovery procedure rather than deleting
state.

### Reboot boundary

System `/run` and user runtime trees are boot/session-scoped. Reboot terminates
the old manager work and changes boot/manager epochs. Startup recreates paths and
loads the current Nix manifest. The implementation must document whether any
pre-reboot ambiguous evidence is additionally persisted by an approved future
mechanism; version 1 does not claim cross-boot operation-result recovery. It
never reports old work as successful merely because volatile interlocks are gone.

## Unavailable-component behavior

| Missing/unavailable component | Required behavior |
| --- | --- |
| Worker binary/package metadata | Nix build/evaluation fails; no broken unit publication |
| Runtime directory/socket creation | Worker unavailable; no fallback path/listener |
| User manager/runtime directory | User worker unavailable; no linger/session creation |
| Manifest/reference | Discovery may report bounded installation failure; mutation denied |
| Manifest incompatible/malformed/stale | Typed identity/materialisation failure; no backend mutation |
| systemd manager/bus | Manager layer unavailable; lifecycle denied |
| Podman adapter/API | Runtime layers partial/unavailable; operations requiring it fail closed |
| Journald read | Logs unavailable; unrelated status may remain partial |
| Mandatory audit write | Audited system operation denied before mutation |
| Polkit/agent | System protected operation denied/unavailable; no root retry |
| Activation lock | Activation and lifecycle submission fail closed |
| Invalid/ambiguous interlock | Matching mutation and activation blocked |
| Controller/network | No effect on local CLI/TUI/worker |

The module never repairs these failures by widening permissions, selecting
another manager/runtime, executing a shell command, rebuilding, clearing state,
or starting a controller.

## Tests required before implementation is complete

### Evaluation/build checks

NixOS and Home Manager checks cover:

- exact package installation and executable paths;
- exact service/socket names, arguments, dependency relationships, ownership,
  modes, tmpfiles, hardening, and restart limits;
- absence of network listeners, arbitrary path/argument options, controller
  enrollment, and store secrets;
- canonical same-directory manifest/descriptor generation, exact omitted-field
  digest preimages, shared generation identity, schema validation, typed
  user-runtime-relative endpoint resolution, and absence of build-time user UID;
- system/non-root-user/UID-0-user context separation;
- polkit action mapping and default policy relationships;
- activation script lock ordering/effect; and
- upgrade/rollback package-manifest compatibility failures.

Assertions test unknown users, duplicate workload/service IDs, empty metadata,
invalid/non-canonical UUIDv7 host identity, integrated system/user host-ID
mismatch, wrong target/UID, missing binary, incompatible schema, oversized
manifest, and forbidden policy weakening.

### Controlled runtime checks

Separate controlled tests prove observable effects for:

- system and non-root user socket activation from an initially stopped worker;
- exact socket/path ownership and denial to unauthorized peers;
- same-UID user access and cross-user denial;
- UID-0 user versus system manager/runtime separation;
- polkit observe/inspect/lifecycle allow, prompt, denial, stale PID, no agent,
  and unavailable authorizer;
- no TCP/UDP listener;
- worker crash/restart and new epoch;
- current manifest binding and foreign unit/container rejection;
- activation versus submission lock exclusion;
- every activation failure point before/after artifact reload/publication;
- interlock durable write, capacity, corruption, restart reconciliation, and
  administrator recovery proof;
- manager-wide dependency transaction quiescence;
- mandatory audit failure before mutation;
- Podman/journal/systemd partial/unavailable behavior;
- upgrade and rollback with idle, active, transitioning, and ambiguous workloads;
  and
- controller/network absence without local degradation.

Tests assert resulting files, manager state, worker response, journal audit, and
lack of backend mutation. Exit status alone is insufficient. Privileged/advisory
VM tests remain explicitly classified; required lightweight evaluation/build
checks run normally.

## Implementation issue split

After this design is approved, implementation remains split so each authority
boundary is independently reviewable:

1. **Shared manifest and endpoint publication** — schema, canonical serializer,
   digests, package compatibility, materialisation integration, and build checks.
2. **NixOS system integration** — package install, group, tmpfiles, socket,
   service, polkit, audit, activation locking, and system runtime tests.
3. **Home Manager user integration** — package install, user tmpfiles, socket,
   service, activation locking, non-root/UID-0 separation, and user runtime
   tests.
4. **Activation/interlock integration harness** — lock ordering, failure-point
   rollback, quiescence, durable records, reconciliation, and administrator
   recovery.
5. **Upgrade/rollback compatibility matrix** — package/API/manifest version
   combinations and coherent generation transitions.

[#241] owns `graft-worker`, protocol types, authorizer interface, adapters,
limits, reconciliation, and runtime semantics. [#243]/[#244] own the TUI.
[#245]/[#246] own controller transport/authentication/enrollment. None of these
issues may bypass the fixed Nix paths, contexts, action mapping, or activation
contract defined here.

## Security impact

This design adds a privileged host-native system worker and therefore does not
claim that installing a socket group is harmless. Its controls are:

- root-only system worker with explicit hardening and private writable state;
- private same-UID user workers without cross-user access;
- socket admission separated from typed per-operation authorization;
- no network listener/controller/credential surface;
- fixed Nix-issued endpoint, manager, runtime, manifest, lock, and interlock
  identities;
- deterministic secret-free manifests and endpoint descriptors;
- fail-closed audit, authorizer, manifest, manager, lock, interlock, and
  provenance checks;
- coherent upgrade/rollback under the same activation lock; and
- no raw backend, path, unit, socket, action-ID, argument, or recovery
  passthrough.

These are implementation acceptance criteria, not evidence that the current
release already exposes a worker.

## Linked work

- [#171] owns complete Quadlet search-path/drop-in drift detection;
- [#241] implements the local worker and API;
- [#243] and [#244] design and implement the TUI;
- [#245] and [#246] design and implement the optional controller; and
- [#250] and [#251] design and implement a minimal thin-node profile using the
  same local integration boundary.

[#171]: https://github.com/Patrick-Kappen/graft/issues/171
[#241]: https://github.com/Patrick-Kappen/graft/issues/241
[#243]: https://github.com/Patrick-Kappen/graft/issues/243
[#244]: https://github.com/Patrick-Kappen/graft/issues/244
[#245]: https://github.com/Patrick-Kappen/graft/issues/245
[#246]: https://github.com/Patrick-Kappen/graft/issues/246
[#250]: https://github.com/Patrick-Kappen/graft/issues/250
[#251]: https://github.com/Patrick-Kappen/graft/issues/251
