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
- `config.container.workingDir` is rendered as Quadlet `WorkingDir=` when explicitly set.
- `config.container.environment` is rendered as sorted Quadlet `Environment=KEY=value` lines when explicitly set.
- `config.runtime.mode` supports only `rootfs-store`.
- `config.runtime.packages` are mapped to Nix packages.
- `graft-pause` is always added to the package list.
- missing `config.runtime.command` becomes `['/bin/graft-pause']`.
- explicit `config.runtime.command` is preserved.
- `config.service.restart` is rendered only when explicitly set.

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
- no UID/GID syntax validation is performed yet
- `config.container.group` is not rendered yet

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

`config.container.environment` is treated as literal Quadlet `Environment=`
entries. Output is sorted by key for deterministic builds.

Current environment validation:

- keys must not be empty or whitespace-only
- keys must not contain control characters
- keys must not contain whitespace or `=`
- values may be empty
- values must not contain control characters
- values must not contain whitespace until quoted value support is implemented
- no secret handling is performed
- no environment file or host environment passthrough is performed

Not all fields from the annotated TOML reference are rendered yet. Fields that
are parsed but not listed above should be treated as reserved/roadmap fields. See
[Roadmap](roadmap.md) for planned coverage.
