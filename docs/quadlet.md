# Quadlet — Graft output

## Wat is Quadlet?

Quadlet is een systemd generator die `.container` files leest en daar gewone
systemd `.service` units van maakt. Podman wordt daarna door systemd gestart.

In Graft is Quadlet **output**. De user schrijft TOML; de NixOS module rendert
uiteindelijk de `.container` file.

```text
TOML → CLI resolved JSON → NixOS module → .container
```

## File locaties

| Scope | Pad |
|---|---|
| System/root | `/etc/containers/systemd/` |
| User/rootless | `~/.config/containers/systemd/` |

Symlinks worden ondersteund. NixOS kan dus een file in de store bouwen en via
`environment.etc` naar `/etc/containers/systemd/` linken.

## Verantwoordelijkheden

### CLI

De CLI vertaalt TOML naar resolved JSON:

- package list
- command/Exec
- deploy target
- optionele service/install instellingen
- alle defaults en impliciete dependencies

De CLI rendert niet zelf de `.container` file.

### NixOS module

De NixOS module gebruikt de resolved JSON om mechanisch Quadlet te renderen:

- `ContainerName=` uit resolved name
- `Rootfs=` uit de gebouwde rootfs store path
- `Exec=` uit resolved command
- `Volume=/nix/store:/nix/store:ro` altijd voor store symlinks
- optionele `[Service]` keys alleen als resolved JSON ze bevat
- optionele `[Install]` keys alleen als autostart expliciet is geconfigureerd

## Rootfs-store mapping

Graft gebruikt rootfs uit de Nix store, geen images.

| Quadlet optie | Bron |
|---|---|
| `ContainerName=` | resolved `name` |
| `Rootfs=<path>:O` | NixOS module bouwt rootfs uit resolved `packages` |
| `Exec=` | resolved `command` |
| `Volume=/nix/store:/nix/store:ro` | altijd nodig voor Nix store symlinks |

Voorbeeld zonder user command:

```ini
[Container]
ContainerName=node-dev
Rootfs=/nix/store/xyz-graft-node-dev-env:O
Exec=/bin/graft-pause
Volume=/nix/store:/nix/store:ro
```

Voorbeeld met user command:

```ini
[Container]
ContainerName=web
Rootfs=/nix/store/xyz-graft-web-env:O
Exec=node server.js
Volume=/nix/store:/nix/store:ro
```

## `graft-pause`

`graft-pause` zit altijd in de rootfs.

```text
packages = ["graft-pause", ...user packages]
```

Maar `Exec=/bin/graft-pause` wordt alleen gebruikt wanneer de user geen command
heeft opgegeven. Als de user wel een command opgeeft, wordt dat command `Exec=`.

Dit voorkomt default dependencies op `bashInteractive`, `coreutils` of
`sleep infinity`.

## Restart

`Restart=` heeft geen Graft default.

Alleen als de user restart expliciet configureert, komt er een `[Service]`
sectie met restart-instelling in de `.container` output.

Voorbeeld expliciet:

```ini
[Service]
Restart=on-failure
```

Zonder expliciete restart wordt dit niet gerenderd.

## Auto-start bij boot

Een `.container` file mag bestaan zonder automatisch te starten.

Auto-start gebeurt alleen als een `[Install]` sectie expliciet wordt
gegenereerd, bijvoorbeeld:

```ini
[Install]
WantedBy=multi-user.target
```

Weglaten betekent: systemd kent de unit, maar start hem niet automatisch.

## Overlay

Rootfs-store containers gebruiken overlay bovenop de read-only store rootfs:

```text
lowerdir = /nix/store/xxx-graft-env   (read-only)
upperdir = container storage          (writable)
```

Schrijven in de container gaat naar de upperdir. Die upperdir is later de basis
voor promote/diff workflows.

System containers (`target = "system"`) gebruiken rootful Podman met kernel
overlayfs via `:O`. User containers (`target = "user"`) gebruiken rootless
overlay via `fuse-overlayfs`.

## Niet gebruikt door rootfs-store Graft containers

- `Image=` — Graft gebruikt `Rootfs=` voor deze mode
- image downloads at runtime
- user-written `.container` files as input
- default `Restart=on-failure`
- default `[Install] WantedBy=...`
