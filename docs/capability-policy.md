# Capability policy

> **Status:** this policy classifies Graft configuration authority. It does not
> make planned fields available. The generated schema and
> [Capability status](capabilities.md) remain the current-input contract.

Graft covers useful Podman and systemd behavior through typed intent rather than
raw upstream passthrough. This policy decides which concepts can become normal
Graft features, which require an unmistakable security-sensitive contract, and
which do not belong in Graft TOML.

The policy applies under the authority model in the
[Threat model and trust boundaries](threat-model.md): every selected TOML file is
trusted with the authority of its effective target. Classification reduces
accidental or hidden authority; it is not an authorization boundary against an
actor who can change activated configuration.

## Two independent axes

Capability class and implementation availability are separate.

| Class | Meaning |
| --- | --- |
| **First-class** | A dedicated Graft concept with a narrow, typed contract suitable for ordinary reviewed configuration. First-class does not mean harmless or host-independent. |
| **Dangerous** | Intent that exposes unusual host, manager, namespace, privilege, or cross-workload authority. It may become available only through a dedicated explicit contract with target-specific policy. |
| **Forbidden** | An unrestricted escape hatch or host-execution path that Graft TOML will not accept. A concrete use case must become a separately classified typed concept instead. |

| Availability | Meaning |
| --- | --- |
| **Current** | Implemented through parser, resolver, resolved JSON, materialiser, documentation, and applicable tests. |
| **Planned** | Owned by an approved issue but unavailable until its complete contract is implemented. |
| **Deferred** | Recognised possible scope without an approved implementation contract. |

A dangerous capability can be current, and a first-class capability can still be
planned. A deferred concept may remain unclassified, but it must receive a class
before an implementation contract is approved. Forbidden capability classes
have no availability state because the raw input path itself is rejected.

## First-class requirements

A first-class capability must:

1. use dedicated typed TOML rather than an upstream argument or free-form map;
2. validate cross-field rules and delimiter safety before resolved JSON;
3. preserve explicit target authority without silently widening it;
4. expose concrete effective intent in deterministic resolved output;
5. be rendered mechanically by the Nix modules through Graft-owned keys;
6. document upstream defaults and host responsibilities that Graft cannot
   inspect; and
7. have negative tests plus generator or runtime evidence appropriate to its
   effect.

Omitted first-class intent may use a documented Graft or upstream default only
when that absence is unambiguous. Successful schema validation means current
supported intent, not merely parser-recognised future syntax.

