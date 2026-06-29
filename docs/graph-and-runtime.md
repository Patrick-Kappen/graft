# TOML graph and Nix store runtimes

This document records the intended graph/runtime layer. See also
[`vision.md`](vision.md).

## Goal

Users declare containers in TOML. Nix/HM/CLI point at as little as possible; the
TOML directory determines what can come into existence.

```text
containers/
  base/
    server.toml
    locked.toml
  projects/
    my-app.toml
```

A node can simultaneously be a template, parent, child, addon, or concrete
container.

## Example tree

See [`../examples/config-root`](../examples/config-root) for a working configRoot
example with:

- parent nodes;
- a child addon;
- package operations;
- one deploy-enabled app.

## Node model

```toml
version = 1
name = "projects/my-app"

[parents]
add = ["base/server"]
remove = []
set = []

[children]
add = []
remove = []
set = []

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils"]
command = ["bash", "-lc", "echo hello"]
```

Intended resolution order:

```text
parents -> self -> children
```

The current NixOS module supports this base order, including `parents.add`/
`set`/`remove` and `children.add`/`set`/`remove`.

Later layers may override or extend earlier ones according to explicit merge
rules.

## Entries are not the end goal

The current NixOS module still uses `configFiles`. The end goal is shorter:

```nix
services.graft = {
  enable = true;
  configRoot = ./containers;
};
```

Home Manager / rootless user Quadlet:

```nix
programs.graft = {
  enable = true;
  configRoot = ./containers;
};
```

The directory/TOML then determines what is active, transient, persistent, or
promotable.

## Current merge rules

For `parents` and `children` in the NixOS module:

- refs are relative to `configRoot` without `.toml`, e.g. `"base/server"` ->
  `configRoot/base/server.toml`;
- `set` replaces the node's local ref list;
- if `set` is empty, the node uses `add`;
- `remove` drops refs from that local list;
- parents are merged before self;
- children are merged after self;
- attrsets merge recursively;
- lists concatenate with `lib.unique`;
- `config.runtime.command` is an override list: child replaces parent;
- scalars are overridden by later layers;
- parent cycles raise an error.

Example:

```toml
# base/server.toml
version = 1
name = "base/server"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive"]
command = ["bash", "-lc", "echo parent"]
```

```toml
# projects/app.toml
version = 1
name = "app"

[parents]
add = ["base/server"]

[deploy]
enable = true
target = "system"

[config.runtime]
packages = ["coreutils"]
command = ["bash", "-lc", "echo child"]
```

Effective runtime:

```toml
[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils"]
command = ["bash", "-lc", "echo child"]
```

## Package overrides

Base container:

```toml
version = 1
name = "base/server"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils", "appA", "appB", "appC"]
```

Project override:

```toml
version = 1
name = "projects/my-app"

[parents]
add = ["base/server"]

[config.runtime.packageOps]
remove = ["appB"]
add = ["appZ"]

[[config.runtime.packageOps.replace]]
name = "appC"
with = "appC_pinned"
```

Effective:

```text
base packages
  - appB
  replace appC
  + appZ
```

## Nix store runtime

`rootfs-store` builds no image. Nix only realises store closures.

```text
TOML packages
  -> Nix package refs
  -> runtime buildEnv/symlinkJoin
  -> Quadlet Rootfs= + PATH to runtime/bin
```

A package can live in `/nix/store` without being installed on the host `PATH`.

```text
package in /nix/store        yes
package on host PATH         no
package on container PATH    yes
```

## Cache

Runtime closures can come from a binary cache. If the store paths already exist,
starting/switching is fast. Otherwise Nix downloads or builds only the missing
closures.

## Store access

First practical mode:

```toml
[config.runtime]
storeAccess = "full-readonly"
```

Quadlet then conceptually mounts `/nix/store:/nix/store:ro`.

A later, stricter mode:

```toml
[config.runtime]
storeAccess = "closure-only"
```

Then the container only sees the required closure paths. This is safer but more
complex and must be tested separately.

## Fast CLI flow

No NixOS rebuild:

```bash
graft up ./graft.toml
```

Flow:

```text
TOML
  -> resolve graph
  -> build/reuse runtime closure
  -> render transient Quadlet
  -> systemctl --user start
```

## Promoted flow

When a dynamic container is used more often:

```text
working config
  -> graft promote
  -> resolved/promoted TOML in infra repo
  -> jj branch / PR / merge
  -> NixOS/HM managed Quadlet
```

## Open implementation points

- package refs beyond simple `pkgs.<name>` strings;
- duplicate-name checks exist, but could get richer error messages;
- shared resolution engine across CLI and Nix paths.
