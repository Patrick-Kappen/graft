# Reference

The annotated TOML schema lives in
[`examples/reference.toml`](https://github.com/Patrick-Kappen/graft/blob/main/examples/reference.toml).

This page summarises the currently implemented module options and resolver
behaviour. The TOML schema is broader than the MVP renderer; many fields are
parse-only today and do not yet affect Quadlet output.

## NixOS module

```nix
{
  imports = [ inputs.graft.nixosModules.graft ];

  services.graft = {
    enable = true;
    configRoot = ./containers;
  };
}
```

| Option | Type | Default | Description |
|---|---|---|---|
| `services.graft.enable` | bool | `false` | Enable system/rootful Graft containers. |
| `services.graft.package` | package or null | flake package | Package providing `graft` and `graft-pause`. |
| `services.graft.configRoot` | path or null | `null` | Directory containing `*.toml` container definitions. |

The NixOS module renders only resolved containers with `target = "system"` and
places files under `/etc/containers/systemd/`.

## Home Manager module

```nix
{
  imports = [ inputs.graft.homeManagerModules.graft ];

  programs.graft = {
    enable = true;
    configRoot = ./containers;
  };
}
```

| Option | Type | Default | Description |
|---|---|---|---|
| `programs.graft.enable` | bool | `false` | Enable user/rootless Graft containers. |
| `programs.graft.package` | package or null | flake package | Package providing `graft` and `graft-pause`. |
| `programs.graft.configRoot` | path or null | `null` | Directory containing `*.toml` container definitions. |

The Home Manager module renders only resolved containers with `target = "user"`
and places files under `~/.config/containers/systemd/`.

## Current TOML behaviour

Implemented today:

- `version = 1` is required.
- `name` is required and must be a safe container name.
- `deploy.target` defaults to `system`.
- `deploy.enable = false` prevents rendering.
- `config.container.hostname` is rendered as Quadlet `HostName=` when explicitly set.
- `config.container.user` is rendered as Quadlet `User=` when explicitly set.
- `config.container.group` is rendered as Quadlet `Group=` when explicitly set together with `config.container.user`.
- `config.container.workingDir` is rendered as Quadlet `WorkingDir=` when explicitly set.
- `config.container.environment` is rendered as sorted, quoted Quadlet `Environment="KEY=value"` lines when explicitly set.
- `config.container.environmentFile` is rendered as ordered Quadlet `EnvironmentFile=` lines when explicitly set.
- `config.filesystem.volumes` is rendered as ordered Quadlet `Volume=` lines when explicitly set.
- `config.network.publish` is rendered as ordered Quadlet `PublishPort=` lines when explicitly set.
- `config.runtime.mode` supports only `rootfs-store`.
- `config.runtime.packages` are mapped to Nix packages.
- `graft-pause` is always added to the package list.
- missing `config.runtime.command` becomes `['/bin/graft-pause']`.
- explicit `config.runtime.command` is preserved.
- `config.service.restart` is rendered only when explicitly set.
- `config.service.restartSec`, `timeoutStartSec`, and `timeoutStopSec` are rendered only when explicitly set.

### Renderer escaping

Rendered `[Container]` values use systemd-safe escaping while preserving
literal TOML semantics. Literal `%` characters are written as `%%` so systemd
does not treat them as specifiers after Quadlet places them in generated service
command lines. Values that become generated command-line arguments also write
literal `$` as `$$` so systemd does not perform environment variable
substitution. Quoted `Environment="KEY=value"` lines additionally escape double
quotes and backslashes for systemd syntax. `[Service]` values are rendered
verbatim because Quadlet copies them into the generated unit service section.

### Container field validation

`config.container.hostname` is treated as a literal value for Quadlet
`HostName=`.

Current hostname validation:

- must not be empty or whitespace-only
- must not contain control characters
- no template expansion is performed
- no DNS/FQDN validation is performed yet

`config.container.user` is treated as a literal value for Quadlet `User=`.

Current user validation:

- must not be empty or whitespace-only
- must not contain control characters
- no UID syntax validation is performed yet

`config.container.group` is treated as a literal value for Quadlet `Group=`.
It requires `config.container.user` because Quadlet rejects `Group=` without
`User=`.

Current group validation:

- requires `config.container.user`
- must not be empty or whitespace-only
- must not contain control characters
- no GID syntax validation is performed yet
- `GroupAdd=`, supplemental groups, UID/GID maps, user namespaces, and security hardening defaults are not rendered

`config.container.workingDir` is treated as a literal value for Quadlet
`WorkingDir=`.

Current working directory validation:

- must not be empty or whitespace-only
- must not contain control characters
- no path existence validation is performed
- no automatic directory creation is performed
- no workspace copy or host disk mount is created

Future copied workspace support belongs under `config.workspace`; `workingDir`
only sets the process working directory inside the container.

`config.container.environment` is rendered as quoted Quadlet `Environment=`
assignments. Output is sorted by key for deterministic builds. The whole
`KEY=value` assignment is double-quoted so values may contain spaces or `=`.
Double quotes, backslashes, `%` specifier markers, and literal `$` characters
are escaped for systemd syntax and command-line substitution.

Current environment validation:

- keys must not be empty or whitespace-only
- keys must not contain control characters
- keys must not contain whitespace or `=`
- values may be empty
- values may contain whitespace or `=`
- values must not contain control characters
- no secret handling is performed
- no environment file generation or host environment passthrough is performed

`config.container.environmentFile` is treated as literal Quadlet
`EnvironmentFile=` entries. User order is preserved.

Current environment file validation:

- entries must not be empty or whitespace-only
- entries must not contain control characters
- no env file generation is performed
- no secrets materialisation is performed
- no host environment passthrough is performed

`config.filesystem.volumes` is treated as literal Quadlet `Volume=` entries.
User order is preserved. Entries are rendered mechanically as `target`,
`source:target`, or `source:target:mode`.

Current filesystem volume validation:

- `target` is required by the TOML schema
- `target` must not be empty or whitespace-only
- `target` must not contain control characters
- `target` must not contain `:` because Graft assembles colon-separated `Volume=` text
- optional `source`, when present, must not be empty or whitespace-only
- optional `source`, when present, must not contain control characters
- optional `source`, when present, must not contain `:` because Graft assembles colon-separated `Volume=` text
- optional `mode`, when present, requires `source`
- optional `mode`, when present, must not be empty or whitespace-only
- optional `mode`, when present, must not contain control characters
- optional `mode`, when present, must not contain `:` because Graft assembles colon-separated `Volume=` text
- no path existence validation is performed
- no mode allowlist is applied yet
- no Quadlet `.volume` units are generated
- no tmpfs, device, raw mount, workspace, home, or promote semantics are rendered

`config.network.publish` is treated as literal Quadlet `PublishPort=` entries.
User order is preserved.

Current published port validation:

- entries must not be empty or whitespace-only
- entries must not contain control characters
- no port syntax validation is performed yet
- no Quadlet `.network` units are generated
- no DNS settings or network aliases are rendered
- no automatic firewall rules are managed

`config.service.restartSec`, `timeoutStartSec`, and `timeoutStopSec` are treated
as literal systemd service timing values. A `[Service]` section is rendered when
at least one supported service field is set. Service values are rendered
verbatim and are not `%`-escaped by Graft.

Current service timing validation:

- values must not be empty or whitespace-only
- values must not contain control characters
- no systemd timespan parsing is performed yet
- `[Install]`, autostart, service type, `remainAfterExit`, and `restartIfChanged` are not rendered

Not all fields from the annotated TOML reference are rendered yet. Fields that
are parsed but not listed above should be treated as reserved/roadmap fields. See
[Roadmap](roadmap.md) for planned coverage.
