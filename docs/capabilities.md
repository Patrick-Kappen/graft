# Capability status

This page is the authoritative status boundary between current Graft intent,
reserved parser fields, dangerous capabilities, forbidden passthrough, and
future design. The [Reference](reference.md) explains current field semantics; the generated
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
| Future direction and exclusions | [Roadmap](roadmap.md), [Vision](vision.md), and [Non-goals](non-goals.md) | Must not be presented as runnable current syntax |
| Upstream implementation details | Versioned links in [Tested upstream context](#tested-upstream-context) | Compatibility expansion remains owned by [#129] |

## Status definitions

| Status | Meaning |
| --- | --- |
| **Current** | Typed, validated, resolved, mechanically materialised, documented, and covered by the applicable tests. |
| **Planned** | Owned by an approved implementation issue but unavailable in normal configuration. |
| **Deferred** | Recognised as possible future scope, without an implemented contract. |
| **Dangerous** | Unavailable security-sensitive intent that requires typed policy and explicit review before implementation. |
| **Forbidden** | Not a Graft input path; unrestricted passthrough or host execution will not be accepted. |

A parser-recognised field is not automatically supported. Normal resolution
fails closed when any reserved field is explicitly configured, including
`false`, zero, and empty leaf values. Empty parent tables remain valid when none
of their fields are set. Unknown fields and unsupported enum values fail TOML
parsing or schema validation earlier.

## Current Graft v1 fields

Every supported semantic field path below is accepted by the generated v1
schema. Structural parent tables carry no independent intent. **Both** means NixOS
system/rootful and Home Manager user/rootless targets. A dash means the field is
consumed before that pipeline stage and intentionally has no output there.

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
| `config.runtime.packages` | Optional ordered non-empty Nix package names | `runtime.packages`, always including `graft-pause` | Resolves names from the target `pkgs` and builds the rootfs | Packages become available on the container path | Both | Current |
| `config.runtime.command` | Optional non-empty argv with non-empty, control-free entries | `runtime.command`; defaults to `/bin/graft-pause` | Passed to the shared renderer | Quoted `Exec=` | Both | Current |
| `config.container.hostname` | Optional non-empty, control-free literal | `container.hostname` | Passed through mechanically | `HostName=` | Both | Current |
| `config.container.user` | Optional non-empty, control-free literal | `container.user` | Passed through mechanically | `User=` | Both | Current |
| `config.container.group` | Optional non-empty, control-free literal; requires `user` | `container.group` | Passed through mechanically | `Group=` | Both | Current |
| `config.container.workingDir` | Optional non-empty, control-free literal; no existence check | `container.workingDir` | Passed through mechanically | `WorkingDir=` | Both | Current |
| `config.container.environment` | Optional map; keys are non-empty, control-free, contain no whitespace or `=`; values are control-free | Sorted `container.environment` map | Passed to the shared renderer | Quoted, sorted `Environment="KEY=value"` | Both | Current |
| `config.container.environmentFile` | Optional ordered list of non-empty, control-free paths | `container.environmentFile` | Passed to the shared renderer | Ordered, quoted `EnvironmentFile=` | Both | Current |
| `config.filesystem.volumes` | Optional ordered list; empty is omitted | `filesystem.volumes` | Passed to the shared renderer | Ordered `Volume=` | Both | Current |
| `config.filesystem.volumes[].source` | Optional non-empty, control-free literal without `:` | `filesystem.volumes[].source` | Joined mechanically with target and mode | Source component of `Volume=` | Both | Current |
| `config.filesystem.volumes[].target` | Required non-empty, control-free literal without `:` | `filesystem.volumes[].target` | Joined mechanically with source and mode | Target component of `Volume=` | Both | Current |
| `config.filesystem.volumes[].mode` | Optional non-empty, control-free literal without `:`; requires source | `filesystem.volumes[].mode` | Joined mechanically with source and target | Mode/options component of `Volume=` | Both | Current |
| `config.network.mode` | Optional `none` or `container`; absence preserves the Podman default; incompatible with `publish` when set | Optional typed `network.namespace` | Passed to the shared renderer | No line, `Network=none`, or a resolved `.container` reference | Both | Current |
| `config.network.container` | Required safe Graft workload name for container mode; validates existence, target, lifecycle, enablement, self-reference, and cycles | `network.namespace.unit` | Passed to the shared renderer | `Network=<source>.container`; Quadlet adds dependencies | Both | Current |
| `config.network.publish` | Optional ordered non-empty, control-free literals; only with implicit default mode | `network.publish` | Passed to the shared renderer | Ordered `PublishPort=` | Both | Current |
| `config.service.lifecycle` | Optional `long-running`, `job`, or `setup`; finite modes require an explicit command and restrict restart policy | Optional `service.type` and `service.remainAfterExit` | Passed to the shared renderer | `Type=` and finite `RemainAfterExit=` | Both | Current |
| `config.service.restart` | Optional supported systemd restart policy; finite lifecycle restrictions apply | `service.restart` | Passed to the shared renderer | `Restart=` | Both | Current |
| `config.service.restartSec` | Optional non-empty, control-free literal; requires restart other than `no` | `service.restartSec` | Passed to the shared renderer | `RestartSec=` | Both | Current |
| `config.service.timeoutStartSec` | Optional non-empty, control-free literal | `service.timeoutStartSec` | Passed to the shared renderer | `TimeoutStartSec=` | Both | Current |
| `config.service.timeoutStopSec` | Optional non-empty, control-free literal | `service.timeoutStopSec` | Passed to the shared renderer | `TimeoutStopSec=` | Both | Current |
<!-- supported-schema-fields:end -->

Detailed lifecycle, startup, and namespace combinations live in
[Workload lifecycle semantics](lifecycle.md),
[Workload startup activation](activation.md), and
[Container network intent](networking.md). Renderer quoting and generator-owned
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
| `config.filesystem.readOnly`, `readOnlyTmpfs`, `tmpfs`, `mounts`, and `devices` | Field-specific resolver error | Planned mount policy and implementation: [#142] and [#164] |
| `config.network.dns`, `dnsOption`, `dnsSearch`, and `addHost` | Field-specific resolver error | Planned network Phase B: [#193] |
| `config.networks`, including nested labels and raw maps | Field-specific resolver error | Planned typed `.network` resources: [#147] |
| `config.volumes`, including nested labels and raw maps | Field-specific resolver error | Planned typed `.volume` resources: [#148] |
| `config.security.*` | Field-specific resolver error | Current boundary: [Threat model](threat-model.md); policy and secure controls: [#128], [#139], and [#163] |
| `config.resources.*` | Field-specific resolver error | Planned limits: [#145] |
| `config.secrets` | Field-specific resolver error | Planned credential pipeline: [#143] and [#166] |
| `config.workspace.*`, `config.home.*`, and `config.attach.*` | Field-specific resolver error | Deferred workspace/instance design: [#151], [#153], and [#160] |
| `config.service.restartIfChanged` | Field-specific resolver error | Deferred service reconciliation behavior |
| `config.service.type`, `config.service.remainAfterExit` | Migration error directing users to typed `lifecycle` | Forbidden as alternate raw lifecycle syntax |
| `config.quadlet.container`, `service`, and `install` | Raw-map error; install points to typed `deploy.activation` | Forbidden raw Quadlet passthrough |

## Dangerous and forbidden boundaries

Dangerous means unavailable, not “accepted with a warning.” Current assumptions
and residual risks are defined in the [Threat model](threat-model.md);
classification and future opt-in policy remain owned by
[#128](https://github.com/Patrick-Kappen/graft/issues/128).

| Capability | Current input result | Status |
| --- | --- | --- |
| Host networking and other host namespace sharing | Unsupported enum or field-specific error | Dangerous; policy pending [#128], network design in [#193] |
| `privileged`, capability additions, host devices, unconfined seccomp/labels, and user namespaces | Field-specific error | Dangerous; typed policy required before implementation |
| Writable host mounts | Literal `config.filesystem.volumes` currently permits a mode without `ro`; paths and policy are not attested | Current narrow passthrough with security policy pending [#142]/[#163]; review explicitly |
| `PodmanArgs`, `GlobalArgs`, and raw Quadlet maps | Field-specific error | Forbidden as unrestricted passthrough; future needs require typed intent |
| `Conflicts=`, `Upholds=`, failure handlers, and stop propagation to external units | Unknown field or raw-map error | Dangerous; activation and reverse lifecycle effects require typed policy under [#128] |
| `Requisite=` and reload propagation | Unknown field or raw-map error | Deferred until a concrete typed use case exists |
| Raw `[Unit]`, `[Service]`, `[Install]`, host `ExecStart*`/`ExecStop*`, and host shell | Unknown field or raw-map error | Forbidden; only reviewed typed dependencies, lifecycle, timing, and startup intent may produce their fixed directives |
| Arbitrary Nix expressions or package repositories in TOML | Unknown field or package lookup error | Forbidden; the trusted host package set owns evaluation |

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

Current generator fixtures use **Podman/Quadlet 5.8.2** and **systemd 260** from
the project's pinned Nix environment. These are tested versions, not yet a
formal minimum-version promise. The maintained compatibility contract, upgrade
diffs, cgroup v2 prerequisites, and unsupported-version diagnostics remain in
[#129](https://github.com/Patrick-Kappen/graft/issues/129).

Authoritative references used by the current implementation:

- [Podman 5.8.2 Quadlet documentation](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/docs/source/markdown/podman-systemd.unit.5.md)
- [Podman 5.8.2 `.container` generator source](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/pkg/systemd/quadlet/quadlet.go)
- [systemd 260 service documentation](https://www.freedesktop.org/software/systemd/man/260/systemd.service.html)
- [systemd 260 unit documentation](https://www.freedesktop.org/software/systemd/man/260/systemd.unit.html)
- [systemd 260 timer documentation](https://www.freedesktop.org/software/systemd/man/260/systemd.timer.html)
- [NixOS 26.05 Podman module source](https://github.com/NixOS/nixpkgs/blob/nixos-26.05/nixos/modules/virtualisation/podman/default.nix)
- [the repository's exact nixpkgs lock](https://github.com/Patrick-Kappen/graft/blob/main/flake.lock)

Host installations may use a different version through their own nixpkgs pin.
The capability and compatibility claims above apply only to the versions that
Graft's tests actually exercise.

[#107]: https://github.com/Patrick-Kappen/graft/issues/107
[#128]: https://github.com/Patrick-Kappen/graft/issues/128
[#129]: https://github.com/Patrick-Kappen/graft/issues/129
[#139]: https://github.com/Patrick-Kappen/graft/issues/139
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
