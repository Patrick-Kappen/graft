# NixOS module

De NixOS-route gebruikt door de gebruiker geschreven TOML-bestanden als build-input. TOML zet alles: naam, packages, command, mounts, netwerk, security en resources. De module kan nu naar losse TOML-bestanden wijzen of naar een directory met `configRoot`.

```text
user-authored TOML files
  -> NixOS module build input
  -> Nix leest TOML met builtins.fromTOML
  -> parents.add resolven binnen configRoot
  -> effective TOML genereren in de store
  -> effective config.runtime.packages -> pkgs.<name> runtime closure
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

## Parent resolving

De NixOS-module ondersteunt nu de basis graph-stap: `parents.add`/`set`/`remove` en `children.add`/`set`/`remove`.

```text
containers/
  base.toml
  projects/app.toml
```

Parent:

```toml
# containers/base.toml
version = 1
name = "base"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive"]
command = ["bash", "-lc", "echo from parent"]

[config.container.environment]
FROM_PARENT = "1"
```

Child:

```toml
# containers/projects/app.toml
version = 1
name = "app"

[parents]
add = ["base"]

[deploy]
enable = true
target = "system"

[config.runtime]
packages = ["coreutils"]
command = ["bash", "-lc", "echo from child"]

[config.container.environment]
FROM_CHILD = "1"
```

Effectief:

- resolve-volgorde is `parents -> self -> children`;
- attrsets mergen recursief;
- lijsten concateneren met `lib.unique`;
- `config.runtime.command` wordt door latere lagen overschreven;
- scalar values worden door latere lagen overschreven;
- alleen de child wordt actief omdat alleen die `[deploy] enable = true` heeft.

De module genereert hiervoor een effective TOML in de Nix store en rendert daaruit Quadlet.

`parents.set`/`children.set` vervangen de lokale refs van die node. `parents.remove`/`children.remove` verwijderen refs uit de lokale lijst na `set`/`add` normalisatie.

## Package operations

Na graph merge past de NixOS-module package operations toe op `config.runtime.packages`.

```toml
[config.runtime]
packages = ["bashInteractive", "coreutils", "hello"]

[config.runtime.packageOps]
remove = ["coreutils"]
add = ["gnugrep"]

[[config.runtime.packageOps.replace]]
name = "hello"
with = "hostname"
```

Effectieve packages:

```toml
packages = ["bashInteractive", "hostname", "gnugrep"]
```

Volgorde:

1. verwijder `remove` en replacement-namen uit de bestaande package-lijst;
2. voeg replacement `with` packages toe;
3. voeg `add` packages toe;
4. deduplicate met `lib.unique`.

De effective TOML bevat daarna alleen `config.runtime.packages`; `packageOps` wordt niet aan de renderer doorgegeven.

## Huidige eerste implementatie

- TOML wordt niet uit Nix options gegenereerd.
- `configRoot` ontdekt recursief `*.toml` en activeert alleen TOML met `[deploy] enable = true` en system target.
- `configFiles` blijven beschikbaar als expliciete build inputs.
- Elk actief TOML-bestand moet een unieke top-level `name` hebben.
- De NixOS module resolved `parents.*` en `children.*`, past `config.runtime.packageOps` toe, en leest daarna `config.runtime.packages` uit de effective TOML.
- Runtime package strings worden vertaald naar `pkgs.<name>`.
- De renderer ondersteunt nu `config.runtime.mode = "rootfs-store"` met `runtime.command`.
- Package refs buiten simpele `pkgs.<name>` strings moeten nog volgen.

## Eindrichting

De TOML-map bepaalt via discovery/metadata welke containers managed zijn. Snelle projectcontainers hoeven geen NixOS rebuild te doen; die lopen via `pac up` en transient/user Quadlet.
