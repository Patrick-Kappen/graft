# Secure target defaults design

> **Status:** approved design for #139. These defaults and relaxations are not
> current behavior until the remaining #163 implementation lands. The
> [capability status](capabilities.md) remains authoritative for accepted TOML.

Graft will apply one explicit process-hardening baseline to both user and system
targets. The targets differ in host authority, not in whether the workload gets
a weaker baseline. Every default and relaxation must appear in resolved JSON
before the Nix modules render it mechanically.

## Target selection

`deploy.target` becomes required:

```toml
[deploy]
target = "user"
```

or:

```toml
[deploy]
target = "system"
```

The current implicit `system` default will be removed. Rootful system execution
must not result from omission. `user` selects the user manager; it means
rootless Podman only when that manager runs under a non-root host account. A
root-owned user manager remains rootful. Account provisioning and per-container
UID/GID isolation stay outside this phase under #140 and #141.

## Baseline

After #163 implements this design, a minimal workload resolves these concrete
defaults:

```json
{
  "filesystem": {
    "readOnly": true
  },
  "security": {
    "dropCapabilities": [
      "all"
    ],
    "noNewPrivileges": true
  }
}
```

The shared renderer emits:

```ini
ReadOnly=true
DropCapability=all
NoNewPrivileges=true
```

The same baseline applies to long-running services, finite jobs, setup jobs,
local development, and unattended server workloads. Graft does not select an
opaque security profile from workload shape. Temporary agents require the
additional mandatory contract tracked by #153 and #169.

`ReadOnly=true` still permits the tested upstream read-write tmpfs mounts and
any explicit tmpfs, volume, or CDI-injected mount. It makes the root filesystem
read-only; it does not make the complete workload view immutable.

## Explicit relaxations

The implementation will expose only separate typed relaxations:

```toml
[config.filesystem]
readOnly = false

[config.security]
noNewPrivileges = false
addCapabilities = ["CAP_NET_BIND_SERVICE"]
```

| Intent | Classification | Resolved effect |
| --- | --- | --- |
| `readOnly = false` | Dangerous relaxation | `filesystem.readOnly = false`; retain the writable runtime overlay. |
| `noNewPrivileges = false` | Dangerous relaxation | `security.noNewPrivileges = false`. |
| `addCapabilities = [...]` | Dangerous capability grant | Ordered canonical capabilities added after dropping all defaults. |

Relaxations never activate by omission, inference, target choice, lifecycle, or
a future validation level. Empty capability additions, duplicates, `all`,
delimiter/control injection, and non-canonical names fail resolution. Direct
raw Podman or Quadlet arguments remain forbidden.

The implementation must render explicit false values into the Quadlet source
rather than silently omit them. Quadlet 5.8.2 represents
`NoNewPrivileges=false` by omitting Podman's
`--security-opt=no-new-privileges` argument, so generated argv demonstrates the
relaxation through absence rather than an explicit negative argument. Graft
does not claim that this pins behavior against a future host/runtime default.

## Capability migration

The secure baseline makes `dropCapabilities = ["all"]` the effective default.
The existing field therefore has these future semantics:

- omission resolves to `["all"]`;
- explicit `["all"]` remains valid and equivalent;
- a partial drop list becomes a migration error because it would implicitly
  restore the rest of Podman's runtime-default capability set;
- workloads request only required capabilities through `addCapabilities`.

Current partial drop lists remain supported until #163 implements this
migration. The implementation must provide a field-specific diagnostic that
points to `config.security.addCapabilities` without silently changing behavior.

## Preserved upstream and host policy

Not every security-relevant setting has a portable universal value. This design
makes the following ownership decisions:

| Area | Decision after this phase |
| --- | --- |
| User namespaces | Preserve runtime behavior until #140/#141 define account, ownership, store-bind, and rootless/rootful semantics. |
| Seccomp | Preserve Podman's default profile; `unconfined` remains unavailable as dangerous intent. |
| SELinux/AppArmor | Preserve host/runtime defaults; label disable and equivalent relaxations remain unavailable. |
| Mask/unmask | Preserve OCI/Podman defaults; raw paths and `ALL` remain unavailable. |
| Tmpfs | Preserve `ReadOnlyTmpfs=true` behavior and current explicit path-only tmpfs; options and collisions remain #142/#164. |
| Devices | Keep the current qualified CDI contract; no implicit direct devices. |
| Network | Preserve Podman's private connected default; `none` remains explicit and host mode remains dangerous. |
| Resources | Define no universal CPU, memory, PID, or ulimit values before #145. |
| Logging | Preserve runtime/manager logging defaults; no raw log-driver passthrough. |
| Secrets | Keep secrets out of TOML and resolved store text until #143/#166. |
| Init | Do not default `RunInit`; lifecycle and compatibility behavior require separate evidence. |

A preserved upstream default is documented context, not a concrete effective
value invented by Graft. Future inspect diagnostics may report observed runtime
state, but the build-time resolver must not claim values it cannot determine.

## Security invariants

Implementation of this design adds these invariants:

1. Missing `deploy.target` fails before materialisation.
2. Minimal user and system workloads resolve the same concrete hardening
   baseline.
3. System remains rootful; user is described as rootless only for a non-root
   manager account.
4. Omission cannot weaken read-only rootfs, capability, or privilege-gain
   policy.
5. Every supported relaxation is explicit in TOML, resolved JSON, and Quadlet;
   generated Podman arguments must match its documented presence-or-absence
   semantics.
6. Partial legacy capability-drop lists fail with a migration diagnostic.
7. Unimplemented seccomp, label, mask, namespace, resource, logging, secret,
   and init relaxations continue to fail closed.

These extend GRAFT-TM-05 and replace the omission behavior described by
GRAFT-TM-13 only when implementation lands. Until then the current threat-model
wording remains accurate.

## Implementation and test contract

The remaining #163 implementation must land the baseline and its relaxations as
one coherent compatibility change. It must cover:

- minimal explicit user and system targets in resolver tests;
- missing-target migration failure;
- all three concrete defaults in resolved JSON;
- explicit `readOnly = false` and `noNewPrivileges = false` rendering;
- ordered canonical capability additions after `DropCapability=all`;
- rejection of partial legacy drop lists and malformed additions;
- disabled workloads retaining dormant resolved policy without materialisation;
- NixOS and Home Manager source-unit parity;
- real Quadlet generation proving `--read-only`, `--cap-drop all`, and
  `--security-opt=no-new-privileges` defaults;
- real generation proving each explicit relaxation produces the expected
  Podman argument presence or absence;
- controlled rootful and non-root rootless runtime evidence for capabilities,
  no-new-privileges, rootfs writes, and writable-overlay opt-out; and
- schema, reference, capability, threat-model, quickstart, and migration
  updates in the same implementation PR.

The implementation must not combine this contract with user-namespace,
resource-limit, secret, mount-policy, or temporary-agent work.
