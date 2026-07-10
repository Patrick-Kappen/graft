# Workload lifecycle semantics

> **Design status:** approved for implementation in
> [#131](https://github.com/Patrick-Kappen/graft/issues/131), but not implemented
> yet. Current configuration must not use the planned `service.lifecycle` field.

Graft models every workload as a systemd-managed service while distinguishing
three process lifecycles through typed workload intent. Users should not need to
choose raw systemd `Type=` or `RemainAfterExit=` values.

## Intent contract

The planned TOML field is:

```toml
[config.service]
lifecycle = "long-running"
```

`lifecycle` accepts exactly these values:

| Lifecycle | Purpose | Quadlet service mapping | State after success | Repeating timer compatible |
| --- | --- | --- | --- | --- |
| `long-running` | daemon or continuously available process | `Type=notify` | inactive if the process exits | no timer semantics |
| `job` | finite or timer-triggered work | `Type=oneshot`, `RemainAfterExit=no` | inactive | yes |
| `setup` | finite work whose completed state remains active | `Type=oneshot`, `RemainAfterExit=yes` | active/exited | no |

When `service.lifecycle` is absent, `long-running` is the default. Graft does not
infer lifecycle from `runtime.command`: the same executable can be a daemon, a
finite job, or a setup action depending on its arguments and purpose.

The existing parser fields `service.type` and `service.remainAfterExit` are raw
systemd-shaped, are not implemented, and are not the public lifecycle contract.
[#131](https://github.com/Patrick-Kappen/graft/issues/131) must reject them with
an actionable migration diagnostic instead of preserving a second way to express
the same intent.

## Long-running service

Planned input:

```toml
[config.runtime]
command = ["server", "--listen", "0.0.0.0:8080"]

[config.service]
lifecycle = "long-running"
restart = "on-failure"
```

State transitions:

```text
inactive → activating → active
                       ├─ clean exit, no matching restart → inactive
                       ├─ failure, no matching restart    → failed
                       └─ matching restart policy         → activating
active   → explicit stop → deactivating → inactive
```

Quadlet uses its normal container-service path: conmon sends the readiness
notification, `podman run` is detached, and systemd tracks the generated notify
service. Application-provided or health-based readiness remains deferred to
[#146](https://github.com/Patrick-Kappen/graft/issues/146).

Normalized Podman 5.8.2 generated-service fixture:

```ini
[Service]
Type=notify
NotifyAccess=all
ExecStart=<podman> run --name <name> --replace --rm ... --sdnotify=conmon -d ...
ExecStop=<podman> rm -v -f -i <name>
ExecStopPost=-<podman> rm -v -f -i <name>
```

The `<podman>`, `<name>`, and `...` tokens normalize host-specific executable,
identity, cgroup, rootfs, and command values. The important lifecycle properties
are `Type=notify`, sd-notify, and detached execution.

## Finite job

Planned input:

```toml
[config.runtime]
command = ["extract-incoming"]

[config.service]
lifecycle = "job"
```

State transitions:

```text
inactive → activating while the container command runs
                    ├─ exit 0                  → inactive/success
                    ├─ non-zero exit or signal → failed
                    └─ matching failure restart → activating
activating → explicit stop → deactivating → inactive
```

`systemctl start` remains synchronous until the foreground container command
finishes. The `podman run` result therefore determines whether the unit succeeds
or fails. A job requires an explicit non-empty `runtime.command`; allowing the
implicit `/bin/graft-pause` command would create a job that never completes.

Normalized Podman 5.8.2 generated-service fixture:

```ini
[Service]
Type=oneshot
RemainAfterExit=no
ExecStart=<podman> run --name <name> --replace --rm ...
ExecStop=<podman> rm -v -f -i <name>
ExecStopPost=-<podman> rm -v -f -i <name>
```

There is no `-d` or `--sdnotify` argument. After success the unit becomes
inactive, so it can be started again.

## Repeatable timer-triggered job

A timer-triggered workload uses the same `job` lifecycle. If a timer elapses
while the service is already activating, systemd does not start a concurrent
copy. Once the job is inactive, a later activation can run it again.

The future matching timer shape is:

```ini
[Timer]
OnCalendar=<schedule>
Unit=<name>.service
```

Graft does not generate timer units yet. Typed schedules, missed runs, overlap,
jitter, persistence, and exact timer-to-service identity belong to
[#134](https://github.com/Patrick-Kappen/graft/issues/134). That implementation
must accept only `lifecycle = "job"` for repeating schedules and reject `setup`.
After #131 implements typed jobs, external systemd timer units may trigger a
Graft-generated job while native timer generation remains pending. Before #131,
`service.lifecycle` is unavailable and this workaround does not exist.

Timer-job state transitions:

```text
timer elapses + service inactive or failed → job activating → inactive or failed
timer elapses + service activating or active → no second service instance
later elapse + service inactive or failed     → a new activation
```

## Retained setup job

Planned input:

```toml
[config.runtime]
command = ["prepare-state"]

[config.service]
lifecycle = "setup"
```

State transitions:

```text
inactive → activating while the container command runs
                    ├─ exit 0                  → active/exited
                    ├─ non-zero exit or signal → failed
                    └─ matching failure restart → activating
active/exited → explicit stop → inactive
```

The active state records that setup completed; it does not keep a container
process alive. Because an active unit is not activated again by a timer,
`setup` is deliberately incompatible with repeating schedules.

Normalized Podman 5.8.2 generated-service fixture:

```ini
[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=<podman> run --name <name> --replace --rm ...
ExecStop=<podman> rm -v -f -i <name>
ExecStopPost=-<podman> rm -v -f -i <name>
```

## Restart and failure rules

Graft keeps lifecycle and restart intent separate. No lifecycle adds a restart
policy by default.

| Restart policy | `long-running` | `job` | `setup` |
| --- | --- | --- | --- |
| absent or `no` | allowed | allowed | allowed |
| `on-failure` | allowed | allowed | allowed |
| `on-abnormal` | allowed | allowed | allowed |
| `on-abort` | allowed | allowed | allowed |
| `on-watchdog` | allowed by current service contract | deferred to #146 | deferred to #146 |
| `on-success` | allowed | invalid | invalid |
| `always` | allowed | invalid | invalid |

systemd rejects `on-success` and `always` for `Type=oneshot`. Failure-only
restart policies can retry a failed finite command. A successful `job` remains
inactive; a successful `setup` remains active/exited.

`restartSec` is meaningful only with an effective restart policy. #131 must
reject it when `restart` is absent or `no`, rather than accepting intent that
cannot affect runtime behavior.

## Timeouts and stopping

`timeoutStartSec` has lifecycle-dependent meaning inherited from systemd:

- for `long-running`, it bounds startup until the notify service reports ready;
- for `job` and `setup`, it bounds the complete foreground command;
- a oneshot start timeout is disabled by default, but an explicit value enables
  the bound.

`timeoutStopSec` bounds stop and cleanup behavior for every lifecycle. Quadlet
adds both `ExecStop=` and best-effort `ExecStopPost=` removal. An explicit
systemd stop does not trigger `Restart=`. A timeout or abnormal command exit is
a failure and may trigger a matching restart policy.

Quadlet currently generates `podman run --replace --rm` for all three
lifecycles. Every new activation therefore creates a fresh runtime container;
`RemainAfterExit=yes` retains systemd state, not the exited container object.
Graft must not override these generator-owned cleanup arguments.

## Health and readiness boundary

Lifecycle does not imply a health policy:

- `long-running` initially uses Quadlet/conmon readiness;
- application-provided notify and `Notify=healthy` require explicit typed intent;
- finite jobs do not gain health checks or watchdog restarts by default;
- unhealthy action, graceful stop ordering, reload, and watchdog behavior remain
  owned by [#146](https://github.com/Patrick-Kappen/graft/issues/146).

Until that issue lands, explicit unsupported health or notify fields must fail
closed under [#106](https://github.com/Patrick-Kappen/graft/issues/106).

## Implementation boundary

[#131](https://github.com/Patrick-Kappen/graft/issues/131) owns:

- the typed lifecycle enum and machine-readable schema update;
- resolver mapping and invalid-combination diagnostics;
- mechanical `Type=` and `RemainAfterExit=` rendering in both Nix modules;
- equivalent system and user fixtures;
- real Quadlet generation proving notify/detached versus oneshot/foreground
  behavior;
- a regression fixture for the first finite timer-job service shape.

It does not generate `.timer` units, implement health/readiness, add autostart,
or expose arbitrary systemd service keys.

## Upstream evidence

This design was checked against Podman/Quadlet 5.8.2 and systemd 260. The future
compatibility matrix remains owned by
[#129](https://github.com/Patrick-Kappen/graft/issues/129).

- Podman always generates replacement, auto-removal, and cleanup commands for
  `.container` units: [generator source](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/pkg/systemd/quadlet/quadlet.go#L638-L664).
- Podman accepts only `notify` or `oneshot`, and omits sd-notify plus detach for
  oneshot: [generator source](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/pkg/systemd/quadlet/quadlet.go#L726-L749).
- Podman documents why `RemainAfterExit=yes` blocks repeated timer activation:
  [Podman 5.8.2 documentation](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/docs/source/markdown/podman-systemd.unit.5.md#L103-L120).
- systemd defines oneshot state behavior and restart restrictions in
  [`systemd.service`](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html#Type=).
- systemd defines the no-overlap and retained-active timer behavior in
  [`systemd.timer`](https://www.freedesktop.org/software/systemd/man/latest/systemd.timer.html#Description).
