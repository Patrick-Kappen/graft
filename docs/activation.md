# Workload startup activation

> **Status:** design approved; implementation remains tracked by
> [#132](https://github.com/Patrick-Kappen/graft/issues/132). The current schema,
> resolver, and modules do not accept or render this field yet.

Graft separates workload startup policy from process lifecycle and dependency
activation. A rendered workload remains manually, externally, or
dependency-activated unless the user explicitly asks for startup activation.

## Intent contract

The approved TOML shape is:

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
| Select the service manager | `deploy.target` | System/rootful or user/rootless materialisation |
| Request at manager startup | `deploy.activation` | Optional startup relationship |
| Describe process behavior | `config.service.lifecycle` | Long-running service, finite job, or retained setup |
| Start through another workload | typed reference | Dependency activation owned by the relevant resource contract |
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

For a user workload, `startup` means user-manager startup, not an unconditional
host-boot guarantee:

- with declarative linger, the user manager can start during host boot;
- without linger, it normally starts when the user logs in;
- if no user manager runs, no user workload can be started by its default target.

Linger, user creation, login sessions, and manager availability are host policy.
Graft TOML does not mutate them. Future `graft doctor` diagnostics in
[#101](https://github.com/Patrick-Kappen/graft/issues/101) may report that a
requested rootless startup workload lacks the required host policy, but must not
enable linger implicitly.

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
Typed workload ordering remains owned by
[#133](https://github.com/Patrick-Kappen/graft/issues/133).

## Dependency and ordering boundary

`WantedBy=` creates a weak reverse `Wants=` relationship from the selected
target. It does not add workload-specific `After=`, `Requires=`, `PartOf=`, or
readiness behavior.

Consequences:

- startup workloads may activate in parallel;
- a workload failure does not by itself fail the target startup transaction;
- application readiness remains separate from startup activation;
- stopping a target is not Graft's workload stop contract;
- one workload that needs another must use a typed resource or dependency
  contract rather than relying on target order.

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
- `RequiredBy` changes failure coupling and belongs with typed dependencies;
- `UpheldBy` adds continuous activation semantics that overlap lifecycle and
  restart policy;
- `Alias` changes unit identity and must wait for the identity contract in
  [#107](https://github.com/Patrick-Kappen/graft/issues/107).

Raw `[Install]` passthrough remains subject to the dangerous-capability policy in
[#128](https://github.com/Patrick-Kappen/graft/issues/128). It is not an interim
path for startup activation.

## Implementation requirements for #132

The implementation must cover the complete path:

- add the typed field to Rust parser types and the generated v1 schema;
- validate activation, enable state, lifecycle, and reserved raw install intent
  in the resolver;
- resolve the fixed target relationship before JSON reaches Nix;
- render `[Install]` identically in NixOS and Home Manager;
- preserve no `[Install]` output when activation is absent;
- regenerate the tracked schema deterministically;
- document the field as supported only when the implementation lands.

Required tests are:

- parser, schema, resolver, and resolved-JSON tests for absent, valid, disabled,
  and unsupported activation intent;
- equivalent system and user Nix fixtures;
- long-running, job, and setup lifecycle combinations;
- real Podman 5.8.2 generator assertions for
  `multi-user.target.wants/<name>.service` and
  `default.target.wants/<name>.service`;
- negative generator assertions proving absent intent creates no target link;
- `systemd-analyze verify` over the complete generated unit sets;
- system boot and rootless user-manager startup tests, with linger supplied only
  by the host fixture;
- rebuild tests proving relationship creation/removal without invoking
  `systemctl enable`;
- removal tests proving persistent data is untouched and no foreign unit is
  deleted;
- a regression proving a typed dependency can start a workload independently of
  startup intent;
- a regression proving a timer-triggered job remains timer-owned.

Runtime tests must distinguish generator relationship creation from immediate
reconciliation of an already active manager. They must not pass because NixOS or
Home Manager happens to start changed units as an unrelated activation policy.

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
- systemd defines `WantedBy=` as the install-time reverse of a weak `Wants=`
  dependency and distinguishes it from ordering:
  [`systemd.unit`](https://www.freedesktop.org/software/systemd/man/latest/systemd.unit.html#%5BInstall%5D%20Section%20Options).
