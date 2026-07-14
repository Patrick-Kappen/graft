# Container hardening

Graft applies the same concrete process-hardening baseline to every explicit
system or user target:

```json
{
  "filesystem": { "readOnly": true },
  "security": {
    "dropCapabilities": ["all"],
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

`deploy.target` is required. `system` uses rootful Podman; `user` is rootless
only when its user manager belongs to a non-root account. A root-owned user
manager remains rootful.

## Explicit relaxations

Workloads must state each weaker choice directly:

```toml
[config.filesystem]
readOnly = false

[config.security]
noNewPrivileges = false
addCapabilities = ["CAP_NET_BIND_SERVICE"]
```

| TOML field | Accepted value | Effect |
| --- | --- | --- |
| `config.filesystem.readOnly` | Boolean; defaults to `true` | `false` retains the writable runtime overlay. |
| `config.security.dropCapabilities` | Omitted or exactly `["all"]` | Drops the runtime-default capability set. |
| `config.security.addCapabilities` | Non-empty ordered unique canonical `CAP_*` names | Restores only named capabilities after drop-all. |
| `config.security.noNewPrivileges` | Boolean; defaults to `true` | `false` explicitly opts out of no-new-privileges. |

Partial legacy drop lists fail with a migration diagnostic directing the user
to `addCapabilities`. Empty additions, duplicates, `all`, non-canonical names,
and control characters fail resolution. The resolver preserves addition order.
Quadlet 5.8.2 normalises generated Podman capability arguments to lowercase.

`NoNewPrivileges=false` remains visible in resolved JSON and Quadlet source.
Quadlet represents it in generated Podman argv by omitting
`--security-opt=no-new-privileges`; Graft does not claim that this pins behavior
against a future runtime default.

## Boundaries

`ReadOnly=true` constrains the root filesystem, not every visible mount. Graft
provides `/tmp` and `/var/tmp` mountpoints so Podman's tested read-only-rootfs
tmpfs setup can initialise under rootless overlay. Nix normalises store
directory modes to read-only metadata, so Graft does not promise that those
tmpfs paths are process-writable for a non-root container user. Explicit tmpfs,
volumes, and CDI-injected mounts remain separate writable boundaries. A current
source-backed volume without `mode = "ro"` may still select the writable
upstream default. The
approved [filesystem policy](filesystem-policy.md) replaces that legacy
exception; implementation and migration remain in [#164].

Rootless capability additions apply inside the container user namespace and do
not grant capability in the host's initial user namespace. The runtime may
reject a request that the selected account or namespace mapping cannot grant;
Graft propagates that failure without changing the requested set.

Capabilities do not remove authority conveyed by host paths, shared namespaces,
CDI resources, external systemd dependencies, the selected host account, or the
host kernel. No-new-privileges is not a replacement for a non-root container
user, namespace isolation, seccomp, labels, resource limits, or a VM.

Privileged mode, unconfined seccomp, label relaxations, mask/unmask controls,
raw security options, and user-namespace policy remain unavailable and fail
closed. See the [Secure target defaults](secure-defaults.md),
[Capability policy](capability-policy.md), and [Threat model](threat-model.md).

[#164]: https://github.com/Patrick-Kappen/graft/issues/164
