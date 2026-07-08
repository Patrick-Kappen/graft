# Quadlet — Graft output

Quadlet is a Podman systemd generator. It reads `.container` files and generates
ordinary systemd `.service` units from them.

In Graft, Quadlet is output. Users write TOML; the CLI resolves it to JSON; the
NixOS and Home Manager modules render `.container` files.

```text
TOML → CLI resolved JSON → NixOS/Home Manager module → .container
```

## File locations

| Resolved target | Scope | Path |
|---|---|---|
| `system` | system/rootful | `/etc/containers/systemd/` |
| `user` | user/rootless | `~/.config/containers/systemd/` |

Symlinks are supported. NixOS can build a file in the store and link it through
`environment.etc`. Home Manager can link a file through `xdg.configFile`.

## Responsibilities

### CLI

The CLI translates TOML to resolved JSON:

- package list
- command / `Exec=`
- deploy target
- optional service settings
- Graft defaults and implicit dependencies

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
- Optional `[Service]` keys are rendered only when resolved JSON contains them.
- `[Container]` values that become generated command-line arguments escape `%` specifiers and `$` variables.
- `[Install]` is not rendered by default.

## Rootfs-store mapping

Graft uses a rootfs from the Nix store, not images.

| Quadlet option | Source |
|---|---|
| `ContainerName=` | resolved `name` |
| `Rootfs=<path>:O` | rootfs built from resolved `runtime.packages` |
| `Exec=` | resolved `runtime.command` |
| `Volume=/nix/store:/nix/store:ro` | required for Nix store symlinks |

Example without a user command:

```ini
[Container]
ContainerName=node-dev
Rootfs=/nix/store/xyz-graft-node-dev-env:O
Exec=/bin/graft-pause
Volume=/nix/store:/nix/store:ro
```

Example with a user command:

```ini
[Container]
ContainerName=web
Rootfs=/nix/store/xyz-graft-web-env:O
Exec=node server.js
Volume=/nix/store:/nix/store:ro
```

## `graft-pause`

`graft-pause` is always included in the rootfs:

```text
packages = ["graft-pause", ...user packages]
```

`Exec=/bin/graft-pause` is used only when the user does not set a command. If
the user sets a command, that command becomes `Exec=`.

`graft-pause` exits cleanly on `SIGTERM` and `SIGINT`, so `systemctl stop` and
`systemctl --user stop` can finish without a SIGKILL timeout.

This avoids default dependencies on `bashInteractive`, `coreutils`, or
`sleep infinity`.

## Optional container keys

Graft renders optional Quadlet container keys only when the resolved JSON contains
them:

- `HostName=` from `config.container.hostname`
- `User=` from `config.container.user`
- `Group=` from `config.container.group`
- `WorkingDir=` from `config.container.workingDir`
- `EnvironmentFile=` from `config.container.environmentFile`
- `PublishPort=` from `config.network.publish`
- `Volume=` from `config.filesystem.volumes`

Environment files, published ports, and volumes preserve user order. Environment
variables are sorted by key. Container values render literal `%` as `%%` and
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

## Service settings

Service settings have no Graft defaults.

A `[Service]` section is rendered only when at least one supported service field
is explicitly set. Supported fields currently include `Restart=`, `RestartSec=`,
`TimeoutStartSec=`, and `TimeoutStopSec=`. Service values are copied verbatim
into the generated unit and are not `%`-escaped by Graft.

Example:

```ini
[Service]
Restart=on-failure
RestartSec=10s
TimeoutStartSec=2m
TimeoutStopSec=30s
```

Without explicit service settings, no `[Service]` section is rendered.

## Autostart

A `.container` file may exist without starting automatically.

The current modules do not generate an `[Install]` section. That means systemd
knows the generated service, but does not enable/start it automatically.

If autostart is supported later, it must flow explicitly through TOML and
resolved JSON. It must not be a module default.

## Overlay

Rootfs-store containers use a writable overlay above the read-only store rootfs:

```text
lowerdir = /nix/store/xxx-graft-env   (read-only)
upperdir = container storage          (writable)
```

Writes inside the container go to the upperdir. That upperdir is the future
basis for promote/diff workflows.

System containers (`target = "system"`) use rootful Podman with kernel overlayfs
through `:O`. User containers (`target = "user"`) use rootless Podman and
rootless overlay support such as `fuse-overlayfs`.

## Lifecycle

Generated containers are normal systemd services.

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

Stopping a service removes the runtime container when Podman runs it with
`--rm`. The Quadlet file remains, so the service can be started again later.

## Not used by rootfs-store Graft containers

- `Image=` — Graft uses `Rootfs=` for this mode.
- image downloads at runtime
- user-written `.container` files as input
- default `Restart=on-failure`
- default `[Install] WantedBy=...`
