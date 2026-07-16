# Runtime observability contract

> **Status:** approved design for future implementation. The current release
> does not expose `graft status`, `graft inspect`, or `graft logs`. This contract
> defines the typed read-only surface consumed by the future CLI, TUI, and
> controller through the [local worker API](worker-api.md).

This document defines workload identity, layered snapshots, lifecycle evidence,
logs, metrics, storage accounting, events, freshness, diagnostics,
authorization, redaction, pagination, and client exit behavior. It complements
the mutation semantics in [Local lifecycle operations](lifecycle-operations.md)
and does not create another lifecycle or desired-state authority.

## Objectives

The observability surface must:

- let an operator explain why a workload did not start without manually joining
  Nix, Quadlet, systemd, Podman, cgroup, and journal output;
- expose the same typed model to CLI, TUI, and controller clients;
- preserve layer boundaries rather than collapse state into one boolean;
- report unavailable, stale, unauthorized, unsupported, and not-applicable data
  explicitly;
- correlate finite jobs and retained setup work without inferring success from
  inactive state;
- bind every observation to host, scope, manifest, worker, manager, and boot
  identity;
- bound every page, record, sample, cache, filesystem traversal, and stream;
- redact secrets and sensitive host details before serialization; and
- remain read-only and locally useful without a controller.

It must not become a metrics database, log archive, generic journal query API,
raw backend inspect API, runtime scanner, desired-state database, or lifecycle
escape hatch.

## Client surface

The initial commands are:

```text
graft status [--scope system|user] [<workload>]
graft inspect [--scope system|user] <workload>
graft logs [--scope system|user] <workload>
```

