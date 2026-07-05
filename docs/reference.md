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

| Field             | Type              | Notes |
| ----------------- | ----------------- | ----- |
| `name`            | string            | Container name (defaults to top-level `name`). |
| `hostname`        | string            | Hostname inside the container. |
| `pod`             | string            | Attach to an existing pod. |
| `entrypoint`      | list of string    | Prepended before `command`. |
| `stopSignal`      | string            | Signal used to stop the container (default: `SIGTERM`). |
| `stopTimeout`     | int               | Seconds to wait for stop before killing. |
| `workingDir`      | string            | Initial working directory. |
| `user`            | string            | UID or username. |
| `group`           | string            | GID or group name. |
| `timezone`        | string            | Container timezone (e.g. `local`, `UTC`, `Europe/Amsterdam`). |
| `notify`          | string            | sd\_notify mode: `container` (container signals readiness) or `healthy` (on health check pass). |
| `runInit`         | bool              | Run an init process as PID 1. |
| `annotations`     | string map        | OCI annotations (`key = "value"`). |
| `environment`     | string map        | Environment variables. |
| `environmentFile` | list of string    | Files to load environment variables from. |
| `environmentHost` | bool              | Pass all host environment variables into the container. |
| `podmanArgs`      | list of string    | Extra args appended after `podman run`. |
| `globalArgs`      | list of string    | Extra args inserted before the `podman` subcommand. |
| `ip`              | string            | Fixed IPv4 address. |
| `ip6`             | string            | Fixed IPv6 address. |
| `networkAlias`    | list of string    | Network aliases. |
| `exposeHostPort`  | list of string    | Expose host ports without publishing them. |
| `uidMap`          | list of string    | UID mappings (e.g. `0:100000:65536`). |
| `gidMap`          | list of string    | GID mappings. |
| `subUidMap`       | string            | Subordinate UID map file entry (e.g. `@user`). |
| `subGidMap`       | string            | Subordinate GID map file entry. |
| `shmSize`         | string            | Shared memory size (e.g. `64m`). |
| `mask`            | list of string    | Paths to mask inside the container. |
| `unmaskPaths`     | list of string    | Paths to unmask inside the container. |
| `sysctl`          | list of string    | Kernel parameters (e.g. `net.ipv4.ip_unprivileged_port_start=80`). |
| `logDriver`       | string            | Log driver (e.g. `journald`, `k8s-file`). |

**`[config.container.health]`** — container health check:

| Field             | Type   | Notes |
| ----------------- | ------ | ----- |
| `cmd`             | string | Health check command. Required to enable health checks. |
| `interval`        | string | Time between checks (e.g. `30s`). |
| `timeout`         | string | Maximum time per check (e.g. `10s`). |
| `retries`         | int    | Consecutive failures before marking unhealthy. |
| `startPeriod`     | string | Grace period at startup (e.g. `5s`). |
| `onFailure`       | string | Action on failure: `kill`, `restart`, `stop`, or `none`. |
| `startupCmd`      | string | Startup health check command (separate from the regular check). |
| `startupInterval` | string | Interval for startup check. |
| `startupRetries`  | int    | Max retries for startup check. |
| `startupSuccess`  | int    | Consecutive successes to mark startup complete. |
| `startupTimeout`  | string | Timeout per startup check. |

### `[config.filesystem]`

`readOnly` (bool), `readOnlyTmpfs` (bool), `tmpfs` (list), `mounts` (list of raw
mount strings), `[[config.filesystem.volumes]]` (`source`, `target`, `mode`;
targets must be unique), `[[config.filesystem.devices]]` (`source`, `target`,
`permissions`).

### `[config.network]` and units

- `[config.network]`: `mode`, `publish` (list), `dns` (list), `dnsOption` (list), `dnsSearch` (list), `addHost` (list).
- `[[config.networks]]`: extra Quadlet `.network` units — `name`, `driver`,
  `internal`, `ipv6`, `subnet`, `gateway`, `ipRange`, `dns`, `options`,
  `[….labels]`, `[….quadlet]` passthrough.
- `[[config.volumes]]`: extra Quadlet `.volume` units — `name`, `driver`, `copy`,
  `options`, `[….labels]`, `[….quadlet]` passthrough.

### `[config.security]`

