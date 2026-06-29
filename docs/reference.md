# Configuration reference

This is the complete reference for configuring graft: the **Nix module options**
(NixOS and Home Manager) and the **TOML schema** that drives everything.

- For a runnable, fully-annotated TOML that touches every field, see
  [`examples/reference.toml`](../examples/reference.toml). Validate it with
  `graft inspect examples/reference.toml`.
- For a guided introduction, start with [getting-started.md](getting-started.md).

graft's design keeps the Nix side tiny on purpose: **TOML is the source of
truth.** The modules only enable the package and point at your TOML. You can also
author containers directly in Nix (`containers.<name>`); those attrsets are
serialized to TOML and flow through the exact same resolver and renderer — see
[Nix-native authoring](#nix-native-authoring-containers).

---

## NixOS module (`services.graft`)

Complete configuration, including wiring graft as a flake input:

```nix
{
  inputs.graft.url = "github:zerodawn1990/graft";

  # In your NixOS configuration:
  # imports = [ inputs.graft.nixosModules.default ];

  services.graft = {
    enable = true;

    # The graft package used to render TOML -> Quadlet during the build.
    # Defaults to the flake's package for the current system.
    package = inputs.graft.packages.${pkgs.system}.graft;

    # Directory of TOML files, discovered recursively. A file is managed only
    # when it is not no-op and sets `[deploy] enable = true` with
    # `target = "system"` (or no target).
    configRoot = ./containers;

    # Explicit TOML files (alternative/complement to configRoot). Active when
    # not no-op. Prefer configRoot for the directory-discovery workflow.
    configFiles = [
      ./containers/go-dev.toml
    ];

    # Containers authored directly in Nix (no TOML file). The attribute name is
    # the container name. Always active, like configFiles. See "Nix-native
    # authoring" below.
    containers.go-dev.config.runtime = {
      mode = "rootfs-store";
      packages = [ "bashInteractive" "go" ];
      command = [ "bash" "-lc" "go test ./..." ];
    };
  };
}
```

| Option        | Type                | Default                | Description |
| ------------- | ------------------- | ---------------------- | ----------- |
| `enable`      | bool                | `false`                | Enable TOML-driven Podman Quadlet containers. Also enables `virtualisation.podman`. |
| `package`     | package             | flake's `graft`        | graft package used for rendering. |
| `configRoot`  | null or path        | `null`                 | Directory of TOML files, discovered recursively. |
| `configFiles` | list of path        | `[]`                   | Explicit TOML files. |
| `containers`  | attrs of TOML value | `{}`                   | Containers authored directly in Nix. Each attr name is the container name; values mirror the TOML schema. Always active. |

Managed units are rendered during the Nix build to
`/etc/containers/systemd/<name>.container`. The module asserts unique active
names, valid deploy targets, supported runtime modes, and valid/known package
names. See [nixos-module.md](nixos-module.md) for the discovery and graph
behaviour.

---

## Home Manager module (`programs.graft`)

Same option set, rootless/user scope:

```nix
{
  imports = [ inputs.graft.homeManagerModules.default ];

  programs.graft = {
    enable = true;
    package = inputs.graft.packages.${pkgs.system}.graft;  # optional
    configRoot = ./containers;
    configFiles = [ ./containers/dev-shell.toml ];         # optional
    containers.dev-shell.config.runtime = {              # optional, Nix-native
      mode = "rootfs-store";
      packages = [ "bashInteractive" ];
      command = [ "bash" "-l" ];
    };
  };
}
```

| Option        | Type                | Default         | Description |
| ------------- | ------------------- | --------------- | ----------- |
| `enable`      | bool                | `false`         | Enable rootless user Quadlet containers; adds graft to `home.packages`. |
| `package`     | package             | flake's `graft` | graft package used for rendering. |
| `configRoot`  | null or path        | `null`          | Directory of TOML files, discovered recursively. |
| `configFiles` | list of path        | `[]`            | Explicit TOML files. |
| `containers`  | attrs of TOML value | `{}`            | Containers authored directly in Nix. Each attr name is the container name; values mirror the TOML schema. Always active. |

A file is managed only when it is not no-op and sets `[deploy] enable = true`
with `target = "user"`. Units are written to
`~/.config/containers/systemd/<name>.container`. See
[home-manager.md](home-manager.md).

---

## Nix-native authoring (`containers`)

Instead of (or alongside) TOML files, you can declare containers directly as Nix
attrsets via `services.graft.containers.<name>` / `programs.graft.containers.<name>`.
This is convenient when your container config is already computed in Nix (string
interpolation, `lib` helpers, values shared with the rest of your system config).

The attribute name is the container/unit name unless the value sets its own
`name`. The value mirrors the TOML schema exactly — the same top-level keys
(`version`, `name`, `parents`, `children`, `deploy`, `validation`, `config`):

```nix
{
  services.graft = {
    enable = true;
    configRoot = ./containers; # optional: graph refs resolve here

    containers.api = {
      # version defaults to 1; name defaults to the attribute name ("api").
      parents.add = [ "base/runtime" ]; # ref into configRoot
      config = {
        runtime = {
          mode = "rootfs-store";
          packages = [ "bashInteractive" "coreutils" ];
          command = [ "bash" "-lc" "echo hello from nix" ];
        };
        container.environment.FROM_NIX = "1";
        service.type = "oneshot";
      };
    };
  };
}
```

How it works (and why it cannot drift from the TOML path):

1. Each `containers.<name>` attrset is serialized to a TOML file in the Nix store
   with the **same** `pkgs.formats.toml` formatter used to produce the effective
   config.
2. That generated TOML file is fed through the **same** loader, graph resolver,
   `packageOps` step, and `render-nixos-units` renderer as every file-based
   config. There is no second engine and no parallel schema.

Semantics:

- **Always active**, exactly like `configFiles` — declaring it in Nix *is* the
  opt-in, so `[deploy] enable` is not required (a `deploy.target` you set is
  still validated).
- `parents` / `children` refs resolve against `configRoot`, so Nix-authored
  containers can extend file-based base layers. (Referencing another
  `containers.<name>` entry as a parent is not supported; put shared bases in
  `configRoot`.)
- The type is `attrsOf <toml-value>`: anything expressible in TOML is accepted,
  and only TOML-serializable values are allowed (the module fails otherwise).
- All the usual module assertions apply (unique active names, valid deploy
  target, supported runtime mode, valid/known package names).

---

## TOML schema

The full annotated example is [`examples/reference.toml`](../examples/reference.toml).
The sections below summarise every field. Empty means no-op; the loader rejects
unknown fields.

### Top level

| Field        | Type   | Notes |
| ------------ | ------ | ----- |
| `version`    | int    | Must be `1`. |
| `name`       | string | Container/unit name and graph node id. |
| `parents`    | table  | `add` / `remove` / `set` lists of refs (relative to configRoot, no `.toml`). |
| `children`   | table  | `add` / `remove` / `set` lists of refs. |
| `deploy`     | table  | `enable` (bool), `target` (`system` \| `user`). Module opt-in. |
| `validation` | table  | `level` (`off` \| `warn` \| `strict`). Reserved; strict checks are on the roadmap. |

Graph resolution order is `parents -> self -> children`: attrsets merge
recursively, lists concatenate and de-duplicate, scalars and
`config.runtime.command` from later layers win.

### `[config.runtime]`

| Field        | Type           | Notes |
| ------------ | -------------- | ----- |
| `mode`       | string         | Only `rootfs-store` is supported today. |
| `packages`   | list of string | Realised as `pkgs.<name>` closures on the container PATH. |
| `command`    | list of string | The process to run. |
| `packageOps` | table          | `add` / `remove` lists + `replace` entries (`name`, `with`). Applied by the modules after the graph merge. |

> The direct CLI `render`/`run`/`up` path does **not** resolve the graph or apply
> `packageOps` — use the NixOS/HM modules (or a pre-resolved effective TOML) for
> those. The CLI fails loudly rather than rendering a partial config.

### `[config.container]`

`name`, `hostname`, `entrypoint` (list), `stopSignal`, `workingDir`, `user`,
`group`, `podmanArgs` (list), and `[config.container.environment]` (string map).

### `[config.filesystem]`

`readOnly` (bool), `readOnlyTmpfs` (bool), `tmpfs` (list), `mounts` (list of raw
mount strings), `[[config.filesystem.volumes]]` (`source`, `target`, `mode`;
targets must be unique), `[[config.filesystem.devices]]` (`source`, `target`,
`permissions`).

### `[config.network]` and units

- `[config.network]`: `mode`, `publish` (list), `dns` (list), `addHost` (list).
- `[[config.networks]]`: extra Quadlet `.network` units — `name`, `driver`,
  `internal`, `ipv6`, `subnet`, `gateway`, `ipRange`, `dns`, `options`,
  `[….labels]`, `[….quadlet]` passthrough.
- `[[config.volumes]]`: extra Quadlet `.volume` units — `name`, `driver`, `copy`,
  `options`, `[….labels]`, `[….quadlet]` passthrough.

### `[config.security]`

`dropCapabilities`, `addCapabilities`, `noNewPrivileges`, `privileged`,
`seccompProfile`, `securityLabelDisable`, `securityOpt`, `userns`. See
[security.md](security.md).

### `[config.resources]`

`memory`, `memorySwap`, `cpus`, `cpuQuota`, `pidsLimit`, `ulimits`.

### `[[config.secrets]]`

`name`, `target`, `type`, `uid`, `gid`, `mode`, `options`. References only —
never place secret bytes in TOML or the Nix store.

### `[config.workspace]` and `[config.home]`

Transient isolation primitives used by `graft up`/`run`:

- `[config.workspace]`: `mode` (`none` \| `copy`), `source`, `target`, `review`
  (`diff`).
- `[config.home]`: `ephemeral` (bool), `target`.

These are honoured by the CLI run path, not by module rendering. See
[agent-update-flow.md](agent-update-flow.md).

### `[config.service]`

`type`, `restart`, `restartSec`, `timeoutStartSec`, `timeoutStopSec`,
`remainAfterExit`. Rendered into the Quadlet `[Service]` section.

`type` must be `oneshot` (the default; for task containers that run once and
exit) or `notify` (for long-running services). Quadlet rejects any other value
for a `.container` unit — including the systemd-native `simple` — so graft fails
rendering early with a clear error instead of letting the generator silently
skip the unit.

### `[config.quadlet]` (raw passthrough)

`[config.quadlet.container]`, `[config.quadlet.service]`,
`[config.quadlet.install]` — string-list maps rendered verbatim. The escape
hatch for options graft does not model yet. `config.quadlet.container` must not
set keys the typed renderer owns (`Rootfs`, `ContainerName`, `Exec`, `Network`,
`Volume`); that is rejected by validation.
