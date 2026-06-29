# configRoot example

This directory demonstrates the intended NixOS `configRoot` flow.

```nix
services.podman-agent-container = {
  enable = true;
  configRoot = ./examples/config-root;
};
```

Only `apps/demo.toml` is deployed because it contains:

```toml
[deploy]
enable = true
target = "system"
```

The other TOML files are reusable graph nodes.

## Graph

```text
base/runtime
base/locked
  -> apps/demo
       -> addons/hostname
```

Resolve order:

```text
parents -> self -> children
```

## Package operations

`base/runtime.toml` starts with:

```toml
packages = ["bashInteractive", "coreutils", "hello"]
command = ["hello"]
```

`addons/hostname.toml` changes that with:

```toml
[config.runtime.packageOps]
remove = ["hello"]
add = ["gnugrep"]

[[config.runtime.packageOps.replace]]
name = "hello"
with = "hostname"

[config.runtime]
command = ["hostname"]
```

Effective runtime packages become approximately:

```toml
packages = ["bashInteractive", "coreutils", "hostname", "gnugrep"]
command = ["hostname"]
```

The NixOS module generates effective TOML in the Nix store and renders a Quadlet unit:

```text
/etc/containers/systemd/pac-demo.container
```