| Field                   | Type           | Notes |
| ----------------------- | -------------- | ----- |
| `dropCapabilities`      | list of string | Linux capabilities to drop (e.g. `["all"]`). |
| `addCapabilities`       | list of string | Linux capabilities to add back. |
| `noNewPrivileges`       | bool           | Prevent privilege escalation. |
| `privileged`            | bool           | Full host access (dangerous). |
| `seccompProfile`        | string         | Path to a seccomp JSON profile. |
| `securityLabelDisable`  | bool           | Disable SELinux / AppArmor labels. |
| `securityLabelFileType` | string         | SELinux file type label. |
| `securityLabelLevel`    | string         | SELinux level label. |
| `securityLabelNested`   | bool           | Allow nested SELinux labels. |
| `securityLabelType`     | string         | SELinux type label. |
| `securityOpt`           | list of string | Raw `--security-opt` values. |
| `userns`                | string         | User namespace mode (e.g. `keep-id`). |

See [security.md](security.md) for details.

### `[config.resources]`

`memory`, `memorySwap`, `cpus`, `cpuQuota`, `pidsLimit`, `ulimits`.

### `[[config.secrets]]`

`name`, `target`, `type`, `uid`, `gid`, `mode`, `options`. References only —
never place secret bytes in TOML or the Nix store.

### `[config.workspace]` and `[config.home]`

Transient isolation primitives used by `graft up`/`run`/`start`:

**`[config.workspace]`**

| Field             | Type   | Notes |
| ----------------- | ------ | ----- |
| `mode`            | string | `none` (default) or `copy` — copy the source tree into an isolated workspace. |
| `source`          | string | Host path to copy (default: `.`). |
| `target`          | string | Mount point inside the container (default: `/workspace`). |
| `review`          | string | `diff` — print a diff on exit. |
| `promote`         | string | What to do with changes after exit: `off` (default), `prompt`, or `auto`. |
| `excludePatterns` | list   | Directories to skip when copying (default: `.git .jj .go .direnv result node_modules`). |

**`[config.home]`**

| Field       | Type   | Notes |
| ----------- | ------ | ----- |
| `mode`      | string | `ephemeral` (temp dir, wiped on each run), `persistent` (host dir survives across runs), or `session` (copy on start, review/promote on stop). |
| `source`    | string | Host path for `mode = "persistent"` or `"session"`. Supports `~` expansion. |
| `target`    | string | Mount point inside the container (default: `/home/user`). Sets `HOME` and `XDG_*` env vars. |
| `review`    | string | `diff` — print a diff before the promote step (session mode only). |
| `promote`   | string | What to do with session changes on stop: `auto` (always apply), `prompt` (ask), or `never` (discard, default). Session mode only. |
| `ephemeral` | bool   | Legacy alias for `mode = "ephemeral"`. Prefer `mode`. |

**`[[config.home.shadow]]`** — extra writable paths, isolated per session:

| Field       | Type   | Notes |
| ----------- | ------ | ----- |
| `container` | string | Path inside the container (e.g. `/workspace`). |
| `host`      | string | Host directory to seed from and promote changes back to. Supports `~` expansion. |

Shadow mounts are backed by per-session copies. Use `graft diff` to review
changes and `graft promote` to copy them back to the host path. `graft reset`
clears all session data.

These are honoured by the CLI run/start path, not by module rendering.

### `[config.attach]`

Controls how `graft attach` and the `graft <instance>` shortcut connect to a
running container.

| Field        | Type   | Notes |
| ------------ | ------ | ----- |
| `tmuxSession` | string | tmux session name to attach to or create (default: `main`). |
| `shell`       | string | Fallback interactive shell when tmux is unavailable (default: `sh`). |
| `startDelay`  | string | How long to wait after `graft up` before attaching. Go duration string, e.g. `500ms`, `2s` (default: `500ms`). |

tmux is optional. graft tries: attach to existing session → start new session → exec shell.

### `[config.service]`

| Field              | Type   | Notes |
| ------------------ | ------ | ----- |
| `type`             | string | `oneshot` (default) or `notify`. Quadlet rejects other values. |
| `restart`          | string | e.g. `on-failure`, `always`. |
| `restartSec`       | string | Delay between restarts (e.g. `10s`). |
| `timeoutStartSec`  | string | Start timeout (e.g. `2m`). |
| `timeoutStopSec`   | string | Stop timeout (e.g. `30s`). |
| `remainAfterExit`  | bool   | Keep service active after container exits. |
| `restartIfChanged` | bool   | Whether NixOS restarts this container on `nixos-rebuild switch`. Set to `false` to keep a running container alive across rebuilds (default: `true`). |

Rendered into the Quadlet `[Service]` section.

### `[config.quadlet]` (raw passthrough)

`[config.quadlet.container]`, `[config.quadlet.service]`,
`[config.quadlet.install]` — string-list maps rendered verbatim. The escape
hatch for options graft does not model yet. `config.quadlet.container` must not
set keys the typed renderer owns (`Rootfs`, `ContainerName`, `Exec`, `Network`,
`Volume`); that is rejected by validation.
