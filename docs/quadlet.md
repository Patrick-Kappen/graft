# Generated Quadlet output

Quadlet is Podman's systemd generator. Graft users write TOML; the resolver
emits JSON; Nix renders `.container` source units; Quadlet generates ordinary
systemd services from them.

Current generator checks use Podman/Quadlet 5.8.2 and systemd 260. These are
tested versions, not yet a minimum-version promise; see
[Capability status](capabilities.md#tested-upstream-context).

## Locations and identity

| Target | Manager and authority | Source-unit directory |
| --- | --- | --- |
| `system` | system manager, rootful Podman | `/etc/containers/systemd/` |
| `user` | current account's user manager; rootless only when non-root | `~/.config/containers/systemd/` |

NixOS and Home Manager may place symlinks to immutable store files in those
locations. The TOML filename currently selects the `.container` filename and
resulting service stem; resolved top-level `name` selects `ContainerName=`. Keep
them equal until [#107](https://github.com/Patrick-Kappen/graft/issues/107).

## Minimal rootfs-store output

A minimal workload with the secure defaults and no explicit command renders:

```ini
[Container]
ContainerName=graft-example
Rootfs=/nix/store/...-graft-graft-example-env:O
Exec="/bin/graft-pause"
Volume=/nix/store/...-rootfs/nix/store:/nix/store:ro,bind,nodev,nosuid
Volume=/nix/store/...-member:/nix/store/...-member:ro,bind,nodev,nosuid
ReadOnly=true
DropCapability=all
NoNewPrivileges=true
```

`Exec=` preserves explicit argv with systemd-compatible quoting. An implicit or
long-running workload without argv uses `/bin/graft-pause`; finite `job` and
`setup` workloads must supply a command. Graft does not render `Image=` or pull
an image for this backend.

The first store line mounts the rootfs scaffold read-only; following lines mount
the sorted realised closure members at their exact store paths. The source unit
is derived from `pkgs.closureInfo`, retains those paths as Nix references, and
fails if closure enumeration, target type, member count, or fragment size is
invalid. Typed targets cannot overlap `/nix/store`, and there is no complete-store
fallback.

## Optional container output

The renderer emits an optional key only when resolved JSON contains the
corresponding current concept. The authoritative field-by-field mapping lives in
[Capability status](capabilities.md#current-graft-v1-fields).

Representative output is:

```ini
HostName=example.internal
User=1000
Group=1000
WorkingDir=/workspace
Environment="GREETING=hello world"
EnvironmentFile="/run/graft/example.env"
Tmpfs=/scratch:rw,noexec,nosuid,nodev,mode=1777,size=16M
Volume=/srv/source:/workspace:ro,bind
Volume=graft-data:/data:rw
Volume=/cache
AddDevice=vendor.example/class=device
AddCapability=CAP_NET_BIND_SERVICE
PublishPort=127.0.0.1:8080:8080
```

A network-isolated workload instead renders:

```ini
Network=none
```

Relevant ordering is deterministic:

1. fixed rootfs-store keys, selected container identity, store scaffold, and
   bytewise-sorted closure members;
2. sorted environment variables and ordered environment files;
3. ordered tmpfs, binds, managed volumes, and CDI references;
4. rootfs and process hardening, with additions after drop-all;
5. network namespace and ordered published ports;
6. optional service and install sections.

Typed tmpfs always receives `rw,noexec,nosuid,nodev` plus validated optional mode
and size. Binds always receive explicit `ro,bind` or `rw,bind`. Named managed
volumes receive explicit `ro` or `rw`; anonymous volumes use target-only syntax
and are writable. See [Filesystems and mounts](filesystem-policy.md).

A qualified CDI name becomes one `AddDevice=` line. The host CDI registry owns
any resulting OCI devices, mounts, environment, and hooks; see
[CDI resource references](cdi.md).

`Network=none` and resolved `.container` references are typed namespace output.
Quadlet adds dependencies for source-unit references. Published ports are valid
only with the implicit default network mode.

## Quoting and escaping

Commands, environment values, paths, and other container arguments are quoted
for systemd parsing where required. Values that become generated command-line
arguments escape literal `%` as `%%` and `$` as `$$`. Environment variables are
sorted by key; order-sensitive lists preserve user order.

Environment-file paths are Podman `--env-file` inputs, not systemd service
`EnvironmentFile=` semantics. Relative paths are resolved by Quadlet against the
source-unit directory. They are explicit host references and are not a secrets
transport.

## Unit relationships

Typed dependencies may render a `[Unit]` section:

```ini
[Unit]
Requires=database.container
After=database.container
PartOf=database.container
```

Workload targets use Quadlet source-unit identities so the generator can map
them to services and add resource dependencies. Exact validated external unit
names remain unchanged. Raw `[Unit]` maps are unavailable; see
[Workload dependencies](dependencies.md).

## Service and startup sections

Lifecycle and timing intent may render:

```ini
[Service]
Type=oneshot
RemainAfterExit=yes
TimeoutStartSec=2m
TimeoutStopSec=30s
```

No restart or timing policy is added by default. The exact lifecycle mapping is
in [Workload lifecycle](lifecycle.md).

No `[Install]` section is rendered by default. Explicit
`deploy.activation = "startup"` resolves to `multi-user.target` for system
workloads or `default.target` for user workloads:

```ini
[Install]
WantedBy=multi-user.target
```

Graft never invokes `systemctl enable` during materialisation. See
[Startup activation](activation.md).

## Overlay lifetime

`Rootfs=...:O` uses runtime-managed overlay storage. The secure baseline makes
rootfs paths read-only, while typed mounts or trusted CDI edits may remain
writable. Graft configures no persistent inspectable upperdir, so container
writes must not be treated as durable state or a current promote workflow.
Stopping the generated service removes the runtime container; the source unit
remains available for a later start.
