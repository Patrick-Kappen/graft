# CLI

The command is:

```bash
graft
```

Run `graft --help` for the full usage; `graft --version` prints the version.

## Up

Run a TOML config directly through the transient Quadlet flow:

```bash
graft up ./config.toml
```

With no argument, `graft up` autodetects a config in the current directory, in
this order:

```text
graft.toml
.graft.toml
config.toml
```

For now `up` is a thin alias around the existing transient run flow. It will
grow into the fast project flow that resolves the TOML graph, realises the
runtime closure, and starts the container.

## Other commands

```text
graft config path | init [path] | show [path]   Manage the no-op example config
graft inspect <file.toml>                        Print resolved metadata as JSON
graft render <file.toml>                         Render Quadlet text
graft render-nixos <file.toml> <rootfs> <name>   Render with concrete store paths
graft render-nixos-units <file.toml> <rootfs> <name> <out-dir>
graft run <file.toml>                            Run via a temporary Quadlet unit
graft run-rootfs -- <command> [args...]          Run a command in a temporary rootfs unit
```

See [`config.md`](config.md) for the configuration reference and
[`getting-started.md`](getting-started.md) for a full walkthrough.
