# Local lifecycle operations

> **Status:** approved design for future implementation. The current release
> materialises lifecycle semantics into Quadlet services but does not expose
> `graft up`, `graft down`, or `graft restart`. This contract depends on the
> [local worker API](worker-api.md) and does not add a rebuild path.

This document defines the lifecycle operations shared by the future CLI, TUI,
and controller. It complements [Workload lifecycle semantics](lifecycle.md),
which defines TOML intent and generated service behavior. It does not change the
existing `long-running`, `job`, or `setup` materialisation contract.

## Objectives

Lifecycle operations must:

- act only on already materialised, manifest-bound Graft workloads;
- preserve systemd as lifecycle authority and Podman as runtime authority;
- produce the same semantics for CLI, TUI, and controller callers;
- distinguish accepted work from a proven terminal result;
- define repeated, concurrent, interrupted, and recovered operations;
- expose dependency failures without inventing a second dependency engine;
- fail closed on stale or ambiguous identity; and
- never imply configuration changes or persistent-data removal.

## Command surface

The initial user-facing commands are:

```text
graft up <workload>
graft down <workload>
graft restart <workload>
```

They map one-to-one to the typed worker actions `up`, `down`, and `restart`.
Command spelling is a client concern; operation behavior is fixed here.

The initial API has no `start`, `stop`, `kill`, `remove`, `delete`, `purge`,
`force`, `reload`, `try-restart`, arbitrary signal, systemd job-mode, or Podman
operation. A future alias must preserve these exact typed semantics rather than
create another lifecycle path.

## Selection and authority

The client resolves an input name to a structured selector before mutation. The
selector contains the explicit target, current manifest generation, and
manifest-issued workload identifier required by the worker contract.

A normal local client may aggregate authorized system and user discovery, but:

- an unqualified name that matches more than one visible scope is ambiguous and
  fails before authorization or mutation;
- the system worker serves only target `system`;
- a non-root user worker serves only target `user` for its own effective UID;
- a UID-0 user worker serves target `user` for UID 0 and remains rootful;
- a client cannot substitute a unit, container, manager, UID, or backend; and
- local operation never requires a controller.

Every request is revalidated against the current manifest and loaded generated
service immediately before submission. Missing, stale, shadowed, foreign, or
scope-mismatched identity fails closed as defined by the worker API.

## Manager operation mapping

The worker submits one typed action for the manifest-bound generated service to
its fixed systemd manager:

| Graft action | systemd operation | Fixed conflict behavior |
| --- | --- | --- |
| `up` | start the generated service | Preserve an incompatible queued job (`fail`). |
| `down` | cancel a verified selected-service start job when needed, then stop | Preserve every remaining incompatible queued job (`fail`). |
| `restart` | restart the generated service | Preserve an incompatible queued job (`fail`). |

The adapter chooses the concrete D-Bus method and unit identity. Clients cannot
select or override the job mode. To abort activation, `down` first reads the
selected service's current job identity, proves that the job belongs to exactly
that service and is a compatible start, and cancels that verified job. It then
submits the stop with conflict-preserving `fail` mode. A missing proof, changed
job identity, cancellation race, or remaining incompatible transaction returns
conflict. Graft never uses transaction-wide `replace`, which could replace jobs
for dependency units pulled into the same transaction. Normal systemd effects
from cancelling the verified selected-service job and its materialised graph are
reported rather than described as selected-unit-only effects. `up` and
`restart` never cancel independently submitted manager work.

`restart` is one manager restart operation. It is not implemented in the client
as `down` followed by `up`; two operations would permit unrelated work to
interleave, lose one operation identity, and make interruption ambiguous.
Systemd may still expose stop, inactive, and start states while executing its
single restart transaction; Graft does not promise an unobservable transition.

The worker does not call Podman directly as a fallback. Quadlet's generated
service owns Podman creation, stop, and generator-defined cleanup.

## State vocabulary

This contract normalizes authoritative systemd state exhaustively:

