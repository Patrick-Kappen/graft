# NixOS module

The NixOS route uses user-authored TOML files as build input. TOML sets
everything: name, packages, command, mounts, network, security, and resources.
The module can point at individual TOML files or at a directory via `configRoot`.

```text
user-authored TOML files
  -> NixOS module build input
  -> Nix reads TOML with builtins.fromTOML
  -> resolve parents.add within configRoot
  -> generate effective TOML in the store
  -> effective config.runtime.packages -> pkgs.<name> runtime closure
  -> renderer produces Quadlet during the Nix build
  -> /etc/containers/systemd/<name>.container
  -> Podman Quadlet/systemd starts the container
```

## Directory-discovery example

```nix
{
  services.graft = {
    enable = true;
    configRoot = ./containers;
  };
}
```

A complete example tree is in [`../examples/config-root`](../examples/config-root):

```text
examples/config-root/
  base/runtime.toml
  base/locked.toml
  addons/hostname.toml
  apps/demo.toml
```

The module discovers `*.toml` recursively. A discovered file becomes a
NixOS-managed container only when it explicitly opts in to deployment:

```toml
[deploy]
enable = true
target = "system"
```

## Explicit files

This stays supported for now:

```nix
{
  services.graft = {
    enable = true;
    configFiles = [
      ./containers/go-dev.toml
      ./containers/pi-agent.toml
    ];
  };
}
```

Explicit `configFiles` are active as soon as they are not no-op.

## Nix-native authoring

Containers can also be authored directly in Nix instead of TOML files, via
`services.graft.containers.<name>`:

```nix
{
  services.graft = {
    enable = true;
    configRoot = ./containers; # optional: parents/children refs resolve here

    containers.go-dev = {
      # name defaults to the attribute name ("go-dev"); version defaults to 1.
      config.runtime = {
        mode = "rootfs-store";
        packages = [ "bashInteractive" "coreutils" "go" "gopls" ];
        command = [ "bash" "-lc" "go test ./..." ];
      };
    };
  };
}
```

The attrset mirrors the TOML schema one-to-one (`version`, `name`, `parents`,
`children`, `deploy`, `validation`, `config`). It is serialized to TOML with the
same `pkgs.formats.toml` formatter that produces the effective config, then runs
through the **same** resolver and renderer as file-based configs — there is no
second engine. Nix-authored containers are always active (like `configFiles`),
so `[deploy] enable` is not required; `parents`/`children` refs resolve against
`configRoot`. See [reference.md](reference.md#nix-native-authoring-containers).

## File-based naming

The entry/unit/container name comes from TOML:

```toml
version = 1
name = "go-dev"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils", "go", "gopls"]
command = ["bash", "-lc", "go test ./..."]
```

This becomes:

```text
/etc/containers/systemd/go-dev.container
```

## Bare TOML template

The shipped template does nothing. Empty means no-op.

```toml
version = 1
name = "example"

[parents]
add = []
remove = []
set = []

[children]
add = []
remove = []
set = []

[config]
# Empty means no-op.
```

No-op TOML files are valid but install no Quadlet unit.

## Parent resolving

The NixOS module supports the base graph step: `parents.add`/`set`/`remove` and
`children.add`/`set`/`remove`.

```text
containers/
  base.toml
  projects/app.toml
```

Parent:

```toml
# containers/base.toml
version = 1
name = "base"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive"]
command = ["bash", "-lc", "echo from parent"]

[config.container.environment]
FROM_PARENT = "1"
```

Child:

```toml
# containers/projects/app.toml
version = 1
name = "app"

[parents]
add = ["base"]

[deploy]
enable = true
target = "system"

[config.runtime]
packages = ["coreutils"]
command = ["bash", "-lc", "echo from child"]

[config.container.environment]
FROM_CHILD = "1"
```

Effective:

- resolution order is `parents -> self -> children`;
- attrsets merge recursively;
- lists concatenate with `lib.unique`;
- `config.runtime.command` is overridden by later layers;
- scalar values are overridden by later layers;
- only the child becomes active because only it has `[deploy] enable = true`.

The module generates an effective TOML in the Nix store and renders Quadlet from
it.

`parents.set`/`children.set` replace the local refs of that node.
`parents.remove`/`children.remove` drop refs from the local list after the
`set`/`add` normalization.

## Package operations

After the graph merge, the NixOS module applies package operations to
`config.runtime.packages`.

```toml
[config.runtime]
packages = ["bashInteractive", "coreutils", "hello"]

[config.runtime.packageOps]
remove = ["coreutils"]
add = ["gnugrep"]

[[config.runtime.packageOps.replace]]
name = "hello"
with = "hostname"
```

Effective packages:

```toml
packages = ["bashInteractive", "hostname", "gnugrep"]
```

Order:

1. remove `remove` and replacement names from the existing package list;
2. add replacement `with` packages;
3. add `add` packages;
4. de-duplicate with `lib.unique`.

The effective TOML then contains only `config.runtime.packages`; `packageOps` is
not passed to the renderer.

## Current first implementation

- Containers can be authored as TOML files (`configFiles`/`configRoot`) or
  directly in Nix (`containers.<name>`); Nix attrsets are serialized to TOML and
  share the same resolver and renderer.
- `configRoot` discovers `*.toml` recursively and activates only TOML with
  `[deploy] enable = true` and a system target.
- `configFiles` remain available as explicit build inputs.
- Each active TOML file must have a unique top-level `name`.
- The NixOS module resolves `parents.*` and `children.*`, applies
  `config.runtime.packageOps`, and then reads `config.runtime.packages` from the
  effective TOML.
- Runtime package strings are translated to `pkgs.<name>`.
- The renderer supports `config.runtime.mode = "rootfs-store"` with
  `runtime.command`.
- Package refs beyond simple `pkgs.<name>` strings are still to come.

## Service section

TOML can set selected systemd service options:

```toml
[config.service]
type = "notify" # "oneshot" (default) or "notify"; Quadlet rejects other values
restart = "on-failure"
restartSec = "10s"
timeoutStartSec = "2m"
timeoutStopSec = "30s"
remainAfterExit = false
```

These are rendered into `[Service]`. Enabling/starting policy is still separate
and will be added later.

## End direction

The TOML directory determines, via discovery/metadata, which containers are
managed. Fast project containers need no NixOS rebuild; they run via `graft up`
and transient/user Quadlet.
