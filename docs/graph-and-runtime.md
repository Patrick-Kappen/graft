# TOML graph en Nix store runtimes

Dit document legt de beoogde graph/runtime-laag vast. Zie ook [`vision.md`](vision.md).

## Doel

Gebruikers declareren containers in TOML. Nix/HM/CLI wijzen zo min mogelijk aan; de TOML-map bepaalt wat er kan ontstaan.

```text
containers/
  base/
    server.toml
    locked.toml
  projects/
    my-app.toml
```

Een node kan tegelijk template, parent, child, addon of concrete container zijn.

## Voorbeeldboom

Zie [`../examples/config-root`](../examples/config-root) voor een werkend configRoot voorbeeld met:

- parent nodes;
- child addon;
- package operations;
- één deploy-enabled app.

## Node model

```toml
version = 1
name = "projects/my-app"

[parents]
add = ["base/server"]
remove = []
set = []

[children]
add = []
remove = []
set = []

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils"]
command = ["bash", "-lc", "echo hello"]
```

Beoogde resolve-volgorde:

```text
parents -> self -> children
```

Huidige NixOS-module ondersteunt deze basisvolgorde, inclusief `parents.add`/`set`/`remove` en `children.add`/`set`/`remove`.

Latere lagen mogen eerdere lagen overschrijven of uitbreiden volgens expliciete merge-regels.

## Geen entries als einddoel

De huidige NixOS-module gebruikt nog `configFiles`. Het einddoel is korter:

```nix
services.podman-agent-container = {
  enable = true;
  configRoot = ./containers;
};
```

Home Manager/rootless user Quadlet:

```nix
programs.podman-agent-container = {
  enable = true;
  configRoot = ./containers;
};
```

De map/TOML bepaalt dan wat actief, transient, persistent of promoteable is.

## Huidige merge-regels

Voor `parents` en `children` in de NixOS-module gelden nu:

- refs zijn relatief aan `configRoot` zonder `.toml`, bijvoorbeeld `"base/server"` -> `configRoot/base/server.toml`;
- `set` vervangt de lokale ref-lijst van de node;
- als `set` leeg is, gebruikt de node `add`;
- `remove` verwijdert refs uit die lokale lijst;
- parents worden vóór self gemerged;
- children worden na self gemerged;
- attrsets mergen recursief;
- lijsten concateneren met `lib.unique`;
- `config.runtime.command` is een override-list: child vervangt parent;
- scalars worden door latere lagen overschreven;
- parent cycles geven een fout.

Voorbeeld:

```toml
# base/server.toml
version = 1
name = "base/server"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive"]
command = ["bash", "-lc", "echo parent"]
```

```toml
# projects/app.toml
version = 1
name = "app"

[parents]
add = ["base/server"]

[deploy]
enable = true
target = "system"

[config.runtime]
packages = ["coreutils"]
command = ["bash", "-lc", "echo child"]
```

Effectieve runtime:

```toml
[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils"]
command = ["bash", "-lc", "echo child"]
```

## Package overrides

Base container:

```toml
version = 1
name = "base/server"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils", "appA", "appB", "appC"]
```

Project override:

```toml
version = 1
name = "projects/my-app"

[parents]
add = ["base/server"]

[config.runtime.packageOps]
remove = ["appB"]
add = ["appZ"]

[[config.runtime.packageOps.replace]]
name = "appC"
with = "appC_pinned"
```

Effectief:

```text
base packages
  - appB
  replace appC
  + appZ
```

## Nix store runtime

`rootfs-store` bouwt geen image. Nix realiseert alleen store closures.

```text
TOML packages
  -> Nix package refs
  -> runtime buildEnv/symlinkJoin
  -> Quadlet Rootfs= + PATH naar runtime/bin
```

Een package kan in `/nix/store` staan zonder geïnstalleerd te zijn in host PATH.

```text
package in /nix/store        ja
package in host PATH         nee
package in container PATH    ja
```

## Cache

Runtime closures kunnen uit een binary cache komen. Als store paths al bestaan, is starten/switchten snel. Als niet, downloadt of bouwt Nix alleen ontbrekende closures.

## Store access

Eerste praktische mode:

```toml
[config.runtime]
storeAccess = "full-readonly"
```

Quadlet mount dan conceptueel `/nix/store:/nix/store:ro`.

Latere strengere mode:

```toml
[config.runtime]
storeAccess = "closure-only"
```

Dan ziet de container alleen de benodigde closure paths. Dit is veiliger maar complexer en moet apart getest worden.

## Snelle CLI-flow

Geen NixOS rebuild:

```bash
pac up ./pac.toml
```

Flow:

```text
TOML
  -> resolve graph
  -> build/reuse runtime closure
  -> render transient Quadlet
  -> systemctl --user start
```

## Promoted flow

Als een dynamische container vaker gebruikt wordt:

```text
working config
  -> pac promote
  -> resolved/promoted TOML in infra repo
  -> jj branch / PR / merge
  -> NixOS/HM managed Quadlet
```

## Open implementatiepunten

- package refs naast simpele `pkgs.<name>` strings;
- duplicate name checks bestaan, maar kunnen rijkere foutmeldingen krijgen;
- Home Manager module.
