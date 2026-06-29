# Minimale reproduceerbare Podman container

Dit project heet `podman-agent-container`; de korte CLI-naam is `pac`.

## Huidige verticale slice

Het project heeft nu een kleine werkende slice:

- Nix package/app;
- `pac` symlink naast `podman-agent-container`;
- TOML config-template;
- TOML inspect/render;
- `rootfs-store` runtime;
- Quadlet `.container` rendering;
- transient `systemctl --user` run/up;
- eerste NixOS-module die Quadlet naar `/etc/containers/systemd` rendert.

Standaard wordt nog steeds niets automatisch gestart. Een lege/no-op TOML blijft no-op.

## Rootfs-store

Er wordt geen image gebouwd. De container gebruikt een minimale rootfs en Nix store closures.

Conceptueel:

```text
Rootfs=<minimal-rootfs>
Volume=/nix/store:/nix/store:ro
Environment=PATH=<runtime-closure>/bin
Exec=<configured command>
```

Packages hoeven niet in host PATH te staan; ze hoeven alleen in `/nix/store` gerealiseerd te worden.

## CLI

```bash
pac config show
pac config path
pac config init
pac inspect examples/rootfs-store.toml
pac render examples/rootfs-store.toml
pac up examples/rootfs-store.toml
```

Autodetect:

```bash
pac up
```

zoekt in de huidige directory naar:

```text
pac.toml
podman-agent-container.toml
.pac.toml
config.toml
```

## Nog niet

- graph resolving;
- package add/remove/replace;
- directe closure build in `pac up` zonder NixOS module;
- workspace copy/jj candidate flow;
- shell hook zoals direnv;
- idle/leave lifecycle;
- Home Manager module.
