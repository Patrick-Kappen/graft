# Filesystem and mount policy

> **Status:** approved design for [#142]. Current behavior remains documented in
> the [Reference](reference.md) until implementation lands through [#164].

This design replaces Graft's ambiguous volume passthrough with typed filesystem
intent. It keeps ordinary writable storage concise, makes host write authority
explicit, and rejects mount layouts whose effective target depends on ordering.
It does not expose raw Quadlet `Mount=`, Podman mount strings, direct host-device
paths, or host-policy bypass flags.

## Goals

The implementation must:

- distinguish host bind mounts from Podman-managed volumes;
- make writable host access explicit and default host binds to read-only;
- represent tmpfs options without accepting arbitrary mount flags;
- reject duplicate, nested, protected, and otherwise ambiguous targets before
  materialisation;
- preserve the same typed intent for system and user targets while documenting
  their different runtime authority; and
- expose effective access in resolved JSON and future lint/inspect output.

The design affects `GRAFT-TM-01`, `GRAFT-TM-02`, `GRAFT-TM-03`,
`GRAFT-TM-05`, `GRAFT-TM-07`, `GRAFT-TM-12`, `GRAFT-TM-13`, and
`GRAFT-TM-14`. It narrows the legacy volume exception and deliberately adds
explicit dangerous host-write authority. `GRAFT-TM-12` remains unchanged:
direct host devices stay unavailable.

## Capability classification

| Intent | Class | Reason |
| --- | --- | --- |
| Anonymous Podman-managed volume | First-class | The declaration explicitly requests runtime-managed writable storage without selecting an arbitrary host path or existing named resource. |
| Named Podman volume | Dangerous | Its explicit name can reuse existing persistent host-managed state or couple workloads within one Podman storage scope. |
| Tmpfs mount with bounded mode and size | First-class | It has no host source, but can mask rootfs content and consumes host memory. |
| Read-only host bind | Dangerous | It exposes an arbitrary host path and may expose sensitive data, sockets, submounts, or changing host state. |
| Writable host bind | Dangerous | It additionally lets the workload mutate host-visible state with the selected target's authority. |
| Direct device | Dangerous and deferred | Pure resolution cannot attest that a `/dev` path is a device rather than a directory or symlink, so this design does not approve a direct-path form. |
| Raw mount strings, propagation, relabel, recursive chown, custom idmaps, and arbitrary options | Forbidden as generic input | These bypass typed policy. A future concrete use case requires a separately reviewed field. |

Omission never creates a host crossing. A bind declaration defaults to
read-only. A managed-volume or tmpfs declaration is itself explicit writable
storage intent; neither is inferred from another field.

## Typed bind mounts

```toml
[[config.filesystem.binds]]
source = "/srv/data"
target = "/data"
readOnly = true
```

Each bind requires an absolute, lexically normalised, colon-free host `source`
and absolute, lexically normalised, colon-free container `target`. `readOnly`
defaults to `true`. `readOnly = false` is explicit dangerous authority and
remains visible in resolved JSON and generated Quadlet source.

Graft fixes every host bind to non-recursive `bind` semantics. A read-only bind
renders the equivalent of `ro,bind`; a writable bind renders `rw,bind`. Host
submounts are therefore excluded rather than accidentally retaining independent
write flags beneath a read-only parent. Recursive bind, propagation, SELinux
relabeling, recursive chown (`U`), idmapped mounts, subpaths, overlay mounts, and
arbitrary options remain unavailable. The shared renderer emits only
Graft-owned mechanical keys, never a user-provided `Mount=` value.

Graft rejects `/` as an exact bind source. It also rejects `/proc`, `/sys`,
`/dev`, and `/run` and descendants of those protected subtrees. This blocks
common whole-host, virtual-kernel, direct-device, and runtime-socket bypasses.
Other sensitive paths, including an entire home directory or `/root`, remain
explicit dangerous intent rather than
being described as safe. Future lint may make them more prominent, but cannot
weaken mandatory resolver errors.

Pure resolution cannot attest source existence, type, ownership, permissions,
filesystem boundaries, or symlink traversal. Nix evaluation and generated
store objects are not a reliable view of arbitrary activation-time host paths.
Graft therefore applies lexical policy only and states this limitation instead
of claiming canonical-path confinement. The host administrator remains
responsible for ensuring the effective source is the reviewed object.

SELinux and AppArmor remain host/runtime policy. Graft does not automatically
relabel host content or disable label separation. A denied mount must fail;
Graft does not relax labels to make it succeed.

## Podman-managed volumes

```toml
[[config.filesystem.volumes]]
name = "database"
target = "/var/lib/database"

[[config.filesystem.volumes]]
target = "/scratch"
```

`name` is optional. When present, it is 1–128 ASCII characters matching
`^[A-Za-z0-9][A-Za-z0-9_.-]*$` and must not end in `.volume`; this excludes
paths, delimiters, whitespace, control characters, and Quadlet resource-unit
interpretation. Its absence explicitly requests an anonymous volume. `target`
is required, absolute, lexically normalised, and colon-free. A named volume
persists under Podman's volume lifecycle; an anonymous volume follows generated
container cleanup behavior and is not a persistence guarantee.

A literal name is an explicit dangerous resource reference. Podman reuses an
existing volume of that name, so Graft cannot claim ownership, provenance, or
empty initial state. Reusing the same name in multiple workloads under one
Podman storage scope intentionally shares state; the resolver must make that
relationship visible in set diagnostics. Scope follows the effective Podman
account and storage configuration, not the Graft manager target alone. A
non-root user normally has separate rootless storage, while a root-owned user
manager can share rootful Podman storage and named volumes with the system
manager. Custom Podman storage configuration can further change that boundary;
Graft does not attest it.

Managed volumes are writable by default because their declaration explicitly
requests writable runtime-managed storage. An optional `readOnly = true` may
narrow access. Graft does not accept a host path in `name` and does not interpret
`.volume` as a Quadlet resource reference in this phase.

Copy-up, ownership mutation, subpaths, volume drivers, external volumes,
idmapped mounts, and automatic `.volume` unit generation remain outside this
contract. Podman may perform its documented first-use ownership and copy-up
behavior for a managed volume; implementation tests must record the effective
behavior of the pinned runtime rather than treating it as a Graft guarantee.

## Typed tmpfs

The current string-list form migrates to typed entries:

```toml
[[config.filesystem.tmpfs]]
target = "/tmp"
mode = "1777"
size = "512M"
```

`target` is required, absolute, lexically normalised, and colon-free. `mode` is
an optional string containing a canonical three- or four-digit octal mode no
greater than `1777`, preventing setuid and setgid bits. `size` is an optional positive integer followed by at
most one approved uppercase size suffix (`K`, `M`, `G`, or `T`). The
implementation must pin the exact translation and verify it against the tested
Podman version.

A tmpfs declaration is explicit writable in-memory storage even when the rootfs
is read-only. Graft preserves Podman's safe default `noexec,nosuid,nodev` flags
and does not expose flags that relax them. Read-only tmpfs, copy-up selection,
UID/GID options, swap policy, access-time flags, ramfs, and raw Linux mount
options remain deferred.

The configured mode controls the mounted tmpfs root. It does not derive from
Nix-store directory metadata and does not promise that every process can write;
effective access still depends on the process identity and runtime policy.
Size limits constrain one mount, not total workload or host memory consumption.

## Direct devices remain unavailable

CDI qualified names remain the only approved dedicated device-reference
contract. This design does not approve direct `/dev` fields, target remapping,
permissions, device directories, or optional-device prefixes.

Pure resolution cannot prove that an activation-time `/dev` path exists or is a
device node rather than a directory or symlink. The runtime may accept broader
objects than Graft intends, while activation-time inspection in the Nix module
would move business logic into a dumb materialiser and would still race later
host changes. Treating runtime rejection as Graft policy would therefore make a
false guarantee.

A future direct-device design requires an explicit host-aware attestation model
that preserves pure resolution and defines time-of-check/time-of-use behavior.
Until then dedicated direct-device intent remains dangerous, deferred, and
fail-closed. Graft never changes host device permissions, groups, labels, kernel
modules, or user namespaces to satisfy a device request.

This is not an effective-device isolation guarantee for host binds. Because
Graft cannot attest source type or resolve activation-time symlinks, an allowed
bind source outside `/dev` may itself be a device node or resolve to one. Such
exposure remains residual dangerous bind authority and must be reviewed by the
host administrator. Graft does not describe the bind as a device reference or
infer device permissions from it.

## Path and collision rules

All explicit bind, managed-volume, and tmpfs targets enter one collision check.
Before comparison, each path must be absolute, lexically normalised, and
colon-free. Empty components, `.` and `..`, repeated separators, trailing
separators other than `/`, control characters, terminal whitespace, terminal
`\`, and `:` are rejected. Colon rejection prevents a typed path from becoming
an upstream options or component delimiter during mechanical rendering.

The resolver rejects:

- target `/`, which would replace the workload rootfs;
- any target equal to, below, or above `/nix/store`, including `/nix`, preserving
  Graft's fixed read-only store bind without nested mount ambiguity;
- any target equal to or below `/dev`, `/proc`, or `/sys`;
- bind or managed-volume targets equal to or below `/run`, `/tmp`, or
  `/var/tmp`;
- duplicate targets across or within mount kinds; and
- ancestor/descendant overlap between any two explicit targets.

The overlap rule intentionally forbids otherwise valid nested Linux mounts.
Their effective visibility and required ordering are too easy to misread in a
security review. Users must choose non-overlapping targets rather than relying
on declaration order or Podman's conflict resolution.

Podman's automatic mounts for `/dev`, `/dev/shm`, `/run`, `/tmp`, and
`/var/tmp` under the read-only-rootfs policy are runtime-owned baseline mounts.
Only an explicit typed tmpfs may target `/run`, `/tmp`, or `/var/tmp`, or paths
below those trees, to set bounded mode or size intent. No explicit mount kind
may target `/dev`, `/proc`, or `/sys`, or descendants of those paths. The
implementation must test the final generated argv so the tmpfs exception does
not create duplicate ambiguous mounts.

## Target authority

| Context | Effective boundary |
| --- | --- |
| `target = "system"` | Rootful Podman evaluates host paths and devices with host-root runtime authority. Writable binds can modify root-owned data. |
| `target = "user"` under a non-root account | Rootless Podman remains limited by that account, its user namespace, filesystem permissions, labels, and runtime authorization. Same-user files and sockets can still be high authority. |
| `target = "user"` under UID 0 | The user manager and Podman remain root-owned/rootful. This is not a rootless safety boundary. |

Graft does not infer a safer target, change ownership, broaden groups, disable
labels, or add capabilities when a mount or device fails. A typed declaration
expresses requested authority; it does not guarantee the host grants it.

## Rootfs overlay and state

`Rootfs=<store-path>:O` remains Graft-owned. Its automatic upperdir is writable
runtime state unless `readOnly = true` prevents rootfs writes, and it is not a
persistence, backup, promotion, or secure-erasure contract. Graft does not
expose custom rootfs `upperdir` or `workdir` through workload TOML.

Nix normalises store-directory modes, and an empty overlay upperdir cannot
repair missing lowerdir metadata before Podman prepares mounts. Required
mountpoints remain a materialisation responsibility. Writable temporary paths
must use the approved tmpfs contract rather than relying on derivation-time
`chmod`.

Copied workspace semantics belong to [#27]. A future workspace feature may use
the bind contract internally, but it must define source preparation, ownership,
write scope, Git state, cleanup, and promotion separately. It cannot bypass the
same collision and target-authority rules.

## Legacy migration

Existing `config.filesystem.volumes` entries are not silently reinterpreted.
Implementation under [#164] must return field-specific migration diagnostics:

| Legacy shape | Migration |
| --- | --- |
| `source` begins with `/` | Use `config.filesystem.binds`. |
| `source` begins with `.` | Convert it to a reviewed absolute host path and use `config.filesystem.binds`; relative host sources are no longer accepted. |
| `source` is a named-volume value | Move it to `config.filesystem.volumes[].name`. |
| `source` is absent | Keep a source-less typed volume entry to request an anonymous volume. |
| `mode` contains `ro` or `rw` | Replace it with typed `readOnly`; other options remain unavailable. |
| `tmpfs = ["/path"]` | Replace it with `[[config.filesystem.tmpfs]]` and `target = "/path"`. |

The generated schema changes only when parser, resolver, resolved JSON, shared
renderer, diagnostics, and tests land together. Until then, current alpha syntax
remains current and the new forms remain design-only.

## Resolved and rendered contract

Resolved JSON must carry separate `binds`, `volumes`, and `tmpfs` arrays with
concrete effective booleans and validated options. It must not carry raw option
strings. Declaration order is preserved within a kind; rendering uses one
fixed kind order documented by the implementation.
Collision safety means cross-kind order cannot change path visibility.

The shared Nix renderer remains mechanical and identical for NixOS and Home
Manager. It must not inspect host paths, add policy defaults, or recover from
resolver errors. Future `graft lint` and `graft inspect` must derive effective
read/write and host-crossing diagnostics from the same resolved types; warnings
cannot authorize rejected input.

## Required implementation evidence

Implementation under [#164] requires:

- parser, resolver, schema, and migration tests for every field and rejected
  legacy shape;
- table-driven path normalisation, protected-target, duplicate, and nested
  collision tests across every mount-kind pair;
- negative tests for implicit writable host access, forbidden source classes,
  raw options, unsafe tmpfs modes/sizes, and unavailable direct-device forms;
- NixOS and Home Manager parity plus real Quadlet-generator verification;
- runtime evidence that fixed non-recursive read-only binds exclude writable
  host submounts, plus writable binds, named and anonymous volume lifecycle,
  tmpfs mode/size and non-root writes, and missing sources;
- separate rootful and non-root rootless expectations, with root-owned user
  managers documented as rootful; and
- threat-model, capability, reference, non-goal, and migration documentation
  updated atomically with each implemented phase.

Runtime tests must surface authorization failures. They must not change host
permissions, labels, groups, or security defaults merely to make a dangerous
request succeed.

## Deferred follow-ups

This design does not approve:

- raw `Mount=` or `Volume=` passthrough;
- recursive/shared/slave propagation;
- automatic SELinux relabeling or AppArmor relaxation;
- recursive host chown, arbitrary idmaps, or custom rootfs upper/work dirs;
- direct device paths, remapping, permissions, optional devices, or device
  directories;
- ramfs, arbitrary tmpfs flags, copy-up policy, or tmpfs UID/GID controls;
- `.volume` unit generation or external volume-driver configuration;
- source canonicalisation through activation-time host inspection;
- automatic workspace, backup, promote, retention, or secure deletion policy;
  or
- total host-memory, storage-quota, or resource-limit enforcement.

These exclusions remain fail-closed. Concrete future needs require their own
classification and design rather than an option-string escape hatch.

[#27]: https://github.com/Patrick-Kappen/graft/issues/27
[#142]: https://github.com/Patrick-Kappen/graft/issues/142
[#164]: https://github.com/Patrick-Kappen/graft/issues/164
