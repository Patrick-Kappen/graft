# Local worker and API contract

> **Status:** approved design for future implementation. The current release
> does not install a worker or expose runtime API commands. Lifecycle details
> remain in [#135], observability details in [#137], and service/socket
> installation details in [#242].

This document specifies the local worker boundary required by the
[Control-plane architecture](control-plane.md). It defines process authority,
manifest-based discovery, local transport, framing, version negotiation,
authorization, operation families, limits, interruption behavior, adapter
boundaries, and test obligations. It deliberately does not select Rust crates
or implement a service.

## Design objectives

The local worker must:

- let CLI and TUI clients use one typed contract;
- operate without a controller or network;
- control only Graft workloads in its own explicit manager scope;
- derive identity from a Nix-produced manifest rather than ambient discovery;
- adapt typed operations to authoritative host backends;
- bound every client-controlled allocation, stream, and expensive observation;
- fail closed on identity, scope, authorization, version, and mutation
  ambiguity;
- restart without losing declarative truth; and
- remain extensible to an authenticated controller without exposing its local
  socket remotely.

It must not become a desired-state database, reconciler, scheduler, generic
host API, or rebuild service.

## Process and authority model

Graft uses separate worker instances for separate manager authority:

| Worker context | Manager | Podman authority | Workloads served |
| --- | --- | --- | --- |
| System worker | system manager | rootful | manifest target `system` only |
| Non-root user worker | owning user manager | rootless | manifest target `user` for the same effective UID only |
| UID-0 user worker | root user manager | rootful | manifest target `user` for UID 0 only |

A worker has exactly one context for its lifetime. It cannot switch manager,
UID, target, or Podman connection in response to a client field. The system
worker does not discover user buses or impersonate accounts. A user worker does
not access the system manager or another user's runtime.

The same worker executable may support these deployment contexts, but the Nix
service definition fixes the effective context before accepting clients. Final
executable arguments and unit names belong to implementation and [#242].

### Socket activation and availability

Workers are designed for systemd socket activation:

- the system socket lives under a root-owned runtime directory below `/run`;
- a user socket lives below that account's `$XDG_RUNTIME_DIR`;
- the owning systemd manager passes one activated listening socket to one
  worker service (`Accept=no`), and the worker accepts every connection itself;
- worker credentials and configured context are fixed by that service;
- a user worker is available only while its user manager and runtime directory
  exist;
- Graft does not create a login session, enable linger, or start another user's
  manager to satisfy a client request; and
- controller loss has no effect on local socket activation.

The exact socket paths, modes, groups, service hardening, startup ordering, and
idle policy are decided in [#242]. Clients discover configured endpoints
through installed Graft policy, not an environment-provided arbitrary path for
privileged operations.

## Local transport and framing

The initial local transport is a Unix stream socket. Each application frame is:

```text
4-byte unsigned big-endian payload length
UTF-8 JSON payload of exactly that length
```

There is no newline delimiter, compression, file-descriptor passing, or raw
byte-stream operation. A zero length, an oversized declared length, invalid
UTF-8, trailing bytes inside the JSON value, duplicate JSON object keys,
non-finite number representation, or a value outside the negotiated schema
closes the connection after a bounded typed protocol error where safely
possible.

JSON is chosen for local inspectability, cross-language tooling, straightforward
schema publication, and low initial protocol complexity. Framing prevents log
or backend text from becoming protocol structure. The implementation must parse
into tagged types with unknown-field rejection; it must not dispatch free-form
method strings or deserialize arbitrary backend types.

### Initial hard limits

These are protocol maxima, not target values to allocate eagerly:

| Limit | Initial maximum |
| --- | ---: |
| Inbound frame | 64 KiB |
| Outbound frame | 256 KiB |
| Concurrent requests per connection | 32 |
| Active streams per connection | 8 |
| Connections per principal / worker | 16 / 128 |
| Incomplete handshakes per principal / worker | 4 / 32 |
| In-flight requests per principal / worker | 64 / 256 |
| Active streams per principal / worker | 16 / 64 |
| Buffered response bytes per principal / worker | 2 MiB / 16 MiB |
| Retained mutation records per principal / worker | 256 / 1,024 |
| Encoded retained lifecycle result | 32 KiB |
| Mutation identifier acceptance window | 10 minutes |
| Unacknowledged stream items per stream | 64 |
| Workloads in one list page | 256 |
| Historical log records requested per page | 1,000 |
| Encoded log message in one item | 64 KiB |
| Complete initial handshake | 5 seconds |
| Complete a partially received frame | 30 seconds |
| Unary client deadline | 60 seconds |
| Lifecycle client deadline | 5 minutes |

For local Unix peers, the initial principal key is the accepted peer UID; future
remote principals require their own authenticated key. Worker-wide accounting
is shared by the single `Accept=no` service across all connections.

The server advertises effective values no larger than these maxima. Nix policy
may lower them. A client cannot raise them. Values are versioned protocol
constants and require review before change. Limits are checked before reserving
memory or starting backend work. When a principal or worker budget is exhausted,
the worker rejects a new post-handshake request with `overloaded`; a connection
that cannot complete a bounded handshake is closed. Existing accepted work is
not evicted to admit newer work. Repeated admission failures are rate-limited
and audited without creating an unbounded audit queue.

A backend value too large for one item is truncated at a valid UTF-8 boundary
and carries original-byte-count and truncation metadata. Paginated responses
and streams must never aggregate unbounded backend output into one frame.

## Connection handshake and versioning

The first client frame must be `ClientHello`. The first server frame is either
`ServerHello` or a version/protocol error followed by connection close. No
operation is accepted before a successful handshake.

`ClientHello` contains only:

- protocol major version and an inclusive contiguous supported minor-version
  range;
- client component kind and software version;
- requested operation capabilities;
- requested effective limits no higher than protocol maxima; and
- one client-generated connection identifier for diagnostics, not authority.

`ServerHello` returns:

- selected protocol major and minor version;
- worker software version;
- fixed worker context: target, effective UID, and manager kind;
- supported operation capabilities;
- effective limits and deadline bounds;
- current manifest generation and availability state;
- current worker epoch and server wall-clock time; and
- one server-generated connection identifier for audit correlation.

Major versions must match exactly. The selected minor version is the highest
value in the intersection of the client's inclusive range and the server's
inclusive range for that major. An empty intersection fails negotiation.
Support for a higher minor does not imply support for every lower minor outside
the advertised contiguous range. Capabilities are explicit; absence
means unavailable, never implicit fallback. Software version strings are
diagnostic and do not replace protocol negotiation.

Unknown request variants, fields, enum values, or explicitly requested
unsupported capabilities fail closed. Read-only clients may continue when a
known backend capability is unavailable, but mutation cannot degrade to a
weaker operation.

## Typed frame envelope

After negotiation, every JSON payload is one tagged frame variant:

| Frame | Direction | Purpose |
| --- | --- | --- |
| `ClientHello` | client to server | Negotiate protocol, capabilities, and limits. |
| `ServerHello` | server to client | Fix worker context and negotiated contract. |
| `Request` | client to server | Start one typed unary or streaming operation. |
| `Response` | server to client | Return one unary success or typed error. |
| `StreamItem` | server to client | Return one sequenced bounded stream item. |
| `StreamAck` | client to server | Advance per-stream backpressure window. |
| `StreamEnd` | server to client | End with a typed reason and final cursor. |
| `Cancel` | client to server | Stop client interest in an in-flight operation. |

Every post-handshake frame includes the server-generated connection identifier
returned by `ServerHello` and a non-zero request identifier selected by the
client. The client-generated handshake identifier remains diagnostic and is not
echoed as the protocol connection identifier. Request identifiers are unsigned
integers encoded within JSON's interoperable integer range. Starting a new
`Request` with an identifier that is already active is a conflict; `StreamAck`
and `Cancel` reuse the active request identifier they target. Stream sequence
numbers start at one and increase by one within a request.

The protocol is request/response and server-streaming only in its first
version. Client-streaming and bidirectional arbitrary message exchange are not
available.

## Workload and generation identity

A client identifies a workload with a structured selector:

- explicit target: `system` or `user`;
- workload name;
- manifest generation identifier; and
- manifest-issued workload identifier.

For a user request, the fixed worker effective UID is also part of effective
identity even though clients do not choose it. Host identity is not
client-selectable on a local socket.

The request never accepts a source-unit name, generated service name, container
name or ID, cgroup path, journal unit, rootfs path, closure path, Podman socket,
D-Bus destination, or host filesystem path. The worker obtains those bindings
from its validated manifest and backend observations.

A client may list current workload selectors before acting. A mutation carrying
an old generation fails with `stale_manifest`; the worker does not silently
retarget it to a same-named workload in a newer generation. This prevents a
stale TUI confirmation or delayed controller request from acting on reused
identity.

## Materialisation manifest

Nix materialisation publishes one immutable manifest per worker context and an
atomically replaced configured reference to the current manifest. Final paths
and activation mechanics belong to [#242].

### Manifest envelope

The manifest contains:

- manifest schema major and minor version;
- producer Graft version;
- target and manager kind;
- generation identifier derived from the canonical manifest payload with the
  generation field omitted;
- creation/build provenance suitable for diagnostics;
- sorted workload records; and
- no secret values.

A user manifest does not claim an effective UID at build time. At runtime the
user worker binds it to its own effective UID. A manifest whose target or
manager kind does not match the fixed worker context is rejected completely.

### Workload record

Each record contains typed, mechanically derived data:

- workload name and manifest-issued identifier;
- explicit target;
- source filename identity and non-secret source/resolved digest;
- Quadlet source-unit name and expected generated-service name;
- configured container name;
- resolved lifecycle kind and whether startup intent is present;
- materialised rootfs and closure identities needed for read-only inspection;
- supported lifecycle, state, log, metric, event, and storage capabilities;
- references to Graft-owned generated artifacts needed for provenance checks;
- deployment enablement/materialisation state; and
- relationships needed to explain, but not independently mutate, dependencies.

All collections are sorted and duplicate workload, unit, service, container, or
manifest identifiers fail manifest validation. Fields are typed and schema
versioned. Arbitrary backend maps and extensible free-form operation data are
not permitted.

### Manifest validation and drift

Before mutation, the worker must establish that:

1. the configured manifest reference is trusted by installed Nix policy;
2. the manifest schema and producer are compatible;
3. target and manager kind match the fixed worker context;
4. generation and workload identifier match the request;
5. workload identity is unique;
6. the expected generated service is loaded from the expected Graft source;
7. no known Quadlet search-path shadow or conflicting foreign unit invalidates
   identity; and
8. the requested operation is present in manifest and worker capabilities.

Read-only inspection may return a partial snapshot describing a missing or
stale layer. Mutation fails before calling a backend when any required identity
check fails. Detailed shadow detection remains coordinated with [#171].

## Operation families

This document fixes operation shapes and boundaries. The
[Local lifecycle operations](lifecycle-operations.md) contract defines exact
state transitions, completion, concurrency, and interruption semantics; final
state, metric, log, and event fields are owned by [#137].

### Discovery and capabilities

- list paginated manifest workloads;
- get current manifest generation and validation state;
- get negotiated worker/backend capabilities and health; and
- inspect non-secret workload provenance.

Discovery returns only records visible under the connected worker context and
caller authorization. It never scans or adopts foreign units or containers.

### Lifecycle

- `up`;
- `down`; and
- `restart`.

Each lifecycle request contains only workload identity, an operation identifier,
the operation's origin worker epoch, and a client deadline within negotiated
limits. A fresh operation must use the epoch returned by the current
`ServerHello`. Re-presenting an operation after reconnect preserves its original
epoch so a restarted worker rejects it before backend submission. A separate
typed operation-result query may inspect that identifier and old epoch, but can
return only a retained result or `result_unknown`; it cannot submit lifecycle
work. Backend unit/action selection is worker-owned. There is no force, remove,
delete-data, arbitrary signal, kill, raw job mode, unit property, or Podman
option in the initial API.

### Status and inspection

- get one workload snapshot;
- list summarized workload snapshots; and
- optionally follow changes to typed snapshot layers.

Responses preserve declared, resolved, materialised, generated, manager,
runtime, and observed layers rather than collapsing them into one boolean.
Unavailable or unauthorized detail remains explicit.

### Logs

- query a bounded page relative to a typed journal cursor; and
- follow future records from a cursor.

The worker chooses the journal unit from manifest identity. Requests may contain
bounded time direction, count, and approved severity/filter fields defined by
[#137], but never a raw journal match expression. Every record identifies boot,
unit, timestamp, cursor, truncation, and redaction state.

### Metrics and storage

- get one bounded metric snapshot; and
- follow snapshots at an interval no faster than the server minimum.

The worker reads only the manifest-bound systemd cgroup and approved runtime
resources. Clients cannot submit cgroup or storage paths. Recursive or
potentially expensive storage accounting has separate capability and budget
limits and may return unavailable or partial states.

### Events

- follow typed manifest, manager, runtime, and worker availability changes.

Events report observations, not a reconciliation instruction. They include a
worker-epoch identifier and monotone sequence number within that epoch.

## Authorization

Authentication and authorization are distinct. Unix peer credentials identify
the connected process; host policy decides its allowed typed capabilities.
Credentials are captured once at accept time and attached to the connection.
Client JSON cannot override UID, GID, PID, security label, or worker context.

### User worker

Initial user-worker policy is deliberately narrow:

- the socket is private to the owning account;
- peer UID must equal worker effective UID;
- the account may use approved read and lifecycle operations for manifest-bound
  workloads in that worker context; and
- UID 0 is treated as rootful authority, not as safer user scope.

Access by another account, including administrative cross-user access, is not
part of the first local API.

### System worker

Connecting to the system socket requires explicit Nix-installed access policy.
Connection permission alone does not grant every operation.

At minimum, policy separates:

- system discovery/status/metrics;
- potentially sensitive system logs and full inspection; and
- system lifecycle mutation.

System lifecycle is dangerous and requires explicit per-operation
authorization. The authorization subject is derived from accepted peer
credentials and current process identity using an approved host mechanism such
as polkit. A stale PID, changed credentials, vanished subject, denied prompt, or
unavailable authorizer fails before backend mutation.

The final action identifiers, group policy, non-interactive behavior, prompt
agent behavior, and root bypass rules belong to [#242]. They must not be
client-selected strings.

## Request execution and deadlines

A request moves through fixed phases:

```text
parse and bound
  → authenticate connection
  → authorize typed operation
  → emit denial audit and return, or emit authorized-attempt audit
  → load and validate current manifest
  → bind workload/backend identity
  → validate capability and preconditions
  → submit typed backend operation
  → emit submission audit
  → observe terminal or accepted state
  → emit outcome audit and return result
```

Validation order must avoid leaking unauthorized workload existence. Detailed
errors are returned only after observation authorization is established. A
mutation is not submitted unless the required denial or authorized-attempt audit
record has been accepted by the configured bounded audit sink. An unavailable
or saturated sink therefore fails system mutation closed. Any observation that
host policy requires to be audited follows the same rule.

Client deadlines bound how long the worker waits and how long ordinary response
state is retained. Mutation duplicate records and tombstones are the explicit
exception: they follow the deadline-independent acceptance-window retention
below. Deadlines do not rewrite systemd's workload timeout. Once systemd accepts
a job, client cancellation or disconnect does not imply rollback, stop, or an
opposite lifecycle action.

## Mutation identity, concurrency, and interruption

Every lifecycle request carries a client-generated canonical UUIDv7 operation
identifier plus the worker epoch in which it originated. The embedded timestamp
must be no more than one minute ahead of server receive time and no more than ten
minutes old. They provide correlation, duplicate control, expiry enforcement,
and stale-epoch rejection, not authorization.

Within one worker epoch:

- the first accepted operation identifier owns one immutable request payload;
- reuse with the identical payload observes the same in-flight or completed
  result while retained;
- reuse with a different payload fails as a conflict;
- an unknown expired identifier fails with `operation_id_expired` and never
  starts work, while a known identical in-flight or retained request may still
  join its record;
- at most one lifecycle mutation may be in flight per workload;
- concurrent read operations remain permitted within connection and backend
  limits; and
- an accepted identifier's request remains while in flight, and its bounded
  result or tombstone remains until both terminal completion and its complete
  ten-minute acceptance window have passed; and
- retained mutation records are capped at 256 per principal and 1,024 per
  worker, with overload rejection instead of early eviction.

A worker restart creates a new epoch and loses this operational cache. Exactly
once mutation across restart is impossible without persistent hidden state and
is not promised. A disconnect, deadline, cancellation, or worker crash after
backend submission may therefore return `result_unknown`. The client must query
the current workload state and must not blindly replay a non-idempotent
operation. The [local lifecycle contract](lifecycle-operations.md) defines
which observed states make a new `up`, `down`, or `restart` safe.

## Streaming, cursors, and backpressure

Each stream returns:

- worker epoch;
- request identifier;
- monotone sequence number;
- backend timestamp and observation timestamp where applicable;
- freshness or lag metadata;
- typed item payload; and
- resumable cursor when the backend supports one.

The client acknowledges the highest contiguous sequence it consumed. The worker
never keeps more than the negotiated unacknowledged-item window. A slow client
causes a typed `slow_consumer` stream end rather than unbounded buffering.

Worker-local event and metric sequences do not survive worker restart. A new
epoch makes the gap explicit. Journal cursors may survive the worker but can
expire through journal rotation; resumption then returns `cursor_expired` with
an approved recovery position, never silent omission.

Cancellation ends client interest and releases buffers. It does not reverse an
accepted lifecycle operation. Every stream ends with one typed reason such as
`completed`, `cancelled`, `deadline`, `cursor_expired`, `slow_consumer`,
`manifest_changed`, `backend_unavailable`, or `worker_shutdown`.

## Typed errors

Errors have a stable code, safe summary, retry classification, operation phase,
worker epoch, request identifier, and optional bounded structured details.
Backend text is redacted, size-limited data and never an error code or protocol
field.

Initial error families include:

| Family | Examples |
| --- | --- |
| Protocol | malformed frame, unsupported version, unknown field, limit exceeded |
| Authentication | missing or invalid peer credentials, worker-context mismatch |
| Authorization | observation denied, logs denied, lifecycle denied, authorizer unavailable |
| Identity | unknown workload, wrong scope, stale generation, ambiguous identity, manifest invalid |
| Materialisation | source missing, service missing, shadowed unit, incompatible producer |
| Backend | manager unavailable, runtime unavailable, journal unavailable, metrics unavailable |
| State | conflict, operation in progress, unsupported lifecycle, precondition failed |
| Stream | cursor expired, slow consumer, manifest changed, gap after worker restart |
| Interruption | cancelled, deadline, worker shutdown, result unknown |

Retry guidance is one of `never`, `after_state_refresh`, `after_authorization`,
`after_backend_recovery`, or `same_request_with_backoff`. It does not authorize
a client to retry mutation automatically.

## Backend adapters

The worker core depends on typed internal adapter traits rather than commands or
backend response maps.

### Manifest adapter

- loads only the Nix-configured current reference;
- validates schema, digest, ordering, uniqueness, and worker context;
- atomically publishes a new in-memory generation; and
- notifies streams when the generation changes.

### systemd adapter

- connects only to the worker context's manager;
- maps manifest-issued service identity to typed unit state and jobs;
- submits only approved start/stop/restart behavior from [#135];
- reports invocation, result, cgroup, and lifecycle changes; and
- never exposes arbitrary D-Bus methods or unit names to clients.

### journald adapter

- selects records using worker-owned host/scope/unit identity;
- supports typed bounded query and cursor follow;
- preserves record boundaries and truncation metadata; and
- treats every field as untrusted display data.

### Podman adapter

- connects only to the fixed context's runtime;
- verifies container identity against manifest and generated-service evidence;
- returns typed inspect, state, stats, and approved storage metadata; and
- does not accept client-provided sockets, names, IDs, filters, or options.

### cgroup adapter

- accepts only the cgroup reported for the manifest-bound systemd service;
- verifies it remains below the expected manager hierarchy;
- returns typed bounded accounting values; and
- does not follow a client or Podman supplied arbitrary path.

### Storage adapter

- accounts only manifest-bound rootfs, writable overlay, and managed-volume
  resources approved by [#137];
- distinguishes immutable store size, writable layer size, volume size,
  unavailable data, and shared/deduplicated bytes;
- applies time, entry, depth, and byte-accounting budgets; and
- never recursively walks a client path.

Each adapter requires deterministic mocks, negative contract tests, and a
controlled integration harness. Crate/library selection is an implementation
decision that must preserve these boundaries.

## Worker state and restart recovery

Permitted mutable state is bounded and operational:

- validated current manifest snapshot;
- short-lived backend observations;
- in-flight operations;
- retained duplicate-operation results;
- active stream windows and cursors;
- rate-limit counters; and
- current worker epoch.

No workload desired state or configuration is written by the worker. Structured
audit records go to the owning journal or another Nix-approved sink; they are
not used to reconstruct intent.

On restart the worker:

1. creates a new epoch;
2. reloads and validates the configured manifest;
3. reconnects only its fixed-context backends;
4. reconstructs observations;
5. reports prior in-flight results as unknown if clients ask with old epoch
   context; and
6. requires streams to resume through a backend cursor or acknowledge a gap.

Running workloads continue under systemd and Podman while the worker is absent.

## Audit contract

Every system mutation and every denied mutation emits structured audit events.
Authorization denial is recorded before the denial is returned. An authorized
attempt is recorded before backend submission, followed by separate submission
and outcome or result-unknown records. User-worker mutation auditing follows the
same schema in the user's journal. Approved sensitive observations such as
system logs may also require audit under host policy. Required initial audit
records use a bounded sink and fail the operation closed when that sink cannot
accept them; later audit failure cannot erase or roll back an accepted backend
operation and is surfaced as degraded worker health.

Audit fields include:

- timestamp, worker epoch, connection and operation identifiers;
- peer UID/GID/PID and available authenticated subject metadata;
- fixed worker context and workload identity;
- manifest generation;
- typed operation and authorization result;
- backend-submission and terminal/result-unknown state; and
- safe typed error code.

Audit events never include secret environment values, full log messages,
credentials, arbitrary backend payloads, or unbounded paths. Client-supplied
labels are not promoted into trusted audit fields.

## Controller extension boundary

The local socket is never bound to TCP, forwarded, or exposed directly to a
controller. [#245] defines a separate mutually authenticated remote transport.
That transport may translate authenticated, authorized remote requests into the
same internal typed dispatcher, but the local worker still:

- binds its fixed manager context;
- validates current manifest generation and workload identity;
- applies local capability and Nix policy;
- authorizes the remote principal for the typed operation;
- enforces local limits and concurrency; and
- emits the local audit result.

Remote claims cannot manufacture local Unix peer credentials or bypass local
validation. Controller connectivity is optional and cannot be required for
local CLI/TUI operations.

## Security impact and invariants

This design concretizes the authority expansion approved in the umbrella
architecture. It affects:

- **GRAFT-TM-01:** protocol schemas and tagged operations reject unknown or
  unsupported explicit intent;
- **GRAFT-TM-02:** operation types and adapters make raw backend passthrough
  unavailable;
- **GRAFT-TM-03:** length framing and typed parsing prevent protocol-structure
  injection; CLI and TUI clients must escape or visibly encode every untrusted
  backend string before terminal rendering, while preserving the original typed
  value only for non-terminal machine output;
- **GRAFT-TM-04:** manifest generation and workload identifiers bind requests to
  an explicit materialised source set;
- **GRAFT-TM-05:** fixed worker context and UID preserve system/rootful,
  user/rootless, and root-owned user/rootful authority distinctions;
- **GRAFT-TM-06:** runtime operations do not alter declarative startup intent;
- **GRAFT-TM-07:** clients cannot select rootfs, closure, package, or store paths;
- **GRAFT-TM-09:** the initial mutation API has no persistent-data or foreign-unit
  removal operation; and
- **GRAFT-TM-13:** lifecycle requests have no hardening-relaxation fields.

Capability classification remains:

| Capability | Class | Availability |
| --- | --- | --- |
| Own-user observation and lifecycle | First-class | Planned in #241 after this contract, #135, #137, and #242 are approved |
| System observation | Dangerous | Planned with separate host-policy authorization |
| System lifecycle mutation | Dangerous | Planned with per-operation authorization |
| Remote controller request | Dangerous | Planned in #245/#246 |
| TOML authoring/rebuild activation | Dangerous | Deferred |
| Raw backend or arbitrary RPC passthrough | Forbidden | Not applicable |

The design adds host observation and mutation authority to Graft clients; it
does not widen workload TOML authority. Implementation requires negative tests
for unknown protocol intent, wrong scope, UID-0 user context, stale generation,
identity reuse, unauthorized observation/mutation, malformed framing, backend
text injection, limits, concurrent mutation, interruption, and worker restart.

## Implementation slices

Implementation should remain reviewable and testable in this order:

1. Publish protocol and manifest Rust types plus generated schemas and malformed
   input tests, without a listening service.
2. Materialise deterministic system and user manifests through Nix with schema,
   collision, target, and generation tests.
3. Add a socket-activated read-only worker with manifest and mock systemd
   adapters.
4. Add typed status/inspect and bounded logs/metrics after [#137] approval.
5. Add user lifecycle operations after [#135] approval.
6. Add system observation and per-operation lifecycle authorization after [#242]
   approval.
7. Add restart, interruption, concurrency, audit, and failure-injection tests.
8. Integrate CLI commands, then the TUI, against the same client library.
9. Add the separate remote protocol only after [#245] approval.

No slice may add a generic escape hatch while waiting for a later typed
operation.

## Open details delegated to linked designs

- [Local lifecycle operations](lifecycle-operations.md): exact `up`, `down`, and
  `restart` state machines and idempotent results;
- [#137]: snapshot fields, metric formulas, log filters, cursor recovery, and
  storage accounting semantics;
- [#171]: complete search-path shadow and foreign-override detection;
- [#241]: implementation crate boundaries and selected backend libraries;
- [#242]: concrete paths, units, users/groups, socket modes, polkit actions,
  hardening, idle policy, and upgrade ordering;
- [#245]: enrollment, mutual authentication, remote replay protection, and
  controller authorization.

[#135]: https://github.com/Patrick-Kappen/graft/issues/135
[#137]: https://github.com/Patrick-Kappen/graft/issues/137
[#171]: https://github.com/Patrick-Kappen/graft/issues/171
[#241]: https://github.com/Patrick-Kappen/graft/issues/241
[#242]: https://github.com/Patrick-Kappen/graft/issues/242
[#245]: https://github.com/Patrick-Kappen/graft/issues/245
