# Minimal reproducible Podman container

This project is `graft`; the binary is also `graft`.

## Current vertical slice

The project has a small working slice:

- Nix package/app;
- the `graft` binary;
- TOML config template;
- TOML inspect/render;
- `rootfs-store` runtime;
- Quadlet `.container` rendering;
- transient `systemctl --user` run/up;
- a first NixOS module that renders Quadlet to `/etc/containers/systemd`.

By default nothing is started automatically. An empty/no-op TOML stays no-op.

## Rootfs-store

No image is built. The container uses a minimal rootfs and Nix store closures.

Conceptually:

```text
Rootfs=<minimal-rootfs>
Volume=/nix/store:/nix/store:ro
Environment=PATH=<runtime-closure>/bin
Exec=<configured command>
```

Packages do not need to be on the host `PATH`; they only need to be realised in
`/nix/store`.

## CLI

```bash
graft config show
graft config path
graft config init
graft inspect examples/rootfs-store.toml
graft render examples/rootfs-store.toml
graft up examples/rootfs-store.toml
```

Autodetect:

```bash
graft up
```

searches the current directory for:

```text
graft.toml
.graft.toml
config.toml
```

## Not yet

- graph resolving in the CLI;
- package refs/pins beyond simple `pkgs.<name>` strings;
- direct closure build in `graft up` without the NixOS module;
- workspace copy/jj candidate flow;
- a direnv-style shell hook;
- idle/leave lifecycle.