| systemd `ActiveState` and relevant substate | Lifecycle state |
| --- | --- |
| `inactive` | `inactive` |
| `activating` with correlatable start/invocation/restart evidence | `activating` |
| `active` with expected running substate | `active-running` |
| `active` with expected exited substate | `active-exited` |
| `deactivating` with correlatable stop/cleanup evidence | `deactivating` |
| `failed` | `failed` |
| `reloading`, `refreshing`, or `maintenance` | `manager-busy` |
| `activating` or `deactivating` without correlatable transition evidence | `manager-transition-conflict` |
| `active` with an incompatible or unrecognized substate | `unsupported-manager-state` |
| Any future unrecognized authoritative value | `unsupported-manager-state` |
| Authoritative state cannot be obtained | `unknown` |

`active-running` is the expected successful active state for `long-running`.
`active-exited` is the expected retained success state for `setup`. A successful
`job` returns to `inactive`; success must therefore come from the
operation-correlated manager job and invocation result rather than current
active state alone.

For every action, `manager-busy` and `manager-transition-conflict` return
conflict without submission because reload, refresh, maintenance, and
uncorrelatable transitions are outside this API. An `unsupported-manager-state`
returns `unexpected_state` without submission. `unknown` remains reserved for
unavailable authoritative observation and returns backend unavailable. These
global rules apply before the action matrices, so no raw manager state is
silently coerced.

The selected unit's queued `Job` is inspected independently of `ActiveState`
before every matrix decision:

| Requested action | Queued job handling |
| --- | --- |
| `up` | Join a verified compatible start job; every other job conflicts. |
| `down` | Join a verified compatible stop job; cancel a verified start job then submit stop; every other job conflicts. |
| `restart` | Join a verified compatible restart job; every other job conflicts. |

A job is compatible only when its concrete unit, type, and current identity match
the pinned selected service and expected action. If job identity or state changes
between validation and submission, the request conflicts. `no_change` and a
successful terminal condition additionally require that no queued job can
reverse the observed state. The worker never derives job absence from
`ActiveState` alone.

