# NixOS module

De NixOS-route gebruikt door de gebruiker geschreven TOML-bestanden als build-input. TOML zet alles: naam, packages, command, mounts, netwerk, security en resources. De module kan nu naar losse TOML-bestanden wijzen of naar een directory met `configRoot`.

```text
user-authored TOML files
  -> NixOS module build input
  -> Nix leest TOML met builtins.fromTOML
  -> config.runtime.packages -> pkgs.<name> runtime closure
  -> renderer maakt Quadlet tijdens Nix build
  -> /etc/containers/systemd/<name>.container
  -> Podman Quadlet/systemd start container
```

## Directory-discovery voorbeeld

```nix
{
  services.podman-agent-container = {
    enable = true;
    configRoot = ./containers;
  };
}
```

De module ontdekt recursief `*.toml`. Een ontdekt bestand wordt alleen een NixOS-managed container als het expliciet deploy aanzet:

```toml
[deploy]
enable = true
target = "system"
```

## Expliciete bestanden

Dit blijft voorlopig ondersteund:

```nix
{
  services.podman-agent-container = {
    enable = true;
    configFiles = [
      ./containers/go-dev.toml
      ./containers/pi-agent.toml
    ];
  };
}
```

Expliciete `configFiles` zijn actief zodra ze niet no-op zijn.

De entry/unit/containernaam komt uit TOML:

```toml
version = 1
name = "go-dev"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils", "go", "gopls"]
command = ["bash", "-lc", "go test ./..."]
```

Dit wordt:

```text
/etc/containers/systemd/go-dev.container
```

## Kale TOML-template

De meegeleverde template doet niets. Leeg betekent no-op.

```toml
version = 1
name = "example"

[parents]
add = []
remove = []
set = []

[children]
add = []
remove = []
set = []

[config]
# Empty means no-op.
```

No-op TOML-bestanden zijn geldig maar installeren geen Quadlet unit.

## Huidige eerste implementatie

- TOML wordt niet uit Nix options gegenereerd.
- `configRoot` ontdekt recursief `*.toml` en activeert alleen TOML met `[deploy] enable = true` en system target.
- `configFiles` blijven beschikbaar als expliciete build inputs.
- Elk actief TOML-bestand moet een unieke top-level `name` hebben.
- De NixOS module leest `config.runtime.packages` uit TOML en vertaalt die naar `pkgs.<name>`.
- De renderer ondersteunt nu `config.runtime.mode = "rootfs-store"` met `runtime.command`.
- Graph resolving voor `parents`/`children` moet nog volgen.

## Eindrichting

De TOML-map bepaalt via discovery/metadata welke containers managed zijn. Snelle projectcontainers hoeven geen NixOS rebuild te doen; die lopen via `pac up` en transient/user Quadlet.