`graft logs` supports typed bounded query and follow modes defined below. Scope
selection and omission follow the [lifecycle selection contract](lifecycle-operations.md#selection-and-authority): discovery authorization happens first,
an unqualified uniquely visible name may resolve, and an ambiguous name requires
`--scope`.

The TUI and controller consume the same worker operations and cannot obtain
additional raw backend fields.

## Observation envelope

Every snapshot, page, event, metric sample, log record, and diagnostic carries or
inherits one bounded typed observation envelope:

- Nix-configured non-secret host identity when authorized;
- target `system` or `user`;
- effective UID for user scope;
- complete structured workload selector, including manifest generation, when the
  observation is workload-specific;
- host boot ID when applicable and authorized;
- worker epoch;
- manager epoch from the worker contract when applicable and correlatable;
- server observation time as UTC Unix epoch milliseconds;
- source monotonic time where the source provides it;
- snapshot revision or stream sequence where applicable;
- authorization/redaction class; and
- freshness/completeness metadata.

Clients cannot set returned host, effective UID, boot, worker epoch, manager
epoch, unit, container, cgroup, or source-path identity. The worker obtains them
from its fixed context, validated manifest, and authoritative backends. A typed
authorized log boot selector constrains a query but cannot override the returned
envelope or unit/scope binding.

Host identity is an installed Graft identifier, not an automatically disclosed
raw machine ID or hostname. The host boot ID is typed correlation metadata and
is returned only where authorized; it is never accepted as authority from a
client.

### Time representation

Absolute timestamps are UTC Unix epoch milliseconds encoded as JSON integers.
Durations and cumulative counters use explicit units in field names or typed
unit enums. Freshness age is computed from the worker's monotonic clock and is
not made negative by wall-clock correction.

A missing source timestamp remains absent. The worker never substitutes its
observation time as if it were backend event time.

## Snapshot consistency

A workload snapshot is a bounded observation assembled from several authorities;
it is not an atomic distributed transaction. The worker:

1. captures the current manifest generation and fixed identities;
2. captures worker, manager, and boot epochs;
3. queries each authorized layer with its own bounded deadline;
4. rechecks manifest/manager identity after collection;
5. marks layers stale or the whole snapshot generation-changed when identity
   changed during collection; and
6. never combines old evidence with a replacement identity as current state.

Each layer has independent source and observation timestamps. `complete` means
all applicable, authorized, supported layers returned within their bounds; it
does not claim simultaneous backend sampling.

Snapshot completeness is exactly one of:

- `complete`;
- `partial`; or
- `unavailable`.

A partial snapshot remains useful and preserves every layer's individual status.
An unavailable snapshot contains only safely authorized identity and typed
failure metadata.

## Layer availability

Every layer is wrapped in exactly one availability state:

| State | Meaning |
| --- | --- |
| `fresh` | Successfully observed within that layer's freshness policy. |
| `stale` | Evidence exists but identity, age, or collection race prevents treating it as current. |
| `unavailable` | Applicable source could not be reached or queried within bounds. |
| `unauthorized` | Current policy does not permit this layer or field group. |
| `unsupported` | Worker/backend version does not implement this typed capability. |
| `not_applicable` | The layer has no meaning for this workload state, such as runtime for a disabled workload. |

These states are tagged alternatives. A layer cannot simultaneously be `fresh`
and `unavailable`, and clients must not infer a zero/false/empty value from any
non-fresh state.

## Layered workload snapshot

The full snapshot retains seven layers plus an explicitly derived summary.

### Declared layer

Derived from the validated non-secret manifest record:

- workload name and source identity/digest;
- explicit target;
- `deploy.enable`;
- startup activation intent;
- lifecycle kind;
- typed dependency identities; and
- declared capability classes.

It contains no TOML text, secret value, environment value, raw command line, or
absolute private source path.

### Resolved layer

Derived mechanically from resolver evidence published by Nix:

- resolved configuration digest;
- resolver/schema producer version;
- selected capability summary;
- dependency graph digest;
- validation status; and
- non-secret warning/diagnostic codes.

The worker does not rerun ambient TOML resolution to reconstruct this layer.

### Materialised layer

Derived from the current manifest and installed artifacts:

- manifest generation and schema version;
- enabled/disabled/materialisation state;
- rootfs identity;
- closure identity;
- generated-artifact identities;
- producer/build provenance digest; and
- current-reference validation state.

A disabled workload is visible with `deploy.enable = false`; generated, manager,
and runtime layers are normally `not_applicable` and no missing-unit diagnostic
is invented.

### Generated layer

Derived from Graft-owned Quadlet source and manager generator evidence:

- expected Quadlet source identity;
- expected generated service identity;
- source present/missing;
- generator success/failure/unknown;
- loaded source/provenance match;
- shadow/conflict state; and
- compatible producer/runtime version evidence.

It never returns arbitrary drop-ins, raw unit text, full search paths, or a raw
systemd property map. Complete search-path and override detection remains owned
by [#171].

### Manager layer

Derived from the fixed systemd manager:

- manager epoch and unit identity;
- load state;
- normalized lifecycle state;
- typed active state and approved substate;
- queued job identity/type/state when present;
- invocation identity when present;
- service result;
- execution result/exit code/signal when available;
- restart count;
- lifecycle start, active-enter, inactive-enter, and completion timestamps when
  available;
- attributed cgroup identity; and
- activation-interlock state relevant to this workload.

Raw D-Bus variants and properties remain adapter-internal. Job and invocation
identities are never compared across manager epochs.

### Runtime layer

Derived from the fixed-context Podman adapter and manager attribution:

- expected and observed container identity match state;
- presence and runtime state;
- rootful/rootless execution class;
- container ID in an authorized bounded representation;
- runtime PID when present;
- manager-bound cgroup match;
- created/started/exited timestamps when available;
- typed exit result when attributable; and
- typed runtime health state only when supported by approved intent.

The worker never adopts a same-named foreign container. Raw Podman inspect JSON,
labels, annotations, mounts, environment, arguments, engine paths, and arbitrary
status strings are not returned.

### Observed layer

Contains worker-derived evidence that cannot be represented as one backend
layer:

- cross-layer identity/provenance match;
- snapshot collection interval;
- layer lag/freshness;
- runtime-versus-manager mismatch;
- manifest change during collection;
- manager epoch change during collection;
- interlock or backend degradation; and
- bounded typed diagnostics.

This layer describes observations, not desired state.

## Summary classification

The summary is a deterministic convenience derived from visible layer tags. It
is not a new authority and always links to the evidence layers that produced it.

Initial summary values are:

- `disabled`;
- `not_materialised`;
- `invalid_materialisation`;
- `manager_unavailable`;
- `unit_missing`;
- `unit_shadowed`;
- `inactive`;
- `activating`;
- `active`;
- `active_exited`;
- `deactivating`;
- `failed`;
- `runtime_mismatch`; and
- `unknown`.

Precedence is fail-closed:

1. disabled intent;
2. invalid/missing/stale materialisation or provenance;
3. manager unavailable/identity change;
4. loaded-unit failure or transition;
5. runtime identity/cgroup mismatch;
6. expected lifecycle success or quiescent state; and
7. unknown when evidence is insufficient.

`active` means manager `active-running` for a long-running workload with no
known identity mismatch. It does not imply application health. `active_exited`
is the successful retained setup manager state. A workload can be `failed` while
its runtime layer is still present or while a replacement invocation is active;
the detailed result remains authoritative.

## Lifecycle-specific evidence

### Long-running

Successful current operation evidence requires:

- expected manager unit and manager epoch;
- terminal start/restart manager job where one existed;
- attributed current invocation;
- manager `active-running`; and
- no known runtime identity/cgroup mismatch.

Uptime starts at the attributed invocation's active/start monotonic evidence, not
at worker startup or container discovery.

### Setup

Successful retained setup evidence requires:

- expected invocation;
- successful manager/execution result; and
- manager `active-exited`.

A running container, runtime PID, CPU rate, or health state is normally
`not_applicable` after successful setup completion.

### Finite job

Current `inactive` state never proves historical job success. The snapshot may
report a last execution result only when systemd/journal/runtime evidence can be
bound to:

- boot ID;
- manager epoch;
- unit and invocation;
- lifecycle kind;
- start/completion timestamps; and
- exit code, signal, or typed manager result.

After evidence loss, manager restart, journal rotation, or ambiguous attribution,
the last result becomes `stale` or `unavailable`; it is not reconstructed from
inactive state. The worker has no historical job-result database.

`Result=protocol` is preserved as typed `result_protocol`. Other initial manager
failure codes include start timeout, stop timeout, exit code, signal, core dump,
watchdog, start limit, condition failure, dependency failure, resources, and
unknown/unsupported result. Unsupported future values remain visible as bounded
unknown enum data, not treated as success.

## Lifecycle-operation result relationship

Retained mutation results from the lifecycle contract remain a separate typed
operation-result API. A status snapshot may reference a currently retained
operation ID/result when authorized, but it never embeds or fabricates a
mutation terminal response.

After worker restart, status may explain current manager/runtime state while an
old operation-result query independently returns
`OperationResultUnavailable(cache_lost)`. These statements are not
contradictory: current state does not prove exactly what happened to the old
request.

## Status operations

### List status

The worker returns a paginated list of bounded summary snapshots. The request
contains:

- optional explicit scope handled by endpoint selection;
- page size no greater than the worker maximum;
- opaque worker-issued page cursor; and
- optional fixed typed summary/lifecycle filters advertised by capability.

There is no arbitrary filter expression, sort key, unit name, container name, or
backend query. Initial ordering is workload name ascending within the fixed
worker scope. A normal client aggregates separate authorized endpoints and sorts
by scope then workload name.

A page cursor is bound to worker epoch, principal, manifest generation, filter,
and ordering. Mismatch or expiry returns `page_cursor_expired`; the client
restarts listing. The cursor is not a filesystem/backend cursor and exposes no
secret content.

### Get status

Returns one summary plus the applicable layered evidence needed to explain it.
It is less detailed than inspect but does not use a different state model.

### Follow status

Returns typed snapshot-change events. It does not poll by allowing the client to
submit arbitrary intervals. The worker coalesces backend bursts within its
advertised bounded policy and clients recover gaps by fetching a fresh snapshot.

## Inspect operation

`inspect` returns one full authorized layered snapshot plus:

- negotiated worker/backend capabilities;
- supported lifecycle/observability operations;
- metric/storage samples explicitly requested within bounds;
- provenance validation summary;
- typed diagnostics; and
- layer-specific collection duration/freshness.

Inspect remains allowlisted. It never includes:

- raw TOML/resolved JSON/Quadlet/unit text;
- raw D-Bus or Podman maps;
- environment or credential values;
- command arguments;
- host absolute source/rootfs/closure/cgroup/storage paths;
- arbitrary labels/annotations/journal fields; or
- unrestricted dependency-unit details.

Clients needing machine-readable output receive the typed snapshot schema, not a
backend debug dump.

## Logs

The worker exposes bounded `QueryLogs` and `FollowLogs` operations. It always
selects the journal using fixed host/scope and manifest-bound generated-service
identity.

### Log request

A query may contain only:

- structured workload selector;
- direction `forward` or `backward`;
- record count from 1 through 1,000;
- optional `since_ms` and `until_ms` UTC Unix epoch milliseconds;
- approved priority set from `emerg` through `debug`;
- boot selection `current` or one explicit boot ID already visible under policy;
- optional journal cursor; and
- caller deadline within negotiated bounds.

`since_ms > until_ms`, empty priority set, zero count, oversized cursor, invalid
UTF-8, or malformed boot identity is rejected. A journal cursor is bounded to 4
KiB and treated as untrusted opaque data. The worker always reapplies the fixed
unit/boot/scope filters after seeking; a cursor can choose a position, never
broaden records. Raw journal matches and arbitrary fields are forbidden.

If both cursor and time bounds are present, the cursor establishes position and
time bounds still filter records. Direction determines iteration, while returned
records remain explicitly sequenced in delivery order.

### Log record

Each record contains:

- observation envelope identity;
- journal boot ID;
- manager epoch observed for the unit binding when correlatable, otherwise
  explicit `unavailable`;
- journal realtime timestamp;
- journal monotonic timestamp when available;
- typed priority;
- approved source/stream classification;
- bounded UTF-8 message;
- journal cursor;
- original byte count;
- truncation flag; and
- redaction flag/classification.

For a historical boot, the worker normally cannot reconstruct the complete
manager epoch because journald does not retain its D-Bus bus UUID and unique
systemd owner. Such records expose manager epoch as `unavailable` unless another
approved authoritative source proves every component; the worker never combines
current-manager components with a historical boot.

Message content is untrusted display data. The worker preserves record
boundaries, truncates at a valid UTF-8 boundary to the protocol limit, and never
promotes message text into identity, severity, error code, or terminal structure.
CLI/TUI clients visibly encode terminal control characters.

### Pagination and cursor recovery

A bounded page returns first/last cursor and whether more records were observed
within query bounds. Empty result is successful.

Cursor outcomes are:

- `valid`;
- `cursor_expired` after rotation/vacuum;
- `boot_mismatch`;
- `unit_binding_changed` after manifest/manager identity change;
- `unauthorized`; or
- `journal_unavailable`.

The worker may return an authorized recovery position for expiry but never
silently skips a gap. Cursor validity does not imply retention guarantees beyond
journald policy.

### Follow logs

Follow starts from an explicit cursor or the worker-defined current tail. It uses
worker stream sequence/backpressure. Journal rotation, boot change, manager
binding change, slow consumer, worker shutdown, or authorization change ends the
stream with a typed reason. Reconnect uses the last journal cursor when still
valid; request-local stream sequence alone is not a journal resume cursor.

The worker does not copy logs into its own persistence layer.

## Metrics

Metric values use a common tagged sample:

- metric name from a fixed enum;
- numeric value using an integer where the source is integral;
- explicit unit;
- source enum;
- source and observation timestamps;
- monotonic sample interval for rates;
- age/freshness;
- availability; and
- optional bounded explanation code.

Absent, unsupported, unauthorized, overflowed, reset, or unavailable values are
not encoded as zero. Counter decreases caused by manager/runtime epoch or
invocation change reset the rate baseline and produce `counter_reset`, never a
negative rate.

### Fast metrics

Initial metrics are:

| Metric | Unit | Primary source |
| --- | --- | --- |
| CPU cumulative usage | nanoseconds | manifest-bound systemd cgroup `cpu.stat` |
| CPU rate | nanoseconds per second over sample interval | difference of same cgroup counter |
| Memory current | bytes | cgroup `memory.current` |
| Memory peak | bytes | cgroup `memory.peak` when supported |
| Memory limit | bytes or `unbounded` | cgroup `memory.max` |
| PIDs current | count | cgroup `pids.current` |
| PIDs limit | count or `unbounded` | cgroup `pids.max` |
| Manager restart count | count | typed systemd service property within manager epoch |
| Invocation uptime | milliseconds | attributed invocation monotonic timestamp |
| Runtime PID | PID | systemd/Podman identity-correlated runtime evidence |

The cgroup adapter accepts only the path reported for the manifest-bound service
and verifies it remains under the fixed manager hierarchy. Client paths and
runtime-supplied arbitrary paths are rejected.

Podman stats may provide separately named supplemental metrics when the adapter
can prove container identity and source semantics. They never silently replace a
missing cgroup metric with a value of different scope.

### Metric snapshots and follow

A snapshot is bounded by metric set and workload. Follow intervals are
server-advertised and cannot be faster than two seconds in version 1. Each
principal may have only the negotiated active metric streams. Samples are
coalesced under backpressure; skipped intervals produce an explicit gap/sample
count.

Rates require two valid samples from the same boot, manager epoch, cgroup, and
invocation. The first sample reports cumulative value with rate
`not_applicable`.

## Storage accounting

Storage is a separate expensive capability, not an ordinary fast metric. The
worker reports categories independently:

- immutable rootfs/closure logical bytes;
- immutable closure path count;
- writable container layer bytes;
- named managed-volume bytes;
- anonymous ephemeral-volume bytes when attributable;
- bind-source bytes as `unsupported` in version 1;
- shared/deduplicated bytes when a backend can prove them; and
- aggregate logical bytes with explicit double-counting semantics.

### Immutable closure size

Logical closure bytes use Nix metadata for the manifest-bound closure. They are
not described as physical per-container disk consumption because store paths may
be shared and filesystem compression/deduplication may differ. Physical unique
bytes are `unsupported` unless an approved backend can calculate them without
misattribution.

### Writable and volume size

The worker selects writable layer and volume identities from manifest/runtime
attribution. Clients never provide paths. Named/external volumes preserve their
ownership classification; anonymous volumes remain ephemeral and may disappear
with generated cleanup.

Bind-source accounting is `unsupported` in version 1. Lexically validated bind
sources may contain symlinks and nested mount points, so safe size accounting
requires a separately reviewed host-aware root-resolution, symlink, mount,
filesystem-boundary, and race contract. Policy cannot enable traversal until
that contract exists.

### Budgets and caching

One storage query has fixed version-1 maxima:

- 5-second monotonic execution budget;
- 100,000 visited entries;
- depth 64;
- 256 resources; and
- one active storage query per principal.

Policy may lower these maxima. Budget exhaustion returns partial per-category
results plus `storage_budget_exceeded`; it does not discard completed categories.
Results may be cached for up to five minutes with exact sample time and age.
Cache keys include host/scope, principal authorization class, manifest
generation, workload, resource identity, and relevant backend epoch.

Storage values are not automatically included in high-frequency metric follow.
A client explicitly refreshes storage or accepts the visible cached age.

## Health and readiness

The model distinguishes:

- `manager_ready`;
- `runtime_running`; and
- `application_health`.

For the initial long-running lifecycle, conmon/systemd notify evidence may make
`manager_ready = ready`. It does not prove application-level readiness beyond
the materialised contract. `runtime_running` reflects identity-correlated
runtime state. `application_health` is `unsupported` until typed health intent
from [#146] is approved and implemented.

For completed setup and finite job workloads, runtime-running and application
health are normally `not_applicable`. Unsupported or absent health is never
reported as healthy.

## Events

The worker emits only fixed event variants:

- `manifest_changed`;
- `materialisation_changed`;
- `manager_state_changed`;
- `manager_job_changed`;
- `invocation_changed`;
- `runtime_state_changed`;
- `backend_availability_changed`;
- `interlock_changed`;
- `authorization_changed`; and
- `gap`.

Every event contains worker epoch, request-local stream sequence, observation
time, workload identity when applicable, source enum/time, snapshot revision,
and a bounded typed payload.

Stream sequence orders delivery within one authorized stream request, not
causality across Nix, systemd, Podman, journald, and cgroups. It starts at one
and increases by one as required by the worker API. Authorization filtering does
not create hidden sequence holes because each stream has its own sequence.
Source timestamps remain visible.

### Snapshot revision

Snapshot revision is monotone within one worker epoch and workload selector. It
changes when the worker publishes a semantically different authorized snapshot.
It is not durable, globally ordered, or a resume cursor after worker restart.

### Stream gaps and recovery

A client acknowledges consumed sequences under the worker API. Buffer overflow,
slow consumer, backend loss, manifest replacement, manager epoch change, worker
shutdown, authorization change, or worker restart is explicit.

A stream-local `gap` contains last contiguous sequence and bounded reason. After
a gap the client fetches a fresh snapshot before treating later deltas as
complete. Worker restart creates a new epoch; reconnect cannot resume from the
old request-local sequence. Journal follow may separately resume by journal
cursor.

Events and revisions are not persisted as history. Fast metric samples use the
separately authorized metric-follow operation and are never delivered through a
generic event/status stream.

## Diagnostics

Snapshots and terminal read responses may include bounded typed diagnostics:

- layer;
- code;
- severity `info`, `warning`, or `error`;
- safe lowercase summary;
- bounded evidence identity;
- retry classification;
- suggested next typed client action when one exists; and
- redaction state.

Initial diagnostic codes include:

- `workload_disabled`;
- `manifest_stale`;
- `quadlet_source_missing`;
- `generator_failed`;
- `generated_unit_missing`;
- `unit_shadowed`;
- `manager_unavailable`;
- `manager_epoch_changed`;
- `runtime_unavailable`;
- `runtime_identity_mismatch`;
- `result_protocol`;
- `start_limit_hit`;
- `dependency_failed`;
- `journal_cursor_expired`;
- `storage_budget_exceeded`;
- `interlock_blocks_activation`; and
- `partial_snapshot`.

A diagnostic may recommend refresh, qualification, authorization, approved
rebuild/activation, or a typed lifecycle operation. It never asks the worker to
execute raw shell, Nix, D-Bus, systemctl, journalctl, or Podman commands.

## Authorization

Observation authorization is checked before detailed identity disclosure and is
rechecked for follow streams and result pages. Initial capability groups are:

- discovery/summary status;
- full status/inspect;
- fast metrics;
- storage accounting;
- logs query;
- logs follow; and
- events/status follow.

Own-user observation follows user-worker policy. System observation is dangerous
and host-policy controlled. Logs and full inspect may be granted separately from
summary/metrics. Connection permission alone grants none of these capabilities.

A page cursor, log cursor, snapshot revision, operation ID, or stream ID is never
authorization. Authorization change terminates or redacts future delivery and
never leaks retained data from a stronger prior policy.

## Redaction

The worker serializes only allowlisted typed fields. It never returns:

- secret or credential values;
- environment values;
- raw command arguments;
- credential locations;
- absolute private source, rootfs, closure, cgroup, overlay, or storage paths;
- unrestricted labels/annotations;
- raw journal fields;
- raw D-Bus/Podman payloads; or
- backend error text without bounded classification/redaction.

Stable artifact and source identities use manifest-issued IDs/digests rather
than private absolute paths. Container IDs, PIDs, boot IDs, invocation IDs, and
cgroup identities are sensitive and returned only under the corresponding
inspect/metric/log policy.

Redaction happens before caching, framing, auditing, and stream buffering. Caches
are keyed by authorization class so a stronger cached response cannot satisfy a
weaker request.

## CLI output and exit status

Human `status` output defaults to bounded columns:

```text
NAME  SCOPE  LIFECYCLE  SUMMARY  FRESHNESS
```

Every row visibly marks partial/stale state. `inspect` renders layer headings and
availability tags. `logs` writes log content only through terminal-safe
rendering. Machine output preserves typed response tags and keeps logs separate
from diagnostics.

Read-only command exit statuses are:

| Exit | Meaning |
| ---: | --- |
| 0 | Query succeeded, including workload failure, empty logs, or a clearly marked useful partial snapshot. |
| 2 | Invalid CLI syntax or locally invalid request. |
| 3 | Authentication or authorization denied/unavailable. |
| 4 | Unknown, ambiguous, stale, or mismatched workload identity/cursor. |
| 6 | Worker or required backend unavailable with no useful authorized result. |
| 8 | Follow interrupted or gap requires refresh. |
| 9 | Protocol/API incompatibility. |

A failed workload is observed data, not failure of the status query. A partial
snapshot exits zero only when at least one requested applicable layer is useful;
an entirely unavailable response uses exit 6. Typed response/error codes remain
authoritative over shell categories.

## Worker state boundary

Permitted observability state is bounded and operational:

- short-lived redacted snapshot cache;
- metric baselines for rates;
- storage result cache;
- page and journal cursors;
- active stream windows;
- request-local stream sequences and snapshot revisions; and
- backend availability state.

The worker does not persist:

- metrics history;
- copied logs;
- workload event history;
- desired state;
- arbitrary backend snapshots; or
- a replacement for journald/systemd/Podman/Nix authority.

Worker restart invalidates worker pages, revisions, rates, and event sequences.
Journal cursors may survive according to journald retention. Current state is
reconstructed from manifest and authoritative backends.

## Security impact

This contract applies analogous controls without extending the scope of the
stable `GRAFT-TM-*` invariants:

- unknown snapshot, filter, metric, event, and diagnostic intent fails closed;
- read APIs expose no raw backend passthrough;
- backend strings remain bounded untrusted data and terminal clients visibly
  encode control characters;
- observations are bound to manifest, worker, manager, boot, target, and
  workload identity where applicable;
- system/rootful, non-root user/rootless, and UID-0 user/rootful observation
  remain separate;
- observation cannot alter startup activation or lifecycle;
- clients cannot select store, rootfs, cgroup, overlay, volume, or host paths;
  and
- inspection exposes no hardening-relaxation operation or secret backend
  configuration.

Capability classification remains:

| Capability | Class |
| --- | --- |
| Own-user summary/status/approved metrics | First-class but sensitive |
| Own-user logs/full inspect/storage | First-class, separately authorizable |
| System summary/metrics | Dangerous, host-policy controlled |
| System logs/full inspect/storage | Dangerous, separately host-policy controlled |
| Raw backend inspect/journal/path query | Forbidden |
| Historical metrics/log database | Deferred outside the worker |

## Implementation slices

After this design and [#242] are approved:

1. Publish snapshot, layer availability, diagnostic, log, metric, event, and
   cursor types with exhaustive schemas and unknown-field tests.
2. Implement mock manifest/systemd/Podman/cgroup/journal/storage adapters with
   complete unavailable/stale/epoch-change matrices.
3. Add read-only discovery/status and inspect against mock adapters.
4. Add real manager/runtime identity and finite-job result correlation.
5. Add bounded journal query/follow with cursor rotation and redaction tests.
6. Add fast metrics, reset-safe rates, and cgroup hierarchy validation.
7. Add budgeted storage categories and shared-byte correctness tests.
8. Add event/status streams, gap recovery, authorization changes, and worker
   restart tests.
9. Add CLI status/inspect/log rendering, JSON, pagination, exit-status, and
   terminal-injection tests.
10. Integrate TUI/controller clients only through the same API.

No slice may add an untyped backend map, arbitrary filter/path, hidden history
database, or mutation side effect.

## Linked work

- [#101] owns broader local environment/doctor diagnostics;
- [#136] implements the initial lifecycle client slice;
- [#146] owns typed health/readiness/reload/shutdown intent;
- [#171] owns complete Quadlet search-path/drop-in drift detection;
- [#241] implements the worker/API including this observability surface;
- [#242] owns concrete Nix services, sockets, paths, authorization, and adapter
  installation; and
- [#245] owns remote controller authentication and transport.

[#101]: https://github.com/Patrick-Kappen/graft/issues/101
[#136]: https://github.com/Patrick-Kappen/graft/issues/136
[#146]: https://github.com/Patrick-Kappen/graft/issues/146
[#171]: https://github.com/Patrick-Kappen/graft/issues/171
[#241]: https://github.com/Patrick-Kappen/graft/issues/241
[#242]: https://github.com/Patrick-Kappen/graft/issues/242
[#245]: https://github.com/Patrick-Kappen/graft/issues/245
