# Graft — Design

## Principe

Graft heeft drie lagen met strikte verantwoordelijkheden:

```text
TOML → CLI → JSON stdout → NixOS module → Quadlet .container
```

- **TOML** is onze user-facing Graft-taal: alleen echte keuzes van de user.
- **CLI** vertaalt die Graft-taal naar een complete resolved build spec.
- **NixOS module** materialiseert die spec naar rootfs + Quadlet `.container` file.

De user schrijft dus geen Quadlet en geen Nix-boilerplate. De `.container` file is output.

---

## TOML is user intent

De TOML beschrijft wat de user wil, niet hoe Podman/NixOS dat uitvoert.

Voorbeeld:

```toml
version = 1
name = "node-dev"

[config.runtime]
packages = ["nodejs"]
```

Daar staat bewust niet in:

- rootfs setup
- `/nix/store` mount
- overlay details
- default keep-alive command
- default restart policy
- default autostart/install section
- Quadlet boilerplate

---

## CLI resolve-logica

De CLI leest TOML en schrijft resolved JSON naar stdout. De CLI schrijft zelf geen JSON file in de repo.

```text
graft <container.toml> > $out
```

Nix vangt stdout op via IFD:

```nix
resolvedJson = pkgs.runCommand "graft-resolve-${name}" {} ''
  ${graft}/bin/graft ${tomlFile} > $out
'';

resolved = builtins.fromJSON (builtins.readFile resolvedJson);
```

De CLI is de enige laag die keuzes maakt:

- defaults toepassen
- dependencies toevoegen
- keep-alive regelen
- package-operaties verwerken
- TOML/Graft-concepten vertalen naar wat NixOS nodig heeft

---

## `graft-pause`

`graft-pause` is een minimale binary in dezelfde Rust crate als de CLI.

```text
/bin/graft
/bin/graft-pause
```

`graft-pause` is altijd aanwezig in de resolved package list en dus altijd aanwezig in de rootfs.

Regels:

```text
user geeft geen command  → command = ["/bin/graft-pause"]
user geeft wel command   → command = user command
```

In beide gevallen blijft `graft-pause` in `packages`, omdat hij klein is en altijd beschikbaar moet zijn.

Dus:

```text
packages = ["graft-pause", ...user packages]
```

Geen `bashInteractive`, geen `coreutils`, geen `sleep infinity` als default keep-alive.

---

## Defaults en expliciete keuzes

De CLI mag alleen defaults toevoegen die bij de Graft-logica horen.

| Veld | Regel |
|---|---|
| `packages` | altijd `graft-pause` + user packages |
| `command` | user command, of `/bin/graft-pause` als ontbreekt |
| `deploy.target` | default `system`, tenzij user `user` zet |
| `runtime.mode` | alleen `rootfs-store` |
| `restart` | geen default; alleen meenemen als user het zet |
| `deploy.enable` | geen default; alleen meenemen als user het zet |
| autostart / `[Install]` | geen default; alleen als user het expliciet configureert |

Een TOML-bestand laten bestaan betekent dat de module er een `.container` file van kan maken. Dat is iets anders dan de container automatisch starten.

---

## NixOS module is dumb

De NixOS module leest de resolved JSON en doet alleen mechanisch werk:

1. resolved package namen naar Nix packages vertalen
2. rootfs bouwen met `pkgs.buildEnv`
3. echte runtime directories toevoegen (`/etc`, `/tmp`, `/var`, `/run`, ...)
4. `/nix/store` read-only mounten in de container
5. Quadlet `.container` file renderen
6. symlink plaatsen onder `/etc/containers/systemd/` voor system containers

De module beslist niet:

- welke default command gebruikt wordt
- welke base package nodig is
- welke restart policy geldt
- of `coreutils`/`bash` nodig is
- hoe TOML-concepten geïnterpreteerd moeten worden

Dat is CLI-logica.

---

## Rootfs en Quadlet

Graft gebruikt rootfs uit de Nix store, geen images.

```ini
[Container]
ContainerName=node-dev
Rootfs=/nix/store/...-graft-node-dev-env:O
Exec=/bin/graft-pause
Volume=/nix/store:/nix/store:ro
```

Belangrijk:

- `Image=` wordt niet gebruikt voor rootfs-store containers.
- `Rootfs=...:O` geeft een writable overlay bovenop een read-only store rootfs.
- `/nix/store` wordt read-only in de container gemount.
- Niet in de store = niet beschikbaar in de container.

System containers gebruiken rootful Podman met kernel overlayfs via `:O`.
User containers gebruiken rootless overlay via `fuse-overlayfs`.

---

## Build/cache logica

Incrementality komt van Nix:

```text
TOML ongewijzigd → zelfde derivation → CLI draait niet opnieuw
TOML gewijzigd   → CLI draait opnieuw → nieuwe resolved JSON
packages anders  → rootfs verandert
alleen command/restart anders → Quadlet verandert, rootfs mogelijk niet
```

Geen hidden state. Geen JSON in git nodig.

---

## Later, niet nu

Niet onderdeel van de huidige build-time resolve-flow:

- promote/diff workflow
- graph merge (`parents` / `children`)
- per-container UID hardening
- runtime lifecycle commands

Als runtime commands later komen, zijn de afgesproken namen `graft up` en `graft down`; geen `graft shell`.
