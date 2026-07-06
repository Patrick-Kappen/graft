# Graft — Design

## Principe

TOML → config file → klaar.

De TOML is voor de user. Geen boilerplate, alleen echte keuzes.
De CLI bevat alle logica en defaults. De Nix module is dom.

---

## Flow

```
TOML
  ↓
nixos-rebuild switch
  ↓
CLI resolved (base packages, defaults, dependencies)
  ↓
Nix bouwt rootfs in store  →  /nix/store/xxx-graft-<naam>-env
  ↓
Quadlet .container file gegenereerd (alles erin, niks meer nodig)
  ↓
Symlink: /etc/containers/systemd/<naam>.container → store
  ↓
systemd weet ervan — container start NIET automatisch tenzij geconfigureerd
```

---

## Container

- Alles is een service
- Rootfs = Nix store (read-only lowerdir) + overlay upperdir (writable, weg bij stop)
- `/nix/store` van de host gemount in de container (read-only)
- Niet in de store = niet beschikbaar in de container
- Keep-alive en base packages regelt de CLI — user ziet dat niet

---

## TOML

Alleen user keuzes. Minimaal voorbeeld:

```toml
version = 1
name    = "mycontainer"

[deploy]
enable = true
target = "system"
```

Extra packages als echt nodig:

```toml
[config.runtime]
packages = ["nodejs"]
```

---

## CLI

- Bevat alle logica: base packages, defaults, dependency resolution
- Genereert de complete Quadlet config tijdens `nixos-rebuild`
- Nix module bouwt rootfs, legt symlinks neer — geen logica

## Commands (later)

- `graft up <naam>` — start de container (`systemctl start`)
- `graft down <naam>` — stop de container
- Meer commands komen wanneer nodig, namen bepaalt de user
