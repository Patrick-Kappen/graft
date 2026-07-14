# Capability status

This page is the authoritative availability boundary between current Graft
intent, reserved parser fields, and future design. The
[Capability policy](capability-policy.md) separately defines first-class,
dangerous, and forbidden classes. The [Reference](reference.md) explains current
field semantics; the generated
[Graft v1 JSON Schema](https://github.com/Patrick-Kappen/graft/blob/main/crates/graft/schema/graft-v1.schema.json)
is the machine-readable current-input contract.

## Documentation source map

| Purpose | Authoritative source | Drift protection |
| --- | --- | --- |
| Runnable NixOS workload | [NixOS quickstart](quickstart/nixos.md) and its tracked TOML fixture | JSON Schema, module evaluation, real Quadlet generation |
| Runnable Home Manager workload | [Home Manager quickstart](quickstart/home-manager.md) and its tracked TOML fixture | JSON Schema, module evaluation, real Quadlet generation |
| Current human configuration semantics | [Reference](reference.md) | Capability/schema path comparison plus documentation checks |
| Current machine-readable input | [Graft v1 JSON Schema](https://github.com/Patrick-Kappen/graft/blob/main/crates/graft/schema/graft-v1.schema.json) | Generated-versus-tracked Rust schema test |
| Capability and pipeline status | This page | `documentation-drift` rejects missing, extra, or duplicate current paths |
| Current security assumptions and invariants | [Threat model and trust boundaries](threat-model.md) | Invariants link to their implementation and test evidence |
| Capability class and implementation gates | [Capability policy](capability-policy.md) | Security-sensitive work must classify authority before schema inclusion |
| Future direction and exclusions | [Roadmap](roadmap.md), [Vision](vision.md), and [Non-goals](non-goals.md) | Must not be presented as runnable current syntax |
| Upstream implementation details | Versioned links in [Tested upstream context](#tested-upstream-context) | Compatibility expansion remains owned by [#129] |

## Availability and class

| Availability | Meaning |
| --- | --- |
| **Current** | Typed, validated, resolved, mechanically materialised, documented, and covered by the applicable tests. |
| **Planned** | Owned by an approved implementation issue but unavailable in normal configuration. |
| **Deferred** | Recognised as possible future scope, without an implemented contract. |

| Class | Meaning |
| --- | --- |
| **First-class** | Dedicated typed intent suitable for ordinary reviewed configuration. |
| **Dangerous** | Dedicated explicit intent that exposes unusual host, manager, namespace, or privilege authority. |
| **Forbidden** | An unrestricted escape hatch or host-execution path that Graft TOML will not accept. |

Class and availability are independent: a dangerous capability can be current,
and a planned capability can be first-class. See the
[Capability policy](capability-policy.md) for implementation gates and the
classification matrix.

A parser-recognised field is not automatically supported. Normal resolution
fails closed when any reserved field is explicitly configured, including
`false`, zero, and empty leaf values. Empty parent tables remain valid when none
of their fields are set. Unknown fields and unsupported enum values fail TOML
parsing or schema validation earlier.

## Current Graft v1 fields

Every supported semantic field path below is accepted by the generated v1
schema. Structural parent tables carry no independent intent. **Both** means
NixOS system-manager and Home Manager user-manager targets. The system target is
rootful; the user target is rootless only when Home Manager runs for a non-root
account. A dash means the field is consumed before that pipeline stage and
intentionally has no output there.

<!-- supported-schema-fields:start -->
| TOML field | Input and semantic validation | Resolved JSON | Nix materialisation | Quadlet/systemd output | Targets | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `version` | Required integer; exactly `1` | Consumed during validation | — | — | Both | Current |
| `name` | Required safe container name; keep equal to the TOML filename stem until [#107] | `name` | Selects rootfs/container identity | `ContainerName=` | Both | Current |
| `dependencies` | Optional typed dependency list; empty is omitted; duplicate or cyclic workload targets fail | Optional concrete `dependencies` relation lists | Passed to the shared renderer | Optional `[Unit]` section | Both | Current |
| `dependencies[].target.workload` | Required safe Graft name for a workload target; validates existence, target, enablement, self-reference, ambiguity, and cycles | Concrete `.container` source-unit identity in applicable relation lists | Passed through mechanically | Quadlet translates the source unit to its generated service | Both | Current |
| `dependencies[].target.externalUnit` | Exact concrete systemd unit name; strict line, character, suffix, length, and template validation; manager existence is not inspected | Concrete external identity in applicable relation lists | Passed through mechanically | Exact selected-manager unit identity | Target-specific | Current |
| `dependencies[].requirement` | Optional `required` or `optional`; every dependency needs at least one relationship axis | `dependencies.requires` or `dependencies.wants` | Passed through mechanically | `Requires=` or `Wants=` | Both | Current |
| `dependencies[].ordering` | Optional `after` or `before`; no ordering is inferred | `dependencies.after` or `dependencies.before` | Passed through mechanically | `After=` or `Before=` | Both | Current |
| `dependencies[].lifecycle` | Optional `part-of` or `bound`; `bound` already activates its target, rejects a separate requirement, and cannot target a Graft `job` | `dependencies.partOf` or `dependencies.bindsTo` | Passed through mechanically | `PartOf=` or `BindsTo=` | Both | Current |
| `deploy.enable` | Optional boolean; absence means materialise | Optional `deploy.enable` | Filters materialisation when `false` | No unit when disabled | Both | Current |
| `deploy.target` | `system` or `user`; defaults to `system` | Effective `deploy.target` | Selects NixOS or Home Manager output | Selects system or user manager | Both | Current |
| `deploy.activation` | Optional `startup`; no aliases or arbitrary targets | `install.wantedBy` | Renders a fixed install relationship | `WantedBy=multi-user.target` or `default.target` | Target-specific | Current |
| `config.runtime.mode` | Optional; only `rootfs-store`, which is also the default | `runtime.mode` | Selects the Nix-store rootfs backend | `Rootfs=<store-path>:O` | Both | Current |
| `config.runtime.packages` | Optional ordered non-empty Nix package names | `runtime.packages`, always including `graft-pause` | Resolves `graft-pause` from host-selected `cfg.package`, other names from target `pkgs`, and builds the rootfs | Added to the generated rootfs path; explicit mounts may hide paths | Both | Current |
| `config.runtime.command` | Optional non-empty argv with non-empty, control-free entries; `job` and `setup` require it | `runtime.command`; defaults to `/bin/graft-pause` only for implicit or long-running lifecycle | Passed to the shared renderer | Quoted `Exec=` | Both | Current |
| `config.container.hostname` | Optional non-empty, control-free literal | `container.hostname` | Passed through mechanically | `HostName=` | Both | Current |
| `config.container.user` | Optional non-empty, control-free literal | `container.user` | Passed through mechanically | `User=` | Both | Current |
| `config.container.group` | Optional non-empty, control-free literal; requires `user` | `container.group` | Passed through mechanically | `Group=` | Both | Current |
| `config.container.workingDir` | Optional non-empty, control-free literal; no existence check | `container.workingDir` | Passed through mechanically | `WorkingDir=` | Both | Current |
| `config.container.environment` | Optional map; keys are non-empty, control-free, contain no whitespace or `=`; values are control-free | Sorted `container.environment` map | Passed to the shared renderer | Quoted, sorted `Environment="KEY=value"` | Both | Current |
| `config.container.environmentFile` | Optional ordered list of non-empty, control-free absolute or relative path values; existence and path safety are not checked | `container.environmentFile` | Passed to the shared renderer | Ordered, quoted `EnvironmentFile=`; Quadlet resolves relative paths against the source-unit directory | Both | Current |
| `config.filesystem.readOnly` | Optional; only `true`; `false` fails until #163 implements the approved relaxation policy | `filesystem.readOnly` when configured; no default | Passed to the shared renderer | `ReadOnly=true`; tested upstream default remains absent/false when omitted | Both | Current through partial [#163] |
| `config.filesystem.tmpfs` | Optional ordered list of unique absolute container paths; empty is omitted; control characters, `:`, terminal whitespace, and terminal `\` fail | `filesystem.tmpfs` | Passed through mechanically | One ordered writable `Tmpfs=<path>` line per entry | Both | Current path-only phase of [#164] |
| `config.filesystem.volumes` | Optional ordered list; empty is omitted; no path, mode, or target-overlap policy | `filesystem.volumes` | Passed to the shared renderer after the fixed store bind | Ordered `Volume=`; may overlap `/nix/store` or expose store paths elsewhere | Both | Current |
| `config.filesystem.volumes[].source` | Optional non-empty, control-free literal without `:` | `filesystem.volumes[].source` | Joined mechanically with target and mode | Source component of `Volume=` | Both | Current |
| `config.filesystem.volumes[].target` | Required non-empty, control-free literal without `:` | `filesystem.volumes[].target` | Joined mechanically with source and mode | Target component of `Volume=` | Both | Current |
| `config.filesystem.volumes[].mode` | Optional non-empty, control-free literal without `:`; requires source | `filesystem.volumes[].mode` | Joined mechanically with source and target | Mode/options component of `Volume=` | Both | Current |
| `config.filesystem.devices` | Optional ordered list; empty is omitted; duplicate sources fail | `filesystem.devices` | Passed to the shared renderer | Ordered `AddDevice=` lines | Both | Current |
| `config.filesystem.devices[].source` | Required colon-free qualified CDI name in `vendor/class=device` form; direct paths, malformed names, colons, and control characters fail | `filesystem.devices[].source` | Passed through mechanically | One `AddDevice=<source>` line; Quadlet translates it to one Podman `--device` argument | Both | Current |
| `config.network.mode` | Optional `none` or `container`; absence preserves the Podman default; incompatible with `publish` when set | Optional typed `network.namespace` | Passed to the shared renderer | No line, `Network=none`, or a resolved `.container` reference | Both | Current |
| `config.network.container` | Required safe Graft workload name for container mode; validates existence, target, lifecycle, enablement, self-reference, and cycles | `network.namespace.unit` | Passed to the shared renderer | `Network=<source>.container`; Quadlet adds dependencies | Both | Current |
| `config.network.publish` | Optional ordered non-empty, control-free literals; only with implicit default mode | `network.publish` | Passed to the shared renderer | Ordered `PublishPort=` | Both | Current |
| `config.security.dropCapabilities` | Optional non-empty ordered list containing `all` alone or canonical `CAP_*` names; duplicates and mixed `all` fail | `security.dropCapabilities` when configured; no default | Passed to the shared renderer | One ordered `DropCapability=` per entry | Both | Current through partial [#163] |
| `config.security.noNewPrivileges` | Optional; only `true`; `false` fails until #163 implements the approved relaxation policy | `security.noNewPrivileges` when configured; no default | Passed to the shared renderer | `NoNewPrivileges=true`; tested upstream default remains absent/false when omitted | Both | Current through partial [#163] |
| `config.service.lifecycle` | Optional `long-running`, `job`, or `setup`; finite modes require an explicit command and restrict restart policy | Optional `service.type` and `service.remainAfterExit` | Passed to the shared renderer | `Type=` and finite `RemainAfterExit=` | Both | Current |
| `config.service.restart` | Optional supported systemd restart policy; finite lifecycle restrictions apply | `service.restart` | Passed to the shared renderer | `Restart=` | Both | Current |
| `config.service.restartSec` | Optional non-empty, control-free literal; requires restart other than `no` | `service.restartSec` | Passed to the shared renderer | `RestartSec=` | Both | Current |
| `config.service.timeoutStartSec` | Optional non-empty, control-free literal | `service.timeoutStartSec` | Passed to the shared renderer | `TimeoutStartSec=` | Both | Current |
| `config.service.timeoutStopSec` | Optional non-empty, control-free literal | `service.timeoutStopSec` | Passed to the shared renderer | `TimeoutStopSec=` | Both | Current |
<!-- supported-schema-fields:end -->

Detailed lifecycle, startup, and namespace combinations live in
[Workload lifecycle semantics](lifecycle.md),
[Workload startup activation](activation.md),
[Container network intent](networking.md),
[Container Device Interface references](cdi.md), and
[Explicit container hardening](hardening.md). Renderer quoting and generator-owned
behavior live in [Quadlet output](quadlet.md).

## Reserved parser fields

These fields exist in parser types for roadmap continuity but are excluded from
the supported schema and fail normal resolution. They never disappear silently.
The paths are grouped because no resolved, Nix, or Quadlet representation exists
yet.

| Parsed paths or concept | Normal result | Status and owner |
| --- | --- | --- |
| `parents.*`, `children.*` | Field-specific resolver error | Planned configuration graph: [#159] and [#173] |
| `validation.level` | Error for `off`, `warn`, and `strict`; fail-closed behavior cannot be downgraded | Deferred until a validation-level contract exists |
| `config.runtime.packageOps.add`, `remove`, and `replace` | Field-specific resolver error | Deferred merge/package mutation design |
| `config.container.name` | Field-specific resolver error | Deferred identity contract: [#107] |
| `config.container.pod`, `entrypoint`, `stopSignal`, `stopTimeout`, `timezone`, `notify`, `runInit`, `environmentHost`, and `health.*` | Field-specific resolver error | Planned health/graceful behavior: [#146]; pod and host-environment contracts remain deferred |
| `config.container.annotations`, `ip`, `ip6`, `networkAlias`, `exposeHostPort`, `uidMap`, `gidMap`, `subUidMap`, `subGidMap`, `shmSize`, `mask`, `unmaskPaths`, `sysctl`, and `logDriver` | Field-specific resolver error | Planned or deferred through [#141], [#145], [#146], and [#193] |
| `config.container.podmanArgs` and `globalArgs` | Field-specific resolver error | Forbidden raw runtime passthrough; future needs require typed intent |
| `config.filesystem.readOnlyTmpfs` and `mounts` | Field-specific resolver error | Broader mount and root-filesystem policy remain in [#142] and [#164] |
| `config.filesystem.devices[].target` and `config.filesystem.devices[].permissions` | Indexed field-specific resolver error | Direct-device remapping and permissions remain deferred to [#142] and [#164] |
| `config.network.dns`, `dnsOption`, `dnsSearch`, and `addHost` | Field-specific resolver error | Planned network Phase B: [#193] |
| `config.networks`, including nested labels and raw maps | Field-specific resolver error | Planned typed `.network` resources: [#147] |
| `config.volumes`, including nested labels and raw maps | Field-specific resolver error | Planned typed `.volume` resources: [#148] |
| `config.security.addCapabilities`, `privileged`, `seccompProfile`, `securityLabelDisable`, `securityLabelFileType`, `securityLabelLevel`, `securityLabelNested`, `securityLabelType`, `securityOpt`, and `userns` | Field-specific resolver error | Typed canonical `addCapabilities` is approved by the [secure defaults design](secure-defaults.md) but remains unavailable until [#163]; the other fields retain their documented deferred or forbidden boundaries |
| `config.resources.*` | Field-specific resolver error | Planned limits: [#145] |
| `config.secrets` | Field-specific resolver error | Planned credential pipeline: [#143] and [#166] |
| `config.workspace.*`, `config.home.*`, and `config.attach.*` | Field-specific resolver error | Deferred workspace/instance design: [#151], [#153], and [#160] |
| `config.service.restartIfChanged` | Field-specific resolver error | Deferred service reconciliation behavior |
| `config.service.type`, `config.service.remainAfterExit` | Migration error directing users to typed `lifecycle` | Forbidden as alternate raw lifecycle syntax |
| `config.quadlet.container`, `service`, and `install` | Raw-map error; install points to typed `deploy.activation` | Forbidden raw Quadlet passthrough |

## Classification boundaries

Current assumptions and residual risks are defined in the
[Threat model](threat-model.md). Dangerous means a capability class, not an
availability state and not “accepted with a warning.” Unavailable intent still
fails closed.

| Capability | Current input result | Class | Availability and owner |
| --- | --- | --- | --- |
| Explicit capability drops, no-new-privileges, and read-only rootfs | Non-relaxing typed values are accepted and rendered only when configured; no secure defaults or `false` relaxation values exist yet | First-class | Current through partial [#163]; concrete defaults and typed opt-outs are approved in the [secure defaults design](secure-defaults.md) but not implemented |
| Qualified CDI resource name without remapping or permissions | Colon-free qualified `source` is accepted, ordered, and rendered as `AddDevice=`; host registry/spec is not inspected | First-class | Current through [#203] |
| Direct host devices, device directories, target remapping, and permissions | Direct paths fail source validation; parser-reserved `target` and `permissions` return indexed field errors | Dangerous | Deferred to [#142] and [#164] |
| Host network namespace sharing | Unsupported `config.network.mode` value | Dangerous | Planned in [#193] |
| PID, IPC, UTS, user, or cgroup namespace sharing | Unsupported enum, unknown field, or field-specific error | Dangerous | Deferred until namespace-specific intent and target rules have an approved implementation issue |
| `privileged` | Field-specific error | Dangerous | Deferred; [#163] keeps unsupported privileged intent rejected |
| Capability additions and unconfined seccomp/labels | Field-specific error | Dangerous | Canonical capability additions are approved for the drop-all baseline in [#163]; unconfined seccomp/labels remain deferred and unavailable |
| Automatic per-container user namespaces | Field-specific error | First-class | Deferred to [#140] and [#141]; the secure-defaults phase preserves runtime behavior |
| Custom UID/GID maps and subordinate-ID selection | Field-specific error | Dangerous | Deferred within [#140] and [#141] |
| Host-path, sensitive-source, or writable-host mounts | Literal `config.filesystem.volumes` parts are delimiter- and line-safe but have no semantic source/path/mode, existence, or target-overlap policy; omitting `mode = "ro"` on a source-backed volume can select an upstream writable default | Dangerous | Current legacy exception to explicit-dangerous-intent policy; [#142] and [#164] must make writable authority explicit or reject it with migration diagnostics; [#163] preserves this exception |
| Host environment-file path references | One ordered non-empty, control-free path value is accepted per entry; Quadlet resolves relative paths against the source-unit directory | Dangerous | Current explicit host crossing without traversal, symlink, existence, ownership, permission, lifecycle, or disclosure attestation; typed credentials planned in [#143] and [#166] |
| Exact external-systemd-unit relationships | Exact typed name is accepted; selected-manager implementation is not inspected | Dangerous | Current explicit host crossing |
| `PodmanArgs`, `GlobalArgs`, and raw Quadlet maps | Field-specific error | Forbidden | No unrestricted passthrough; concrete needs require typed intent |
| `Conflicts=`, `Upholds=`, failure handlers, and stop propagation to external units | Unknown field or raw-map error | Dangerous | Deferred until a concrete typed graph contract exists |
| `Requisite=` and reload propagation | Unknown field or raw-map error | Unclassified | Deferred until a concrete typed use case is classified |
| Raw `[Unit]`, `[Service]`, `[Install]`, host `ExecStart*`/`ExecStop*`, and host shell | Unknown field or raw-map error | Forbidden | Only reviewed typed dependencies, lifecycle, timing, and startup intent may produce fixed directives |
| Disabling mandatory parse, validation, or fail-closed checks | `validation.level` field-specific error | Forbidden | Future lint levels cannot downgrade resolver errors |
| Arbitrary Nix expressions or package repositories in TOML | Unknown field or package lookup error | Forbidden | Trusted host package sources own evaluation |

## Artifact and Quadlet resource scope

| Concept | Status |
| --- | --- |
| `rootfs-store` `.container` workloads | Current flagship backend |
| Typed `.network` resources | Planned in [#147] |
| Typed `.volume` resources | Planned in [#148] |
| Typed `.pod` resources and membership | Designed through [#149]; implementation tracked by [#167] |
| `.image`, `.build`, `.kube`, and `.artifact` | Deferred pending product decision [#150]; no Graft syntax is promised |

Graft does not pursue upstream option-count parity. Consult upstream manuals for
Quadlet capabilities, then check this page before assuming that an option is a
Graft contract.

## Tested upstream context

Current generator fixtures use **Podman/Quadlet 5.8.2**, its Container Device
Interface library **1.0.1**, and **systemd 260** from the project's pinned Nix
environment. These are tested versions, not yet a
formal minimum-version promise. The maintained compatibility contract, upgrade
diffs, cgroup v2 prerequisites, and unsupported-version diagnostics remain in
[#129](https://github.com/Patrick-Kappen/graft/issues/129).

Authoritative references used by the current implementation:

- [Podman 5.8.2 Quadlet documentation](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/docs/source/markdown/podman-systemd.unit.5.md)
- [Podman 5.8.2 `.container` generator source](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/pkg/systemd/quadlet/quadlet.go)
- [CDI 1.0.1 qualified-name parser](https://github.com/cncf-tags/container-device-interface/blob/79790445c2d70820f6824eb42832d2efd0f08dd2/pkg/parser/parser.go)
- [systemd 260 service documentation](https://www.freedesktop.org/software/systemd/man/260/systemd.service.html)
- [systemd 260 unit documentation](https://www.freedesktop.org/software/systemd/man/260/systemd.unit.html)
- [systemd 260 timer documentation](https://www.freedesktop.org/software/systemd/man/260/systemd.timer.html)
- [NixOS 26.05 Podman module source](https://github.com/NixOS/nixpkgs/blob/nixos-26.05/nixos/modules/virtualisation/podman/default.nix)
- [the repository's exact nixpkgs lock](https://github.com/Patrick-Kappen/graft/blob/main/flake.lock)

Host installations may use a different version through their own nixpkgs pin.
The capability and compatibility claims above apply only to the versions that
Graft's tests actually exercise.

[#107]: https://github.com/Patrick-Kappen/graft/issues/107
[#129]: https://github.com/Patrick-Kappen/graft/issues/129
[#140]: https://github.com/Patrick-Kappen/graft/issues/140
[#141]: https://github.com/Patrick-Kappen/graft/issues/141
[#142]: https://github.com/Patrick-Kappen/graft/issues/142
[#143]: https://github.com/Patrick-Kappen/graft/issues/143
[#145]: https://github.com/Patrick-Kappen/graft/issues/145
[#146]: https://github.com/Patrick-Kappen/graft/issues/146
[#147]: https://github.com/Patrick-Kappen/graft/issues/147
[#148]: https://github.com/Patrick-Kappen/graft/issues/148
[#149]: https://github.com/Patrick-Kappen/graft/issues/149
[#150]: https://github.com/Patrick-Kappen/graft/issues/150
[#151]: https://github.com/Patrick-Kappen/graft/issues/151
[#153]: https://github.com/Patrick-Kappen/graft/issues/153
[#159]: https://github.com/Patrick-Kappen/graft/issues/159
[#160]: https://github.com/Patrick-Kappen/graft/issues/160
[#163]: https://github.com/Patrick-Kappen/graft/issues/163
[#164]: https://github.com/Patrick-Kappen/graft/issues/164
[#166]: https://github.com/Patrick-Kappen/graft/issues/166
[#167]: https://github.com/Patrick-Kappen/graft/issues/167
[#173]: https://github.com/Patrick-Kappen/graft/issues/173
[#193]: https://github.com/Patrick-Kappen/graft/issues/193
[#203]: https://github.com/Patrick-Kappen/graft/issues/203