A qualified Container Device Interface (CDI) name is a current first-class
resource reference implemented through [#203]. Graft accepts only a validated,
colon-free qualified name without direct paths, target remapping, or
permissions. The host administrator owns the CDI registry and its specs. A
selected CDI spec can inject devices, mounts, environment values, and hooks into
the OCI configuration, so review of the host-provided spec remains outside
Graft's build-time validation.

## Dangerous requirements

Dangerous intent must satisfy all first-class requirements and also:

1. use a dedicated field or enum variant whose purpose makes the authority
   expansion clear;
2. never be enabled by omission, an unrelated option, inference, or an
   unrestricted map;
3. define system/rootful, non-root user/rootless, and root-owned user/rootful
   behavior separately;
4. reject unsupported combinations and policy downgrades during normal
   resolution, regardless of `validation.level` or future lint settings;
5. keep effective values and relaxations visible in resolved or inspectable
   output;
6. identify affected `GRAFT-TM-*` invariants and whether authority expands; and
7. include negative tests for accidental activation, target crossing,
   injection, incompatible controls, and unsafe defaults.

A future `graft lint` warning can make dangerous intent more prominent, but a
warning never authorizes unsupported input and never replaces fail-closed
resolution. Future merge or inheritance support must preserve the origin and
effective value of dangerous intent rather than making it implicit.

Current narrow passthroughs that predate this policy are not automatically safe.
One explicit legacy exception violates dangerous requirement 2: a source-backed
`Volume=` entry without `mode = "ro"` can become writable through the upstream
default. This is not precedent for new features. [#142] and [#163] must make
writable authority explicit or reject it and provide migration diagnostics.
Until then, host-path, sensitive-source, or writable-host volumes remain current
dangerous residual risk. Host environment-file path references and exact
external-systemd-unit relationships are other explicit current host crossings;
their policy and diagnostics are tracked by [#143], [#166], and [#171].

## Forbidden boundaries

The following input paths are forbidden:

- raw Quadlet `[Unit]`, `[Container]`, `[Service]`, or `[Install]` maps;
- arbitrary Podman global or container arguments;
- host `ExecStart*`, `ExecStop*`, shell, or command hooks;
- arbitrary Nix expressions, imports, overlays, or package repositories in
  workload TOML;
- workload intent that mutates host accounts, linger, firewall, or login policy;
- generic systemd relationship maps or unit injection;
- disabling mandatory parse, validation, or fail-closed checks; and
- raw overrides for keys owned by Graft, including materialised identity,
  rootfs mechanics, generated package/store bindings, lifecycle translation,
  and startup targets.

Forbidden means the generic path remains rejected even when an operator accepts
the risk. A specific need may return as first-class or dangerous typed intent,
but it does not make the original escape hatch acceptable.

## Classification matrix

| Capability or input path | Class | Availability | Decision |
| --- | --- | --- | --- |
| Current typed identity, package, command, lifecycle, startup, workload-dependency, and safe namespace intent | First-class | Current | Keep schema-backed, resolved, and mechanically rendered. Individual host crossings retain their documented boundaries. |
| System versus user manager target selection | First-class | Current | This selects authority context, not guaranteed privilege: system and root-owned user managers are rootful; secure target defaults remain in [#139]. |
| Qualified CDI resource name without target remapping or permissions | First-class | Current through [#203] | Host registry/spec is trusted; Graft validates and renders only the colon-free qualified name. |
| Direct host device paths or directories, optional-device prefixes, target remapping, and permission modes | Dangerous | Deferred to [#142] and [#164] | Requires explicit device policy, target-specific behavior, and runtime authorization tests. |
| Host-path, sensitive-source, or writable-host mounts, recursive propagation, and host sockets | Dangerous | Partly current; policy planned in [#142] and [#163] | Omitting `mode = "ro"` on a source-backed current volume can select an upstream writable default. This legacy exception violates dangerous requirement 2 and must become explicit or fail closed; it is not precedent for broader passthrough. |
| Host environment-file path references | Dangerous | Current; credential replacement planned in [#143] and [#166] | One ordered non-empty, control-free path value per entry; Quadlet resolves relative paths against the source-unit directory. Graft does not attest traversal, symlinks, existence, ownership, permissions, lifecycle, or disclosure. |
| Exact external systemd unit relationships | Dangerous | Current | Exact validated names only; implementation and authorization of the selected-manager unit remain host responsibility. |
| Privileged containers | Dangerous | Deferred; [#163] keeps unsupported privileged intent rejected | No generic runtime argument path or implied opt-in is permitted. |
| Capability additions | Dangerous | Policy planned in [#139], approved controls implemented by [#163] | Capability drops and secure defaults are separate first-class controls. |
| Host network namespace sharing | Dangerous | Planned in [#193] | Explicit typed host mode and target-specific exposure rules are required. |
| PID, IPC, UTS, user, or cgroup namespace sharing | Dangerous | Deferred | Namespace-specific intent and target rules are required before an implementation issue is approved. |
| Unconfined seccomp, disabled labels, unmasked host paths, and equivalent sandbox relaxations | Dangerous | Policy planned in [#139], approved controls implemented by [#163] | Relaxations must be explicit and visible beside effective secure controls. Additional confinement remains first-class. |
| Automatic per-container user namespaces | First-class | Planned in [#139] and [#141] | Effective host/container identities and rootless/rootful behavior must be resolved together. |
| Custom UID/GID maps and subordinate-ID selection | Dangerous | Deferred within [#140] and [#141] | Ownership authority, range conflicts, and mount translation require explicit policy. |
| Failure handlers, conflicts, reverse lifecycle propagation, and stop effects on external units | Dangerous | Deferred | Activation and reverse effects require a concrete typed use case and graph validation. |
| Raw Quadlet, Podman/systemd arguments, host commands, shell, and arbitrary unit maps | Forbidden | — | These bypass typed intent and Graft-owned rendering. |
| Arbitrary Nix or package-repository evaluation from workload TOML | Forbidden | — | The host configuration owns package sources and evaluation. |
| Raw overrides of Graft-owned identity, rootfs, lifecycle, dependency, or startup keys | Forbidden | — | Use the corresponding typed Graft concept or a separately approved design. |

## Schema, resolver, and diagnostics contract

The generated schema contains only current implemented intent. Planned and
deferred parser fields stay excluded. Unknown fields fail parsing; explicitly
configured reserved fields fail normal resolution with their exact TOML path.
Forbidden paths remain absent from the schema and fail parsing or resolution.

When a dangerous capability becomes current, schema inclusion means only that
its explicit typed contract is implemented. It does not reclassify the
capability as first-class. The capability matrix must show both its class and
availability, and reference documentation must identify the effective authority.

`graft lint` may later add error, warning, and informational diagnostics. It must
reuse resolver semantics, cannot downgrade mandatory errors, and cannot turn a
forbidden or unavailable capability into accepted configuration.

## Review checklist

Every design or implementation that changes capability coverage must answer:

1. What is the capability class and availability?
2. Which target authority and `GRAFT-TM-*` invariants change?
3. Can omission, defaults, merging, or dependencies activate it accidentally?
4. What concrete intent appears in resolved output?
5. Which keys remain owned by Graft and the upstream generator?
6. Which negative, generator, and runtime tests prove the boundary?
7. Which capability, reference, schema, threat-model, and non-goal text changes?

If these answers are incomplete, the field remains unavailable and fails closed.

[#139]: https://github.com/Patrick-Kappen/graft/issues/139
[#140]: https://github.com/Patrick-Kappen/graft/issues/140
[#141]: https://github.com/Patrick-Kappen/graft/issues/141
[#142]: https://github.com/Patrick-Kappen/graft/issues/142
[#143]: https://github.com/Patrick-Kappen/graft/issues/143
[#163]: https://github.com/Patrick-Kappen/graft/issues/163
[#164]: https://github.com/Patrick-Kappen/graft/issues/164
[#166]: https://github.com/Patrick-Kappen/graft/issues/166
[#171]: https://github.com/Patrick-Kappen/graft/issues/171
[#193]: https://github.com/Patrick-Kappen/graft/issues/193
[#203]: https://github.com/Patrick-Kappen/graft/issues/203
