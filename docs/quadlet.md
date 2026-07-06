# Quadlet — Referentie voor Graft CLI

## Wat is Quadlet?

Quadlet is een systemd generator die `.container` files leest en daarvan
gewone systemd `.service` units genereert. Podman wordt daarna aangeroepen
door systemd via `ExecStart=podman run ...`.

## File locaties

| Scope | Pad |
|-------|-----|
| System (root) | `/etc/containers/systemd/` |
| User | `~/.config/containers/systemd/` |

Symlinks worden ondersteund — NixOS `environment.etc` aanpak is correct.

## Hoe de CLI de `.container` file genereert

De CLI vertaalt TOML → Quadlet opties. TOML en Quadlet hoeven niet 1:1 te
mappen — de CLI is de vertaler. De user kent alleen TOML.

## Key Quadlet [Container] opties

| Quadlet optie | Betekenis | TOML veld |
|---|---|---|
| `Rootfs=path:O` | Gebruik rootfs met overlay | `config.runtime.mode = "rootfs-store"` |
| `Exec=cmd` | Commando dat draait | `config.runtime.command` |
| `ContainerName=name` | Container naam | `name` |
| `Volume=src:dst:opts` | Mount | `config.filesystem.volumes` |
| `Environment=KEY=val` | Env variabele | `config.container.environment` |
| `PublishPort=8080:80` | Port mapping | `config.network.publish` |
| `Network=host` | Netwerk mode | `config.network.mode` |
| `HostName=name` | Hostname | `config.container.hostname` |
| `NoNewPrivileges=true` | Security | `config.security.noNewPrivileges` |
| `DropCapability=CAP` | Drop capability | `config.security.dropCapabilities` |
| `PodmanArgs=...` | Escape hatch voor overige opties | `config.container.podmanArgs` |

## Altijd aanwezig (CLI default)

```ini
Volume=/nix/store:/nix/store:ro
```

## Auto-start bij boot

Alleen als dit in het file staat:

```ini
[Install]
WantedBy=multi-user.target
```

Weglaten = unit bestaat, maar start niet automatisch.

## Drop-in files

Overrides zonder het hoofdbestand te wijzigen:

```
/etc/containers/systemd/myapp.container.d/10-override.conf
```

Relevant voor later (per-machine overrides op systeem containers).

## Overlay (rootfs-store mode)

```
lowerdir = /nix/store/xxx-graft-env   (read-only)
upperdir = /var/lib/containers/storage/overlay-containers/xxx/userdata
```

Schrijven in de container gaat naar de upperdir — weg bij stop.
De upperdir is de basis voor de promote/diff workflow (later).

## Niet gebruikt door Graft

- `Image=` — wij gebruiken `Rootfs=`, geen image downloads
- Kubernetes YAML (`podman kube play`) — voor image-based containers
