# Explicit container hardening

Graft supports a narrow set of explicit, non-relaxing hardening controls for
both system and user targets:

```toml
[config.filesystem]
readOnly = true

[config.security]
dropCapabilities = ["all"]
noNewPrivileges = true
```

These controls are optional. Graft does not yet apply implicit security
defaults: omitting a field renders no corresponding Quadlet key and preserves
the tested Podman/Quadlet default. Secure target-specific defaults and explicit
relaxation policy remain in
[#139](https://github.com/Patrick-Kappen/graft/issues/139) and the remaining
scope of [#163](https://github.com/Patrick-Kappen/graft/issues/163).

## Supported controls

| TOML field | Accepted value | Effect |
| --- | --- | --- |
| `config.filesystem.readOnly` | `true` only | Makes the container root filesystem read-only. |
| `config.security.dropCapabilities` | Non-empty ordered list containing either `all` alone or canonical `CAP_*` names | Removes capabilities from Podman's default container capability set. |
| `config.security.noNewPrivileges` | `true` only | Prevents container processes from gaining privileges through mechanisms such as set-user-ID binaries and file capabilities. |

`false` is deliberately unavailable for the boolean controls. Until secure
defaults define what a relaxation means, omit the field instead. Capability
names must match `CAP_[A-Z][A-Z0-9_]*`; duplicates and combining `all` with
another entry fail resolution. Graft validates the syntax but does not determine
whether the selected host kernel and runtime recognize a particular capability
name.

The resolver keeps only explicit intent:

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

The shared Nix renderer materialises it mechanically:

```ini
ReadOnly=true
DropCapability=all
NoNewPrivileges=true
```

Quadlet 5.8.2 translates these keys to Podman's `--read-only`, `--cap-drop`, and
`--security-opt=no-new-privileges` arguments. Each capability is rendered as a
separate ordered `DropCapability=` line.

## Boundaries

`readOnly = true` does not mean that every path visible to the workload is
immutable. With the tested upstream default `ReadOnlyTmpfs=true`, Podman mounts
read-write tmpfs filesystems at `/dev`, `/dev/shm`, `/run`, `/tmp`, and
`/var/tmp`. Actual process writes remain subject to path ownership, directory
modes, and the dropped capability set. In particular, a workload must not
assume that `/tmp` is writable merely because the tmpfs mount is read-write;
Nix-store-derived mountpoint modes plus `dropCapabilities = ["all"]` can still
deny a write. Explicit volumes and host-managed CDI specs can also add writable
mounts. `config.filesystem.readOnlyTmpfs` remains unavailable until its
relaxation and compatibility contract is approved.

Dropping capabilities does not remove the authority of mounted host paths,
shared namespaces, CDI-provided resources, external systemd dependencies, the
selected host account, or the host kernel. `noNewPrivileges` constrains
privilege gain inside the process tree; it is not a replacement for a non-root
container user, namespace isolation, seccomp, labels, resource limits, or a VM.

System targets still run through rootful Podman. User targets run through the
current Home Manager account and are rootless only when that account is
non-root. These controls narrow process authority in either context but do not
change the target's trust boundary.

Capability additions, privileged mode, security-label changes, seccomp profile
selection, raw security options, and user-namespace policy remain unavailable
and fail closed. See the [Capability policy](capability-policy.md) and
[Threat model](threat-model.md) for their classifications and residual risks.
