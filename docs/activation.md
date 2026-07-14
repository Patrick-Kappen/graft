# Workload startup activation

> **Status:** implemented and covered through both generated-service checks and
> an advisory manager-level NixOS VM test.

Graft separates workload startup policy from process lifecycle and dependency
activation. A rendered workload remains manually, externally, or
dependency-activated unless the user explicitly asks for startup activation.

## Intent contract

The TOML shape is:

```toml
[deploy]
activation = "startup"
```

`startup` means that the target service manager requests the workload during its
normal startup transaction. It does not mean that Graft starts the service while
resolving TOML or building a Nix configuration.

When `deploy.activation` is absent, Graft renders no `[Install]` relationship.
Absence is the only representation of manual, dependency, or external-trigger
activation; there is no `manual` value and no boolean alias.

The concepts remain distinct:

| Intent | Field | Meaning |
| --- | --- | --- |
| Materialise the workload | `deploy.enable` | Whether the target module renders the Quadlet source unit |
| Select the service manager | `deploy.target` | System manager or current Home Manager account's user manager; rootless only when that account is non-root |
| Request at manager startup | `deploy.activation` | Optional startup relationship |
| Describe process behavior | `config.service.lifecycle` | Long-running service, finite job, or retained setup |
| Start through another workload | `dependencies[].requirement` or typed resource reference | Dependency activation owned by the typed dependency or resource contract |
| Run on a schedule | future typed timer | Timer activation owned by #134 |

## Target mapping

The resolver maps `startup` to one fixed target for each effective deploy target:

| Effective deploy target | Manager event | Quadlet output |
| --- | --- | --- |
| `system` | normal system boot reaches the multi-user environment | `[Install]` with `WantedBy=multi-user.target` |
| `user` | the user's service manager reaches its default target | `[Install]` with `WantedBy=default.target` |

Omitted `deploy.target` keeps the existing `system` default and therefore maps
to `multi-user.target`. Users cannot supply a target name. The resolved JSON
carries the selected install relationship so NixOS and Home Manager only render
it mechanically.

Normalized output for a system workload is:

```ini
[Install]
WantedBy=multi-user.target
```

Normalized output for a user workload is:

```ini
[Install]
WantedBy=default.target
```

Quadlet-generated services are transient and cannot be persistently enabled
with `systemctl enable`. Quadlet instead reads `[Install]` while generating the
service and creates the corresponding target wants symlink. Graft must render
the source relationship and must not run `systemctl enable` as a build or
activation side effect.

## User-manager and linger boundary

For a user workload, `startup` means startup of the current Home Manager
account's user manager, not an unconditional host-boot guarantee. Podman is
rootless only when that account is non-root; the module does not reject UID 0,
so a root-owned user manager retains root authority.

Manager availability still depends on host policy:

- with declarative linger, the user manager can start during host boot;
- without linger, it normally starts when the user logs in;
- if no user manager runs, no user workload can be started by its default target.

