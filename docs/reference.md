# Reference

This page documents only the current Graft configuration contract. Use the
[NixOS system-container quickstart](quickstart/nixos.md) or
[Home Manager user-container quickstart](quickstart/home-manager.md) for a
complete runnable workload.

Related authoritative sources:

- [Graft v1 JSON Schema](https://github.com/Patrick-Kappen/graft/blob/main/crates/graft/schema/graft-v1.schema.json) — accepted current TOML shape;
- [Capability status](capabilities.md) — each field's parser, resolver, Nix, and Quadlet stages plus deferred and forbidden boundaries;
- [Roadmap](roadmap.md) — planned implementation direction;
- [Non-goals and deferred scope](non-goals.md) — deliberate current exclusions.

Taplo applies the generated schema to runnable quickstarts and Nix fixtures
through `.taplo.toml`. Unknown fields, wrong basic types, unsupported enum
values, and missing required fields therefore fail before Graft runs. The Rust
resolver performs the remaining semantic and cross-field validation.

## Fail-closed input contract

A parser-recognised field is not supported merely because it can deserialize.
Normal resolution rejects every explicitly configured reserved leaf with its
exact TOML path, including `false`, zero, an empty list, or an empty map. Empty
parent tables remain valid when none of their fields are set.

`validation.level` is reserved and cannot downgrade this behavior. The generated
schema excludes all reserved fields, so schema-valid input represents current
supported intent. See [Capability status](capabilities.md#reserved-parser-fields)
for the complete status boundary.

## Top-level fields

| Field | Type | Default | Contract |
| --- | --- | --- | --- |
| `version` | integer | required | Must be exactly `1`. |
| `name` | string | required | Must match `^[A-Za-z0-9][A-Za-z0-9._-]*$`. |
| `dependencies` | list of tables | optional | Typed activation, ordering, and lifecycle relationships. |
| `deploy` | table | optional | Materialisation target, enable state, and startup intent. |
| `config` | table | optional | Runtime, container, filesystem, network, and service intent. |

The generated `.container` filename and systemd service stem currently come
from the TOML filename, while `ContainerName=` comes from `name`. Keep the file
stem and `name` equal until
[#107](https://github.com/Patrick-Kappen/graft/issues/107) defines the final
identity contract.

## Deployment

```toml
[deploy]
enable = true
target = "system"
activation = "startup"
```

| Field | Accepted values | Default | Effect |
| --- | --- | --- | --- |
| `deploy.enable` | `true`, `false` | materialise | `false` prevents both NixOS and Home Manager from rendering the workload. |
| `deploy.target` | `system`, `user` | `system` | Selects the NixOS system manager or current Home Manager account's user manager. The user target is rootless only for a non-root account. |
| `deploy.activation` | `startup` | absent | Requests the workload from a fixed target during manager startup. |

Startup maps system workloads to `WantedBy=multi-user.target` and user workloads
to `WantedBy=default.target`. Absence renders no `[Install]` relationship.
Disabled workloads may retain dormant startup intent but are not materialised.
See [Workload startup activation](activation.md) for manager, linger, lifecycle,
and rebuild boundaries.

## Dependencies

```toml
[[dependencies]]
target = { workload = "database" }
requirement = "required"
ordering = "after"

[[dependencies]]
target = { externalUnit = "postgresql.service" }
lifecycle = "part-of"
```

Each dependency has exactly one typed target and at least one relationship axis:

| Field | Accepted values | Effect |
| --- | --- | --- |
| `dependencies[].target.workload` | safe Graft workload name | Resolves through explicit same-target context to a Quadlet `.container` source unit. |
| `dependencies[].target.externalUnit` | concrete validated systemd unit name | Refers explicitly to an existing unit in the selected system or user manager. |
| `dependencies[].requirement` | `required`, `optional` | Renders `Requires=` or `Wants=`. |
| `dependencies[].ordering` | `after`, `before` | Renders `After=` or `Before=`. |
| `dependencies[].lifecycle` | `part-of`, `bound` | Renders `PartOf=` or `BindsTo=`. |

Workload targets must exist, be enabled, use the same deploy target, and not
create self-references, duplicates, ambiguous identities, or cycles. External
unit names are line-safe concrete names with a supported systemd suffix; pure
resolution cannot verify their presence in the selected manager. `bound` cannot
be combined with `requirement` and cannot target a Graft `job`. Relationship
lists are sorted in resolved JSON. An empty dependency list is omitted.

See [Typed workload dependencies](dependencies.md) for exact systemd semantics,
external-unit trust boundaries, lifecycle combinations, automatic Quadlet
resource dependencies, and generator translation.

## Runtime

```toml
[config.runtime]
mode = "rootfs-store"
packages = ["bash"]
command = ["bash", "-c", "exec /bin/graft-pause"]
```

| Field | Type | Default | Validation and effect |
| --- | --- | --- | --- |
| `config.runtime.mode` | string | `rootfs-store` | Only `rootfs-store` is supported. |
| `config.runtime.packages` | list of strings | `[]` | Entries must be non-empty and contain no whitespace or control characters. Names resolve from the target configuration's trusted `pkgs`. |
| `config.runtime.command` | non-empty list of strings | `[/bin/graft-pause]` | Entries must be non-empty and contain no control characters. The argv is preserved and rendered as quoted `Exec=`. |

`graft-pause` is always added to the resolved package list. No default shell,
`coreutils`, restart policy, or startup activation is added. Custom package names
require an explicitly trusted host overlay or package-set extension; TOML never
evaluates arbitrary repository Nix.

## Container

```toml
[config.container]
hostname = "worker.local"
user = "1000"
group = "1000"
workingDir = "/workspace"
environmentFile = ["/run/graft/worker.env"]

[config.container.environment]
LOG_LEVEL = "debug"
GREETING = "hello world"
```

| Field | Type | Validation | Quadlet output |
| --- | --- | --- | --- |
| `config.container.hostname` | string | Non-empty; no control characters; no DNS/FQDN validation. | `HostName=` |
| `config.container.user` | string | Non-empty; no control characters; no UID syntax validation. | `User=` |
| `config.container.group` | string | Requires `user`; non-empty; no control characters; no GID syntax validation. | `Group=` |
| `config.container.workingDir` | string | Non-empty; no control characters; no existence or creation check. | `WorkingDir=` |
| `config.container.environment` | string map | Keys are non-empty, contain no control characters, whitespace, or `=`; values contain no control characters. | Sorted, quoted `Environment="KEY=value"` lines. |
| `config.container.environmentFile` | list of strings | Ordered entries are non-empty and contain no control characters; files are not generated or checked. | Ordered, quoted `EnvironmentFile=` lines. |

`workingDir` sets only the process directory inside the container. It does not
copy a workspace or create a host mount. Environment values are not a secret
transport, and Graft does not generate environment files or import the host
environment.

## Filesystem volumes

```toml
[[config.filesystem.volumes]]
target = "/cache"

[[config.filesystem.volumes]]
source = "/srv/data"
target = "/data"
mode = "ro"
```

`config.filesystem.volumes` preserves user order and renders each entry
mechanically as `target`, `source:target`, or `source:target:mode`.

| Field | Required | Validation |
| --- | --- | --- |
| `target` | yes | Non-empty; no control characters or `:`. |
| `source` | no | Non-empty when present; no control characters or `:`. No path existence check. |
| `mode` | no | Requires `source`; non-empty; no control characters or `:`. No option allowlist. |

A mode without `ro` may create a writable host mount. The current contract does
not attest path safety or confinement; review such mounts explicitly. Typed
mount policy remains tracked by
[#142](https://github.com/Patrick-Kappen/graft/issues/142) and
[#164](https://github.com/Patrick-Kappen/graft/issues/164).

## Network

### Implicit default and published ports

```toml
[config.network]
publish = ["127.0.0.1:8080:8080"]
```

When `mode` is absent, Graft renders no `Network=` and preserves Podman's
target-specific default. Published-port entries are ordered literal
`PublishPort=` values; each must be non-empty and contain no control characters.
Graft currently performs no port-syntax validation and manages no firewall.

### No external IP network

```toml
[config.network]
mode = "none"
```

This renders `Network=none`. It leaves loopback available and does not claim
isolation from communication through mounted sockets or devices.

### Shared workload namespace

```toml
[config.network]
mode = "container"
container = "database"
```

`container` names another Graft workload, not a Podman runtime identity. The
resolver requires that workload to exist, be enabled, use the same target, and
have the effective `long-running` lifecycle. It rejects self-references,
duplicates, missing references, target mismatches, and cycles.

The resolved source-unit reference renders as `Network=<source>.container`,
allowing Quadlet to generate the runtime identity and service dependencies.
`publish` is incompatible with both explicit modes. See
[Container network intent](networking.md) for namespace and exposure details.

## Service

```toml
[config.service]
lifecycle = "long-running"
restart = "on-failure"
restartSec = "10s"
timeoutStartSec = "2m"
timeoutStopSec = "30s"
```

| Field | Accepted values | Default | Validation and effect |
| --- | --- | --- | --- |
| `config.service.lifecycle` | `long-running`, `job`, `setup` | effective `long-running` | Maps typed intent to `Type=` and finite `RemainAfterExit=`. `job` and `setup` require an explicit command. |
| `config.service.restart` | `no`, `on-success`, `on-failure`, `on-abnormal`, `on-watchdog`, `on-abort`, `always` | absent | Rendered only when set. Finite lifecycles reject `always`, `on-success`, and currently `on-watchdog`. |
| `config.service.restartSec` | string | absent | Non-empty, no control characters, and requires restart other than `no`. Rendered verbatim. |
| `config.service.timeoutStartSec` | string | absent | Non-empty and no control characters. Rendered verbatim. |
| `config.service.timeoutStopSec` | string | absent | Non-empty and no control characters. Rendered verbatim. |

No timespan parser is applied to service timing values. See
[Workload lifecycle semantics](lifecycle.md) for state transitions, finite jobs,
restart restrictions, and generator-owned cleanup.

## Renderer escaping

The renderer preserves literal TOML semantics while producing systemd-safe
Quadlet values. Command argv and environment-file paths are quoted; literal
quotes, backslashes, `%` specifiers, and `$` substitutions are escaped according
to their generated command-line context. Environment maps are sorted; ordered
lists retain source order. `[Service]` timing values are copied verbatim because
Quadlet places them directly in the generated service section.

See [Quadlet output](quadlet.md) for the complete output contract and examples.

## NixOS module

```nix
{ inputs, ... }:
{
  imports = [ inputs.graft.nixosModules.graft ];

  services.graft = {
    enable = true;
    configRoot = ./containers;
    configRoots = [
      ./containers/common
      ./hosts/my-host/containers
    ];
  };
}
```

| Option | Type | Default | Description |
| --- | --- | --- | --- |
| `services.graft.enable` | bool | `false` | Enable system/rootful Graft materialisation. |
| `services.graft.package` | package or null | `null` | Package providing `graft` and `graft-pause`; the exported flake module supplies a default. |
| `services.graft.configRoot` | path or null | `null` | First directory containing `*.toml` workloads. |
| `services.graft.configRoots` | list of paths | `[]` | Additional workload directories, read in list order. |

The module renders only effective `target = "system"` workloads under
`/etc/containers/systemd/`.

## Home Manager module

```nix
{ inputs, ... }:
{
  imports = [ inputs.graft.homeManagerModules.graft ];

  programs.graft = {
    enable = true;
    configRoot = ./containers;
    configRoots = [
      ./containers/common
      ./hosts/my-host/containers
    ];
  };
}
```

| Option | Type | Default | Description |
| --- | --- | --- | --- |
| `programs.graft.enable` | bool | `false` | Enable Graft materialisation in the current Home Manager account's user manager. |
| `programs.graft.package` | package or null | `null` | Package providing `graft` and `graft-pause`; the exported flake module supplies a default. |
| `programs.graft.configRoot` | path or null | `null` | First directory containing `*.toml` workloads. |
| `programs.graft.configRoots` | list of paths | `[]` | Additional workload directories, read in list order. |

The module renders only effective `target = "user"` workloads under
`~/.config/containers/systemd/`. Podman is rootless only when Home Manager runs
for a non-root account; the module does not reject UID 0.

Both modules read `configRoot` first and then `configRoots` in order. Every
configured root must exist. New files must be tracked before Git flakes can see
them. Duplicate TOML filenames across roots and duplicate effective workload
names within one target fail materialisation. Importing the underlying module
files directly requires an explicit package; the exported flake modules provide
it with `mkDefault`.