Detailed state fields and cross-layer status remain owned by the observability
design in [#137]. This vocabulary fixes only what lifecycle completion needs.

## `up` contract

`up` requests the workload's successful lifecycle state. Its meaning depends on
the materialised lifecycle.

### Long-running workload

| Initial state | Behavior |
| --- | --- |
| `inactive` | Submit start and wait for `active-running`. |
| `failed` | Submit start and wait for `active-running`; a successful new invocation clears the prior failed state. |
| `activating` | Join a compatible start job, or observe a recognized manager-owned automatic-restart phase; otherwise return conflict. |
| `active-running` | Return success with `no_change`. |
| `active-exited` | Fail `unexpected_state`; do not reinterpret a mismaterialised unit. |
| `deactivating` | Return conflict; do not reverse an independently active stop. |
| `unknown` | Fail backend unavailable; do not submit. |

A clean exit after readiness is not `up` success if the unit is already inactive
when completion is evaluated. A matching systemd restart policy may create
further activations. A directly waiting caller receives success only when the
operation-correlated service reaches `active-running` before that caller's
deadline. Shared operation observation may still establish the retained result
under the deadline and grace rules below.

### Setup workload

| Initial state | Behavior |
| --- | --- |
| `inactive` | Submit start and wait for `active-exited`. |
| `failed` | Submit start and wait for a new successful `active-exited` invocation. |
| `activating` | Join a compatible start job/invocation, or observe a recognized automatic-restart phase; otherwise return conflict. |
| `active-exited` | Return success with `no_change`. |
| `active-running` | Fail `unexpected_state`. |
| `deactivating` | Return conflict. |
| `unknown` | Fail backend unavailable; do not submit. |

Because `RemainAfterExit=yes` retains successful setup state, a fresh `up` does
not run the command again. Callers use `restart` for an explicit new execution.

### Finite job

| Initial state | Behavior |
| --- | --- |
| `inactive` | Submit and wait for one new finite execution. |
| `failed` | Submit and wait for one new finite execution. |
| `activating` | Join a compatible start job/execution, or observe a recognized automatic-restart phase; otherwise return conflict. |
| `active-running` or `active-exited` | Fail `unexpected_state`. |
| `deactivating` | Return conflict. |
| `unknown` | Fail backend unavailable; do not submit. |

A job's successful terminal state is `inactive/success`. Every fresh operation
identifier submitted after a completed job requests a new execution. Duplicate
suppression for the same operation identifier prevents accidental replay of one
request; it does not turn a finite job into retained desired state.

An `activating` state does not prove that a manager job exists. During
`RestartSec`, for example, systemd may report an automatic-restart substate
between attempts with no queued job. The worker therefore inspects job presence,
service substate, invocation identity, and restart metadata. It may join a
compatible manager job or boundedly observe a recognized automatic-restart
sequence. An unrecognized activation without correlatable evidence is a
conflict, not permission to submit another start.

Joining existing manager work means waiting for and reporting that work. It does
not claim that Graft originally submitted it. The result disposition is
`existing_manager_work`, not `worker_submitted`.

## `down` contract

`down` requests a quiescent unit: `inactive`, or sticky `failed` with no active
job or service process. It does not request absence of materialised artifacts or
runtime data.

| Initial state | Behavior |
| --- | --- |
| `inactive` | Return success with `no_change`. |
| `failed` | If quiescent, return `no_change` and preserve failure evidence; otherwise submit stop and require a quiescent result. |
| `activating` | Cancel a verified compatible start job then stop, or directly stop a recognized automatic-restart delay with no job; otherwise conflict. |
| `active-running` | Submit stop and wait for `inactive`. |
| `active-exited` | Submit stop and wait for `inactive`. |
| `deactivating` | Join a compatible stop job or observe recognized service cleanup; otherwise return conflict. |
| `unknown` | Fail backend unavailable; do not submit. |

For a finite job, `down` during `activating` aborts the current execution. The
terminal result uses the existing model: action `down`, disposition
`worker_submitted`, outcome `succeeded`, and final state `inactive`. It does not
claim successful completion of the job command. For setup,
`down` clears retained `active-exited` state. For long-running workloads it
stops the generated service and lets the generated service execute its normal
stop and best-effort cleanup commands.

A sticky failed state is valuable diagnostic evidence and `StopUnit` alone does
not clear it. The initial `down` contract therefore does not call
`ResetFailedUnit`. A quiescent failed unit is already stopped and succeeds with
`no_change` while retaining `failed` as its final manager state. Explicit typed
failure reset remains deferred rather than hidden inside `down`.

`down` never directly removes or mutates:

- TOML or resolved configuration;
- Quadlet source or generated units;
- Nix store paths, rootfs trees, or closure mounts;
- writable overlay state;
- bind-mount sources;
- managed or external volumes;
- secrets or credentials;
- user accounts, sessions, or linger policy; or
- unrelated containers or units.

Quadlet-generated `ExecStop` and `ExecStopPost` behavior remains part of the
already materialised unit. Graft does not add stronger Podman cleanup and never
interprets `down` as data deletion.

## `restart` contract

`restart` explicitly requests a new invocation. It uses systemd restart
semantics, which start an inactive service as well as restarting an active one.

### Restarting a long-running workload

- `active-running` → submit restart and require a different successful
  invocation reaching `active-running`;
- `inactive` or `failed` → submit restart and require `active-running`;
- `activating` or `deactivating` with a verified compatible restart job → join
  it as `existing_manager_work`; otherwise conflict;
- `active-exited` → `unexpected_state`;
- `unknown` → backend unavailable.

### Restarting a setup workload

- `active-exited` → submit restart and require a new successful invocation
  returning to `active-exited`;
- `inactive` or `failed` → submit restart and require `active-exited`;
- `activating` or `deactivating` with a verified compatible restart job → join
  it as `existing_manager_work`; otherwise conflict;
- `active-running` → `unexpected_state`;
- `unknown` → backend unavailable.

### Restarting a finite job

- `inactive` or `failed` → submit restart and wait for one new successful finite
  execution;
- `activating` or `deactivating` with a verified compatible restart job → join
  it as `existing_manager_work`; otherwise conflict;
- active states inconsistent with job materialisation → `unexpected_state`;
- `unknown` → backend unavailable.

A successful `restart` must prove a new invocation or finite execution relative
to the state captured before submission. Merely observing the same expected
active state is insufficient. If systemd executes an operation-correlated stop
phase, the worker must prove that phase did not fail. Restart from `inactive` or
quiescent `failed` may proceed directly to start with no stop phase; that absence
is valid and does not invent stop evidence. Systemd may continue into a
successful new invocation after `ExecStop=` failure; Graft reports outcome
`failed`, failure phase `stop`,
and the actual final active state in that case. Generator-owned ignored
best-effort cleanup such as `ExecStopPost=-...` is not manager failure. The
worker retains stop-job, service-result, and invocation evidence needed to keep
failure phase separate from final state. `restart` is not safe for automatic
replay after an interrupted or unknown result.

## Completion contract

Manager acceptance is not lifecycle success. The worker correlates the manager
job and resulting invocation and waits for the lifecycle-specific terminal
condition:

| Action and lifecycle | Successful terminal condition |
| --- | --- |
| `up`, `long-running` | Correlated service is `active-running`. |
| `up`, `setup` | Correlated invocation completed successfully and unit is `active-exited`. |
| `up`, `job` | Correlated finite invocation completed successfully and unit is `inactive`. |
| `restart`, `long-running` | Any executed stop phase succeeded, and new service invocation is `active-running`. |
| `restart`, `setup` | Any executed stop phase succeeded, and new invocation completed successfully in `active-exited`. |
| `restart`, `job` | Any executed stop phase succeeded, and new finite invocation completed successfully in `inactive`. |
| `down`, every lifecycle | Unit is `inactive`, or is quiescent `failed` and any submitted stop itself completed successfully. |
| Any valid `no_change` | Initial state already satisfies the action without submission and no queued job can reverse it. |

The worker must not infer successful job completion from `inactive` alone. It
uses manager job completion, invocation identity, and typed service result. A
non-zero exit, signal, timeout, protocol failure such as `Result=protocol`,
dependency failure, condition failure, or start-limit rejection remains a typed
failure. For `down`, a sticky failure that predates the operation may remain as
diagnostic state after a successful stop. A new stop or cleanup failure caused
by the operation is terminal failure even if the unit is ultimately quiescent.

A systemd restart policy may perform retries within one activation. Those are
manager behavior from materialised intent, not worker retries. The worker
observes the eventual correlated success or failure while any joined caller
remains interested, then for the fixed completion grace below. It never adds a
retry loop around the lifecycle action.

## Terminal response model

An accepted operation terminates as exactly one tagged response variant:

- `LifecycleTerminalResult` for `no_change`, worker-submitted, or joined manager
  work; or
- `MutationTerminalError` when the accepted request terminates before any
  lifecycle submission or join.

`LifecycleTerminalResult` contains bounded typed fields:

- operation identifier and origin worker epoch;
- current worker epoch;
- manifest generation and workload selector;
- lifecycle kind and requested action;
- authorization classification;
- initial state;
- disposition: `no_change`, `worker_submitted`, or `existing_manager_work`;
- outcome: `succeeded`, `failed`, or `result_unknown`;
- manager job identity when one was observed;
- invocation identity when available;
- final state, typed unit result, and operation failure phase when applicable;
- finite-process exit code or signal when available and authorized;
- request-start and completion timestamps;
- submission timestamp when `worker_submitted`, optional observed timing for
  `existing_manager_work`, and no invented submission time for `no_change`;
- whether dependencies affected the result;
- whether the manifest changed after submission; and
- safe typed failure code and retry guidance when outcome is `failed` or
  `result_unknown`, absent when outcome is `succeeded`.

`MutationTerminalError` contains operation and epoch identity, safe typed error
code, phase, timestamp, and retry guidance. It deliberately has no disposition,
outcome, manager job, invocation, final workload state, or submission timestamp.
The initial codes are `cancelled_before_submission` and
`deadline_before_submission`.

The response contains no raw D-Bus values, journal records, unit properties,
Podman output, command lines, environment values, or arbitrary backend text.
Observability clients may follow separately authorized details through [#137].

### Result disposition and outcome

Disposition records how manager work related to the request, independently of
whether it succeeded:

- `no_change`: no manager operation was needed because initial state already
  satisfied the action;
- `worker_submitted`: this worker submitted the typed manager operation; or
- `existing_manager_work`: the worker joined a compatible job, invocation,
  automatic-restart sequence, or cleanup already in progress.

A job `up` is never `no_change` merely because a previous execution succeeded.

Outcome records what could be proved:

- `succeeded`: the action-specific successful terminal condition was proven;
- `failed`: a terminal manager, dependency, or process failure was proven; or
- `result_unknown`: manager state may have changed, but the terminal result
  cannot be proven.

Thus `worker_submitted` plus `failed` represents an accepted operation whose
workload execution failed. `result_unknown` is not success or failure of the
workload and must not be converted to either by a client.

## Progress stream

A caller may request a bounded server progress stream using the worker protocol.
Items are tagged states, not backend strings:

```text
authorized
validated
existing_manager_work | worker_submitted
queued
activating | deactivating
terminal | result_unknown
```

Each item carries operation identity, worker epoch, sequence, timestamp, and the
small typed fields relevant to that phase. Repeated manager state observations
are coalesced. Logs are not embedded in lifecycle progress.

CLI, TUI, and controller may render progress differently, but they cannot infer
a different terminal result. Untrusted workload or backend strings must follow
the terminal-safe rendering rule in the worker contract.

## Dependencies

The worker does not traverse, reorder, or independently mutate the dependency
graph. It submits the selected generated service and lets systemd apply the
already materialised `Requires=`, `Wants=`, `After=`, `Before=`, `PartOf=`, and
`BindsTo=` relationships documented in [Typed workload dependencies](dependencies.md).

Consequences remain visible:

- `up` or `restart` may activate declared required, optional, bound, or external
  units through the systemd transaction;
- a dependency start or ordering failure can fail the selected operation;
- `down` does not invent reverse stop propagation, but materialised `PartOf=` or
  `BindsTo=` relationships may make systemd change related units;
- systemd may merge a Graft request with an existing transaction; and
- the lifecycle result identifies dependency involvement without claiming a
  complete graph-wide outcome.

These effects were approved when Nix materialised the typed dependency intent.
Runtime authorization permits the selected manifest-bound action, not arbitrary
changes to dependency names or relationships. A stale or unexpected dependency
transaction fails according to manager and identity checks; the worker does not
fall back to direct Podman control.

## Materialisation, enablement, and reload

Lifecycle mutation applies only to a current, valid materialisation manifest.
The worker never performs:

- `daemon-reload`;
- Quadlet generator execution;
- NixOS rebuild or switch;
- Home Manager activation;
- source-unit installation or removal; or
- systemd enable, disable, mask, or unmask.

Nix activation owns source installation, generator invocation through manager
reload, startup policy, and manifest publication. A workload without startup
activation may still be started manually when it is validly materialised.
Systemd unit-file enablement is not workload availability and is not changed by
`up` or `down`.

A missing, masked, not-found, shadowed, stale, generator-failed, or provenance-
mismatched service returns its typed materialisation failure. The worker does
not attempt reload as repair because doing so could activate unreviewed ambient
source changes and would hide a broken Nix activation.

## Authorization and audit

User-scope lifecycle is authorized by the fixed user-worker policy. UID 0 user
scope remains its own rootful context. System lifecycle requires explicit
per-operation authorization under host policy.

Authorization covers one typed action on one manifest-bound workload generation.
It does not authorize:

- another workload or scope;
- arbitrary manager units;
- dependency edits;
- raw D-Bus or Podman access;
- persistent-data mutation; or
- a later operation after identity or generation changes.

The worker emits denial or authorized-attempt audit before returning or
submitting, then separate submission and terminal/result-unknown records as
required by the [worker audit contract](worker-api.md#audit-contract). Failure
to accept the required initial audit record fails mutation closed.

## Concurrency

The worker admits at most one lifecycle mutation per workload. This is in
addition to worker-wide and per-principal limits.

Mutation records are keyed by worker epoch, authenticated principal key, and
UUIDv7. For local workers, the principal key contains the fixed worker context
and accepted peer UID. Future remote callers use a separate stable authenticated
principal identifier. The UUID is explicitly not authorization and cannot join
or conflict with another principal's record. Every duplicate and every separate
operation-result query is reauthorized against the current principal, workload,
and action before returning any in-flight or retained result.

Immutable mutation equality covers only action, structured workload selector,
manifest generation, and origin worker epoch under the principal/UUID key.
Caller-specific delivery state—connection and request IDs, deadline, progress
preference, and response formatting—is excluded. A reconnect may therefore join
the same mutation with a new deadline; changing any mutation field conflicts.

Within one worker epoch and principal key:

- a duplicate operation identifier with the identical immutable request joins
  its retained in-flight or terminal result;
- the same identifier with a different request is a conflict;
- an unknown identifier outside its acceptance window returns
  `operation_id_expired` and can never become a fresh mutation;
- a different lifecycle mutation while one is in flight fails admission before
  its new ID is accepted, returning `operation_in_progress` with safe
  correlation metadata;
- read-only status may proceed concurrently; and
- an existing manager job not submitted through Graft is handled according to
  the action tables rather than being cancelled or replaced.

The lock covers validation through terminal/result-unknown publication. It is
bounded operational memory, not persistent desired state.

A manifest publication during an in-flight operation never retargets that
operation. Before backend submission, a generation change fails validation as
`stale_manifest`. After submission, the worker remains pinned to the original
generation, generated service, manager job, and invocation evidence. The result
records `manifest_changed_during_operation = true`, while subsequent requests
must use the new generation. If manager reload or replacement destroys the
ability to prove the original attribution or terminal outcome, the pinned
operation returns `result_unknown`; it does not adopt the replacement workload.
Lifecycle progress for the submitted operation follows this rule rather than
ending merely because the manifest changed.

Operation identifiers use the exact canonical lowercase hyphenated UUIDv7 wire
encoding defined by the
[worker mutation identity contract](worker-api.md#mutation-identity-concurrency-and-interruption).
Their embedded timestamp may be at most one minute ahead of server receive time
and at most ten minutes old. The server publishes UTC Unix epoch milliseconds in
`ServerHello` so clients can detect local skew before mutation. Every accepted
identifier retains its immutable request while in flight and its complete
bounded terminal result until both the operation is terminal and the
complete ten-minute acceptance window has passed. A resultless tombstone cannot
replace that result. A known identical in-flight or terminal request may still
join after its timestamp ages out and receives the same result; an unknown
expired ID cannot start work. Entries are never evicted early and reused as
fresh. Retained results are capped at 32 KiB each and admission is bounded to
256 records per principal and 1,024 per worker. Exhaustion rejects new mutations
with `overloaded` rather than weakening duplicate protection.

## Deadlines, cancellation, and disconnects

A client deadline bounds only that caller's interest and synchronous delivery;
it does not fix the shared operation outcome. It also does not replace
`TimeoutStartSec`, `TimeoutStopSec`, or manager job timeouts from materialised
intent.

An operation ID becomes accepted and reserved only at the final pre-submission
commit point, after the complete bounded request has passed parsing,
authentication, current authorization, required initial audit, current manifest
and identity validation, operation preconditions, and per-workload concurrency.
Any failure, `operation_in_progress`, disconnect, or malformed frame before that
point reserves no ID. The same ID may be submitted later while its UUIDv7
timestamp remains acceptable.

After acceptance but before the backend call, cancellation or deadline performs
no mutation and stores the typed terminal error
`cancelled_before_submission` or `deadline_before_submission`. The complete
error is retained for the normal acceptance window, and the same principal/ID
can never later submit work. A backend call attempted after acceptance,
including a synchronous manager rejection, terminates as
`LifecycleTerminalResult` with `worker_submitted`, outcome `failed`, and failure
phase `submission`.

After submission, each duplicate/joined caller has independent interest. A deadline, cancellation, or disconnect removes only that caller and
releases its delivery state. Normal shared observation continues while at least
one caller remains interested. The fixed 30-second server completion grace
starts only when the final joined caller loses interest:

- the manager job is not cancelled, reversed, or rolled back;
- the worker retains the per-workload lock and bounded attribution only through
  the grace period;
- a terminal result proven during grace becomes the retained operation result;
- every caller that already timed out or cancelled keeps its own exit-8 client
  result, while a later duplicate/result query receives the retained terminal
  operation result;
- when grace expires without proof, the worker stores `result_unknown`, releases
  the lock and backend observation, and never revises that retained result based
  on later manager state; and
- the client must inspect current state before considering another mutation.

The 30-second grace is independent of an unset or unbounded workload
`TimeoutStartSec`/`TimeoutStopSec`. A manager job may continue after Graft has
published `result_unknown`; its later completion is ordinary observed state,
not retroactive mutation completion.

A caller must never automatically replay `restart` after `result_unknown`.
Fresh `up` and `down` are allowed only after state refresh establishes that the
new request has well-defined behavior under the tables above. A finite job `up`
may execute work again, so it also requires explicit caller confirmation after
an unknown result.

## Worker restart recovery

A worker restart creates a new epoch and loses in-memory duplicate results. It
does not stop workloads or manager jobs.

A reconnect presenting an old operation epoch or expired UUIDv7 cannot submit
lifecycle work. After worker restart has lost the operational cache, an
old-epoch operation-result query always returns `result_unknown`; it never
reconstructs a terminal lifecycle result from audit output. The client then
obtains a fresh status snapshot. Audit remains evidence for operators, not an
operation-result database.

Observed state can establish whether a long-running workload is active, a setup
is retained, or a unit is inactive. It cannot in general prove whether an old
finite job ran exactly once or whether a restart briefly completed. Graft does
not promise exactly-once behavior across worker restart and does not create a
persistent operation database to manufacture that guarantee.

## CLI output and exit status

Human output includes explicit host/scope identity, action, disposition,
outcome, final state, and concise typed failure guidance. Machine output serializes the same
bounded lifecycle result and keeps stdout free of logs. Progress and diagnostics
use their documented client channels.

Initial lifecycle command exit statuses are:

| Exit | Meaning |
| ---: | --- |
| 0 | Terminal operational success, including `no_change`. |
| 2 | Invalid CLI syntax or locally invalid request. |
| 3 | Authentication or authorization denied/unavailable. |
| 4 | Workload, scope, manifest, materialisation, or provenance identity invalid. |
| 5 | Conflict or another operation is in progress. |
| 6 | Worker, manager, runtime, audit sink, or required backend unavailable. |
| 7 | Workload activation, execution, dependency, timeout, or stop failed terminally. |
| 8 | Cancellation, client deadline, interruption, or `result_unknown`. |
| 9 | Protocol or API version incompatibility. |

Typed error/result codes are authoritative. Exit statuses are stable shell-level
categories and must not be parsed as finer state. Existing pure resolver CLI
behavior is unchanged until lifecycle subcommands are implemented.

## Failure classification

Lifecycle-specific failures include:

| Failure | Required behavior |
| --- | --- |
| Ambiguous scope/name | Fail before mutation and identify required qualification. |
| Stale generation | Fail before mutation; client refreshes discovery. |
| Missing/shadowed/foreign service | Fail before mutation; no daemon reload or Podman fallback. |
| Masked or invalid unit | Fail before mutation with materialisation/manager evidence. |
| Unexpected lifecycle state | Fail closed; do not reinterpret unit shape. |
| Existing incompatible manager job | Conflict; do not replace it. |
| Start limit or condition failure | Terminal workload/manager failure. |
| Dependency failure | Terminal failure with bounded dependency classification. |
| Process non-zero exit/signal | Terminal execution failure for job/setup or service failure. |
| `Result=protocol` | Terminal typed service failure, preserving the compatibility boundary. |
| Authorization/audit unavailable | Fail before backend submission. |
| Backend accepted, terminal evidence lost | `result_unknown`; no blind retry. |

Error summaries remain lowercase, bounded, safe for display, and free of raw
backend payloads.

## Security invariants

This contract preserves and strengthens:

- **GRAFT-TM-01:** unsupported lifecycle intent and unknown operation variants
  fail closed;
- **GRAFT-TM-02:** only three typed actions exist, with no raw systemd, Podman,
  shell, or Nix passthrough;
- **GRAFT-TM-04:** every mutation is bound to current manifest generation,
  workload identity, and generated-service provenance;
- **GRAFT-TM-05:** system/rootful, non-root user/rootless, and UID-0 user/rootful
  contexts remain distinct;
- **GRAFT-TM-06:** runtime commands do not change declarative startup intent,
  enablement, or activation policy;
- **GRAFT-TM-09:** `down` and `restart` never imply persistent-data deletion;
  and
- **GRAFT-TM-13:** no lifecycle request can weaken materialised hardening.

Restart replay, finite-job replay, manager-job replacement, automatic reload,
and fallback to direct runtime mutation are fail-closed boundaries rather than
convenience behavior.

## Deferred operations

The initial contract deliberately defers:

- reload and application-specific graceful reload;
- arbitrary signals and force kill;
- health/readiness-triggered actions;
- transient or scaled instances;
- scheduling and overlap policy for native timers;
- bulk graph operations and rollback;
- delete, purge, volume cleanup, or overlay reset;
- changing startup activation or enablement;
- automatic repair/reload/rebuild; and
- controller-specific rollout orchestration.

Each requires separate typed intent, authority, failure, and data-safety design.
None may be approximated through a generic escape hatch.

## Implementation slices

After this design and [#137] and [#242] are approved:

1. Publish lifecycle action, progress, result, and error types with exhaustive
   serialization and unknown-field tests.
2. Implement a mock systemd adapter state matrix for every lifecycle/action/
   initial-state combination.
3. Implement manager job and invocation correlation, including finite job
   success while inactive.
4. Add per-workload concurrency, duplicate operation, cancellation, deadline,
   and worker-epoch tests.
5. Add negative manifest, provenance, masked-unit, dependency, authorization,
   and audit-sink tests.
6. Add real system and user manager integration for long-running, job, and setup
   fixtures.
7. Add CLI commands and exit-status tests against the real worker boundary.
8. Add TUI and future controller clients only through the same typed API.

No implementation slice may introduce daemon reload, direct Podman lifecycle,
or persistent operation state.

## Linked work

- [#136] implements the initial `up` and `down` client/runtime slice after
  worker prerequisites; `restart` implementation requires a separate follow-up
  slice before implementation begins;
- [#137] defines detailed status, result evidence, logs, metrics, and events;
- [#146] owns health, readiness, watchdog, and graceful behavior;
- [#171] owns complete unit shadow/override detection;
- [#241] implements the local worker and typed API;
- [#242] defines concrete Nix services, sockets, policy, and authorization; and
- [#245] defines remote controller authentication and replay protection.

[#136]: https://github.com/Patrick-Kappen/graft/issues/136
[#137]: https://github.com/Patrick-Kappen/graft/issues/137
[#146]: https://github.com/Patrick-Kappen/graft/issues/146
[#171]: https://github.com/Patrick-Kappen/graft/issues/171
[#241]: https://github.com/Patrick-Kappen/graft/issues/241
[#242]: https://github.com/Patrick-Kappen/graft/issues/242
[#245]: https://github.com/Patrick-Kappen/graft/issues/245