Linger, user creation, login sessions, and manager availability are host policy.
Graft TOML does not mutate them. Future `graft doctor` diagnostics in
[#101](https://github.com/Patrick-Kappen/graft/issues/101) may report that a
requested non-root rootless startup workload lacks the required host policy,
but must not enable linger implicitly.

## Lifecycle combinations

Startup activation is orthogonal to the implemented service lifecycle:

| Lifecycle | Startup behavior | State after success |
| --- | --- | --- |
| `long-running` | requested once by the manager startup transaction | active while the process runs |
| `job` | requested once as a startup job; no repetition is implied | inactive |
| `setup` | requested once as retained startup setup | active/exited |

The existing lifecycle validation still applies. In particular, `job` and
`setup` require an explicit command, and their restart-policy restrictions do
not change.

A startup `job` is not a timer. It runs when the relevant target is activated,
then becomes inactive. An already active target does not periodically request it
again. Repeating schedules, missed runs, jitter, overlap, and persistence remain
owned by [#134](https://github.com/Patrick-Kappen/graft/issues/134). The first
native timer contract must reject simultaneous `deploy.activation = "startup"`
so one workload does not silently gain two independent triggers.

A retained `setup` records successful completion through `active/exited`. The
startup relationship alone does not make another workload wait for that setup.
Use a typed [`required` and `after` dependency](dependencies.md) when another
workload must request and wait for it.

## Dependency and ordering boundary

`WantedBy=` creates a weak reverse `Wants=` relationship from the selected
target. It does not add workload-specific `After=`, `Requires=`, `PartOf=`, or
readiness behavior.

Consequences:

- startup workloads may activate in parallel;
- a workload failure does not by itself fail the target startup transaction;
- application readiness remains separate from startup activation;
- stopping a target is not Graft's workload stop contract;
- one workload that needs another must use a typed [workload dependency](dependencies.md)
  or resource contract rather than relying on target order.

The existing shared-network reference remains a valid example of independent
dependency activation: starting a dependent service asks Quadlet/systemd to
start its namespace owner even when that owner has no startup activation.

## Rebuild and removal behavior

Startup intent is declarative source-unit content. Its state transitions are:

```text
activation absent
  → Quadlet source has no [Install]
  → generator output has no startup target symlink

activation absent → startup
  → rebuild renders [Install]
  → generator reload creates the target wants symlink
  → next target/manager startup requests the workload

activation startup → absent
  → rebuild omits [Install]
  → generator reload omits the target wants symlink
  → next target/manager startup does not request the workload
```

A daemon or generator reload while the selected target is already active does
not guarantee an immediate start. Graft does not add an imperative start to make
that happen. Host activation tooling may have separate restart behavior, but it
is not part of this contract.

Removing startup intent does not stop an already running workload. Removing the
workload definition or setting `deploy.enable = false` removes future
materialisation but must not implicitly delete persistent state, mounted data,
workspaces, or foreign units. Explicit runtime reconciliation belongs to future
control-plane design.

## Validation contract

Resolution must reject explicit intent that cannot have the requested effect:

| Condition | Result |
| --- | --- |
| `deploy.activation` absent | valid; no `[Install]` section |
| `deploy.activation = "startup"` | valid for `long-running`, `job`, and `setup` |
| any other activation value | field-specific unsupported-value error |
| `deploy.enable = false` with `activation = "startup"` | valid dormant intent; the modules render no unit or target link |
| future native timer plus `activation = "startup"` | error in the first timer implementation |
| configured raw `config.quadlet.install` | error directing users to typed activation |
| user startup without host linger | valid TOML; host diagnostic, not resolver mutation |

The resolver accepts no arbitrary unit names. Control characters, path
components, aliases, templates, and target injection are therefore impossible
through the simple startup field.

## Advanced install relationships

Quadlet 5.8.2 supports `Alias`, `WantedBy`, `RequiredBy`, and `UpheldBy`, but the
first Graft contract exposes only the fixed `WantedBy` mapping above:

- arbitrary `WantedBy` would make target policy user-injected systemd syntax;
- `RequiredBy` changes reverse failure coupling and remains outside the current
  typed dependency contract;
- `UpheldBy` adds continuous activation semantics that overlap lifecycle and
  restart policy;
- `Alias` changes unit identity and must wait for the identity contract in
  [#107](https://github.com/Patrick-Kappen/graft/issues/107).

Raw `[Install]` passthrough is forbidden by the
[Capability policy](capability-policy.md). It is not an interim
path for startup activation.

## Implemented scope and checks

[#132](https://github.com/Patrick-Kappen/graft/issues/132) implements:

- the typed field in Rust parser types and the generated v1 schema;
- fixed target resolution before JSON reaches Nix;
- field-specific rejection of reserved raw install intent;
- mechanical `[Install]` rendering in NixOS and Home Manager;
- absence, disabled dormant intent, and all three lifecycle combinations;
- real Podman 5.8.2 generator assertions for
  `multi-user.target.wants/<name>.service` and
  `default.target.wants/<name>.service`;
- negative generator assertions proving absent intent creates no target link;
- generator rerun assertions for relationship addition and removal without
  `systemctl enable` or changes outside generator output;
- `systemd-analyze verify` over the complete generated unit sets;
- regressions proving timer jobs remain without startup intent and typed
  dependency activation remains independent.

[#196](https://github.com/Patrick-Kappen/graft/issues/196) adds an isolated
x86_64 NixOS VM that validates:

- normal system startup for long-running, finite job, and retained setup
  workloads;
- a rootless user manager started at boot through declarative linger;
- a non-lingering rootless user manager started and stopped through a real tty
  login session;
- dependency activation of a shared-network owner without startup intent;
- live startup-intent removal without stopping running workloads;
- reboot into the no-startup specialisation and a later full reboot after
  re-adding startup intent;
- preservation of persistent markers, a mounted workspace, and a foreign
  systemd unit throughout the transitions.

Build the manager-level test locally with:

```bash
nix build .#packages.x86_64-linux.activation-runtime-test --no-link --print-build-logs
```

The corresponding `activation-runtime` CI job is advisory and is deliberately
excluded from the aggregate required checks while its cost and runner stability
are evaluated.

With the tested Podman 5.8.2 and systemd 260.2 combination, a linger-started
rootless Quadlet workload has intermittently entered `Result=protocol` during
user-manager bootstrap. The generated service follows Quadlet's `Type=notify`,
`NotifyAccess=all`, and `--sdnotify=conmon` contract, and the test reports the
terminal result immediately rather than retrying or masking it as a timeout.
The focused reproducer has not recreated the failure, so the compatibility
boundary remains advisory and is tracked by
[#212](https://github.com/Patrick-Kappen/graft/issues/212) under the broader
version matrix in [#129](https://github.com/Patrick-Kappen/graft/issues/129).

Run the focused reproducer with:

```bash
nix build .#packages.x86_64-linux.notify-protocol-runtime-test --no-link --print-build-logs
```

## Upstream evidence

This design was checked against Podman/Quadlet 5.8.2 and systemd's unit model:

- Quadlet documents that generated services cannot use normal persistent
  `systemctl enable`, and that it applies `[Install]` itself during generation:
  [Podman documentation](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/docs/source/markdown/podman-systemd.unit.5.md#L122-L143).
- The generator translates `WantedBy`, `RequiredBy`, and `UpheldBy` into
  `.wants`, `.requires`, and `.upholds` symlinks:
  [generator source](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/cmd/quadlet/main.go#L226-L290).
- Podman's fixture verifies the generated install symlinks for every supported
  relationship:
  [upstream fixture](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/test/e2e/quadlet/install.container#L1-L26).
- systemd 260 defines `WantedBy=` as the install-time reverse of a weak `Wants=`
  dependency and distinguishes it from ordering:
  [`systemd.unit`](https://www.freedesktop.org/software/systemd/man/260/systemd.unit.html#%5BInstall%5D%20Section%20Options).
