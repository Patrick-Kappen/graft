# Design: git-driven Podman agent containers

`podman-agent-container` wordt een git-driven, TOML-first wrapper bovenop Podman Quadlet.

Zie eerst [`vision.md`](vision.md) voor de actuele productvisie: `pac` als direnv voor containers.

## Kernidee

```text
Git repo              = bron van waarheid
TOML configs          = declaratieve container-definities
NixOS/Home Manager    = installeert de wrapper en wijst naar configRoot
Podman Quadlet        = runtime/systemd uitvoerder
```

Er zijn twee routes:

```text
snelle route: TOML -> pac up -> transient Quadlet -> container draait
promote route: TOML -> review/branch/merge -> NixOS/HM managed Quadlet
```

## Geen impliciete defaults

Leeg betekent altijd: niets doen.

```toml
version = 1

[config]
# Empty means no-op.
```

of:

```toml
[containers]
# Empty means no-op.
```

mag geen container, baseline, mount of security policy impliciet activeren.

Alles moet expliciet uit TOML komen.

## TOML graph

Een config file is een named unit. De naam komt bij voorkeur uit de TOML `name`.

Voorbeeld:

```text
configs/pi-agent.toml -> pi-agent
```

Een TOML file kan verwijzen naar parents en children:

```toml
version = 1
name = "pi-agent"

[parents]
add = ["base", "no-network"]

[children]
add = ["nix-store", "workspace"]

[config]
# Empty means no-op.
```

Resolve-volgorde:

```text
parents -> self -> children
```

Latere lagen mogen eerdere lagen overschrijven. Hierdoor kunnen gebruikers zelf presets, addons en parent/child-combinaties bouwen.

## Gebruiker bepaalt presets

Er zijn geen ingebouwde presets zoals `bare`, `safe`, `agent` of `pi` die automatisch gedrag activeren.

Gebruikers kunnen zulke presets zelf definiëren:

```toml
# configs/no-network.toml
version = 1
name = "no-network"

[config.network]
mode = "none"
```

```toml
# configs/with-nix-store.toml
version = 1
name = "with-nix-store"

[[config.filesystem.volumes]]
source = "/nix/store"
target = "/nix/store"
mode = "ro"
```

```toml
# configs/pi-agent.toml
version = 1
name = "pi-agent"

[parents]
add = ["no-network", "with-nix-store"]
```

Deze voorbeelden zijn richtinggevend; het volledige schema moet nog worden uitgewerkt.

## NixOS blijft kort

De NixOS/Home Manager configuratie moet alleen de package/module activeren en naar de git-tracked TOML verwijzen.

Richting:

```nix
services.podman-agent-container = {
  enable = true;
  configRoot = ./containers;
};
```

De inhoud van containers, presets en deploy/session policy staat niet in `flake.nix`, maar in TOML.

## Quadlet als uitvoerlaag

Uiteindelijk rendert de wrapper effectieve TOML-configs naar native Podman Quadlet units.

```text
TOML graph
  -> resolve/merge
  -> validate
  -> render .container/.volume/.network units
  -> systemd/Podman Quadlet draait ze
```

`podman-agent-container` is dus geen vervanging voor Podman Quadlet, maar een hogere declaratieve laag erboven.

## Git-driven updates

Updates mogen niet direct de actieve omgeving muteren.

Gewenste flow:

```text
tmp/candidate container
  -> update/install draait geïsoleerd
  -> resultaat wordt TOML/profile/snapshot wijziging
  -> diff/PR
  -> merge
  -> switch/apply
```

De waarheid blijft de Git repo, niet runtime state zoals:

```text
~/.config
~/.local/state
podman containers
npm cache
Pi runtime config
```

## Huidige scope

Voor nu is er een werkende verticale slice: TOML laden, inspect/render/run, `pac up`, rootfs-store Quadlet en een NixOS-module met `configFiles`, `configRoot` discovery en `parents.add` resolving.

Zie ook [`runtime-architecture.md`](runtime-architecture.md) voor de geplande scheiding tussen TOML config engine, Quadlet runtime manager en cleanup/lifecycle beleid.

Zie [`nixos-module.md`](nixos-module.md) voor de eerste NixOS-route die TOML tijdens de Nix build naar `/etc/containers/systemd/*.container` rendert.

Nog niet bouwen:

- geen automatische containers
- geen ingebouwde baseline
- geen impliciete Quadlet units
- geen directe Pi/npm update-flow

Volgende doel: `parents.remove`/`parents.set`, `children.*`, package operations en daarna session lifecycle (`enter`/`leave`/`idle`).
