# Quadlet — Graft output

Quadlet is a Podman systemd generator. It reads `.container` files and generates
ordinary systemd `.service` units from them. Current generator checks use Podman
5.8.2 and systemd 260; see the versioned links in
[Capability status](capabilities.md#tested-upstream-context).

In Graft, Quadlet is output. Users write TOML; the CLI resolves it to JSON; the
NixOS and Home Manager modules render `.container` files.

```text
TOML → CLI resolved JSON → NixOS/Home Manager module → .container
```

## File locations

| Resolved target | Scope | Path |
| --- | --- | --- |
| `system` | system manager, rootful | `/etc/containers/systemd/` |
| `user` | current account's user manager; rootless only when non-root | `~/.config/containers/systemd/` |

Symlinks are supported. NixOS can build a file in the store and link it through
`environment.etc`. Home Manager can link a file through `xdg.configFile`.

Today, the `.container` filename and resulting systemd service stem come from
the TOML filename. `ContainerName=` comes from resolved `name`. Keep the
filename stem and `name` equal until
[#107](https://github.com/Patrick-Kappen/graft/issues/107) defines the final
identity contract.

## Responsibilities

### CLI

The CLI translates TOML to resolved JSON:

- package list
- command / `Exec=`
- deploy target
- optional service settings
- typed concrete dependency identities
- Graft defaults and automatic resource references

The CLI does not render `.container` files.

### Nix modules

The NixOS and Home Manager modules render Quadlet mechanically from resolved
JSON:

- NixOS renders only `target = "system"`.
- Home Manager renders only `target = "user"`.
- `ContainerName=` comes from resolved `name`.
- `Rootfs=` comes from the generated rootfs store path.
- `Exec=` comes from resolved `runtime.command`.
- `Volume=/nix/store:/nix/store:ro` is always rendered for store symlinks.
- Optional `[Unit]` dependency keys are rendered only from concrete resolved identities.
- Optional `[Service]` keys are rendered only when resolved JSON contains them.
- `[Container]` values that become generated command-line arguments escape `%` specifiers and `$` variables.
- `[Install]` is not rendered by default.

## Rootfs-store mapping

The current `rootfs-store` mode uses a rootfs from the Nix store, not images.
Non-rootfs artifact scope remains undecided in
[#150](https://github.com/Patrick-Kappen/graft/issues/150); no future syntax is
promised.

| Quadlet option | Source |
| --- | --- |
| `ContainerName=` | resolved `name` |
| `Rootfs=<path>:O` | rootfs built from resolved `runtime.packages` |
| `Exec=` | resolved `runtime.command` |
| `Volume=/nix/store:/nix/store:ro` | required for Nix store symlinks |

Example without a user command:

```ini
[Container]
ContainerName=node-dev
Rootfs=/nix/store/xyz-graft-node-dev-env:O
Exec="/bin/graft-pause"
Volume=/nix/store:/nix/store:ro
```

Example with a user command:

```ini
[Container]
ContainerName=web
Rootfs=/nix/store/xyz-graft-web-env:O
Exec="node" "server.js"
Volume=/nix/store:/nix/store:ro
```

## `graft-pause`

`graft-pause` is always included in the rootfs:

```text
packages = ["graft-pause", ...user packages]
```

`Exec="/bin/graft-pause"` is used only when the user does not set a command. If
the user sets a command, that command becomes quoted `Exec=` argv.

`graft-pause` exits cleanly on `SIGTERM` and `SIGINT`, so `systemctl stop` and
`systemctl --user stop` can finish without a SIGKILL timeout.

This avoids default dependencies on `bashInteractive`, `coreutils`, or
`sleep infinity`.

## Optional container keys

Graft renders optional Quadlet keys only when the resolved JSON contains them.
The authoritative TOML → resolved JSON → Nix → Quadlet mapping is the
[current-field capability matrix](capabilities.md#current-graft-v1-fields).
This page describes output-specific behavior rather than maintaining a second
field reference.

Typed `Network=` output supports `none` and resolved `.container` source-unit
references. The source-unit form lets Quadlet add automatic `Requires=` and
`After=` relationships; see [Container network intent](networking.md).

Environment files, published ports, and volumes preserve user order. Environment
variables are sorted by key. Environment files and command argv are quoted for
systemd argument parsing. Container values render literal `%` as `%%` and
literal `$` as `$$` when they become generated command-line arguments.

## Environment variables

Environment variables are rendered as sorted, quoted systemd assignments:

```ini
Environment="BRACED=pre$${HOME}post"
Environment="DOLLAR=cost $$5"
Environment="GREETING=hello world"
Environment="PERCENT=100%%"
Environment="QUOTED=say \"hi\""
```

The whole `KEY=value` assignment is quoted. Double quotes, backslashes, `%`
specifier markers, and literal `$` characters are escaped before rendering.
Environment values are not a secret transport.

## Unit dependencies

A `[Unit]` section is rendered only when resolved typed dependencies contain at
least one relationship. The resolver sorts concrete identity lists; Nix joins
them mechanically without accepting free-form keys:

```ini
[Unit]
Requires=database.container
After=database.container
PartOf=database.container
```

For Graft workload targets, Quadlet translates `.container` source-unit names
to generated `.service` identities and fails generation if the source unit is
missing. Exact `externalUnit` identities remain unchanged. Resource-specific
references such as `Network=database.container` continue to let Quadlet add
their automatic dependencies instead of duplicating lines here.

See [Typed workload dependencies](dependencies.md) for input, validation,
relation semantics, and external-unit boundaries. Raw `[Unit]` maps remain
unsupported.

## Service settings

Service settings have no restart or timing defaults. An absent lifecycle leaves
Quadlet's normal long-running notify behavior implicit.

A `[Service]` section is rendered only when at least one supported service field
is explicitly set. `config.service.lifecycle` maps typed workload intent to
`Type=` and, for finite workloads, `RemainAfterExit=`. Supported fields also
include `Restart=`, `RestartSec=`, `TimeoutStartSec=`, and `TimeoutStopSec=`.
Literal timing values are copied verbatim and are not `%`-escaped by Graft.

Example:

```ini
[Service]
Restart=on-failure
RestartSec=10s
TimeoutStartSec=2m
TimeoutStopSec=30s
```

Without explicit service settings, no `[Service]` section is rendered.

## Startup activation

A `.container` file may exist without starting automatically. When
`deploy.activation` is absent, the modules generate no `[Install]` section, so
systemd knows the service without requesting it during manager startup.

Explicit `deploy.activation = "startup"` maps to a fixed system or user target;
see [Workload startup activation](activation.md). The resolver selects the target
and the modules render the resolved `[Install]` relationship mechanically. Graft
never invokes `systemctl enable` during build or materialisation.

## Overlay

Rootfs-store containers use a writable overlay above the read-only store rootfs:

```text
lowerdir = /nix/store/xxx-graft-env   (read-only)
upperdir = container storage          (writable)
```

Writes inside the container go to the upperdir. The current `Rootfs=...:O` mode
does not configure a persistent, inspectable upperdir, so users must not rely on
those writes after the runtime container is removed. It is not a promote flow.
Reviewable overlay inspection, diff, and promotion are future work in
[#160](https://github.com/Patrick-Kappen/graft/issues/160) and
[#175](https://github.com/Patrick-Kappen/graft/issues/175).

System containers (`target = "system"`) use rootful Podman with kernel overlayfs
through `:O`. User-target containers run through the current Home Manager
account's user manager. Podman and rootless overlay support such as
`fuse-overlayfs` apply only when that account is non-root; a root-owned user
manager remains rootful.

## Lifecycle

Generated containers are normal systemd services. The typed distinction between
long-running services, repeatable finite jobs, and retained setup jobs is defined
in [Workload lifecycle semantics](lifecycle.md).

System container:

```bash
sudo systemctl start <name>.service
sudo systemctl stop <name>.service
```

User container:

```bash
systemctl --user start <name>.service
systemctl --user stop <name>.service
```

A user timer may also trigger a generated user service. If that service must run
without an active login session, enable systemd user linger in host
configuration or with `loginctl enable-linger <user>`; Graft TOML does not carry
host login policy.

Stopping a service removes the runtime container when Podman runs it with
`--rm`. The Quadlet file remains, so the service can be started again later.

## Not used by rootfs-store Graft containers

- `Image=` — Graft uses `Rootfs=` for this mode.
- image downloads at runtime
- user-written `.container` files as input
- default `Restart=on-failure`
- default `[Install] WantedBy=...`
