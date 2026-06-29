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

Resolve-volgorde:

```text
parents -> self -> children
```

Latere lagen mogen eerdere lagen overschrijven of uitbreiden volgens expliciete merge-regels.

## Geen entries als einddoel

De huidige NixOS-module gebruikt nog `configFiles`. Het einddoel is korter:

```nix
services.podman-agent-container = {
  enable = true;
  configRoot = ./containers;
};
```

Home Manager later:

```nix
programs.podman-agent-container = {
  enable = true;
  configRoot = ./containers;
};
```

De map/TOML bepaalt dan wat actief, transient, persistent of promoteable is.

## Package overrides

Base container:

```toml
version = 1
name = "base/server"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils", "appA", "appB", "appC"]
```

Project override, toekomstig model:

```toml
version = 1
name = "projects/my-app"

[parents]
add = ["base/server"]

[config.runtime.packages]
remove = ["appB"]
add = ["appZ"]

[[config.runtime.packages.replace]]
name = "appC"
source = "flake"
ref = "github:example/appC/specific-rev"
attr = "packages.${system}.default"
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

- graph resolver;
- merge-regels voor scalars/attrs/lists/package operations;
- duplicate container name detectie;
- package refs naast simpele `pkgs.<name>` strings;
- direct `pac up` closure build zonder NixOS rebuild;
- Home Manager module.
