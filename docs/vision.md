# Visie: pac als direnv voor containers

`pac` (`podman-agent-container`) moet uiteindelijk voelen als **direnv voor containers**.

Niet:

```text
cd project -> host shell krijgt extra PATH/env
```

maar:

```text
cd project -> project container wordt gestart/gebruikt
leave/idle -> container wordt gestopt/blijft idle volgens policy
changes -> uit candidate workspace gehaald en ter review klaargezet
```

De tool levert de schil en orchestration. Gebruikers bepalen zelf beleid, presets, mounts, netwerk, security en packages via TOML.

## Kernwaarde

Het gat dat dit project vult:

```text
Nix store backed runtime closures
+ Podman/Quadlet lifecycle
+ container-only tools
+ TOML graph/compositie
+ snelle dynamische projectcontainers
+ optionele promotie naar permanente config
```

Zonder verplicht:

- Dockerfile;
- OCI image build;
- image pull;
- host PATH vervuiling;
- handgeschreven Quadlet units;
- NixOS rebuild voor snelle/projectmatige runs.

## Productdefinitie

```text
pac = direnv voor Podman/Quadlet containers, backed by Nix store closures
```

Bij een project:

```text
project repo
  pac.toml / podman-agent-container.toml / override.toml
  flake.nix / flake.lock optioneel
```

`pac` kan dan:

1. TOML autodetecteren;
2. parent/child graph resolven;
3. runtime packages realiseren in `/nix/store`;
4. een minimale rootfs-store container renderen;
5. Quadlet transient of persistent starten;
6. workspace isoleren via copy/jj candidate;
7. bij leave/idle wijzigingen verzamelen voor review;
8. later een werkende configuratie promoten naar een repo/branch/PR.

## Belangrijk principe

Leeg betekent niets.

```toml
version = 1
name = "example"

[config]
# no-op
```

Geen impliciete container, mount, network, security policy, package of agent.

## TOML is bron van waarheid

TOML zet uiteindelijk alles:

- containernaam;
- runtime mode;
- packages;
- command;
- mounts;
- filesystem flags;
- network;
- security;
- resources;
- session/idle policy;
- parent/child relaties;
- deploy/promote metadata.

NixOS/Home Manager moeten kort blijven en alleen naar een map wijzen:

```nix
services.podman-agent-container = {
  enable = true;
  configRoot = ./containers;
};
```

Een TOML uit `configRoot` wordt NixOS-managed als hij expliciet deploy aanzet:

```toml
[deploy]
enable = true
target = "system"
```

Of voor Home Manager later:

```nix
programs.podman-agent-container = {
  enable = true;
  configRoot = ./containers;
};
```

De inhoud staat in TOML, niet in Nix options.

## Nix store runtime, niet host install

Packages hoeven niet geïnstalleerd te zijn op de host PATH.

```text
package in /nix/store        ja
package in host PATH         nee
package in container PATH    ja
```

Voorbeeld:

```toml
[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils", "go", "gopls"]
command = ["bash", "-lc", "go test ./..."]
```

Nix realiseert de package closures in `/nix/store`. De container krijgt een runtime env in PATH, maar de host shell niet.

## Geen image build nodig voor snelle flow

Voor `rootfs-store` wordt geen container image gebouwd.

Wat wel ontstaat:

```text
/nix/store/...-runtime
/nix/store/...-minimal-rootfs
/generated Quadlet .container
```

Als store paths al bestaan of uit binary cache komen, is dit snel. Als niet, bouwt/downloadt Nix alleen ontbrekende closures.

Later kan OCI/image mode alsnog bestaan voor distributie of niet-Nix hosts.

## Snel versus permanent

Er zijn twee hoofdroutes.

### Snelle projectflow

Geen NixOS rebuild.

```bash
pac up ./pac.toml
```

Of autodetect:

```bash
pac up
```

Flow:

```text
TOML in project
  -> resolve/build runtime closure
  -> temporary Quadlet in $XDG_RUNTIME_DIR/containers/systemd
  -> systemctl --user start
  -> container draait
```

### Permanente/promoted flow

Voor containers die je vaker gebruikt:

```text
effective/project config
  -> pac promote
  -> nieuwe TOML in infra/NixOS/Home Manager repo
  -> jj branch / PR / review / merge
  -> NixOS/HM managed Quadlet
```

Dus snel experimenteren blijft transient. Permanent maken gebeurt via reviewbare repo-wijzigingen.

## Parent/child en overrides

Doel is composable containers.

Base container:

```toml
# base/server.toml
version = 1
name = "base/server"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils", "appA", "appB", "appC"]
```

Project override:

```toml
# projects/my-app.toml
version = 1
name = "my-app"

[parents]
add = ["base/server"]

[config.runtime.packageOps]
remove = ["appX"]
add = ["appZ"]

[[config.runtime.packageOps.replace]]
name = "appY"
with = "appY_pinned"
```

Het project hoeft niet de hele base container te kopiëren. Het kan parent aanroepen en alleen verschillen declareren.

## Package operations en pins

Huidig package operation model:

```toml
[config.runtime.packageOps]
remove = ["appX"]
add = ["appZ"]

[[config.runtime.packageOps.replace]]
name = "appY"
with = "appY_pinned"
```

Doel:

```text
base packages
  - appX
  replace appY
  + appZ
```

Version pinning kan via flake locks/refs. Store paths hoeven alleen gerealiseerd te worden; ze hoeven niet in host profiles te staan.

## Candidate workspace voor agents

Voor agents of veilige projectmutaties:

```text
echte workspace
  -> candidate copy / jj workspace
  -> container krijgt candidate writable
  -> agent werkt daar
  -> leave/idle exporteert diff/change
  -> review/apply/discard
```

Regel:

```text
echte workspace nooit automatisch writable in agent-container
```

Mogelijke workspace modes:

```toml
[workspace]
mode = "jj"       # jj | copy | none
target = "/workspace"
review = "patch" # patch | jj-change
```

## Session lifecycle

Later moet `pac` sessies beheren.

Handmatig eerst:

```bash
pac enter
pac leave
pac status
pac review
pac apply
pac discard
```

Daarna shell hook:

```bash
eval "$(pac hook zsh)"
```

Gedrag:

```text
enter directory with pac.toml -> start/reuse container
leave directory              -> stop/keep/review according to policy
idle timeout                 -> stop/keep/review according to policy
```

TOML richting:

```toml
[session]
mode = "ephemeral"      # ephemeral | persistent | hybrid
idleTimeout = "30m"
leaveAction = "review" # review | keep | discard | stop
```

Persistent mode kan containers langer idle laten draaien.

## Security model

Het project levert geen verborgen policy. Wel kunnen docs suggesties geven.

Gebruiker kan zelf een locked parent maken:

```toml
[config.filesystem]
readOnly = true
readOnlyTmpfs = true
tmpfs = ["/tmp", "/run", "/home/agent"]

[config.network]
mode = "none"

[config.security]
dropCapabilities = ["all"]
noNewPrivileges = true
userns = "keep-id"
```

Voor snelheid mount de eerste versie vaak heel `/nix/store` read-only. Later kan `closure-only` store access worden onderzocht.

```toml
[config.runtime]
storeAccess = "full-readonly" # later: closure-only
```

## Directories en autodetect

`pac up` zonder argument probeert in huidige directory:

```text
pac.toml
podman-agent-container.toml
.pac.toml
config.toml
```

De NixOS-module kan inmiddels `parents.*` en `children.*` vanuit `configRoot` resolven.

## Huidige implementatiestatus

Nu aanwezig:

- Go CLI;
- korte binary `pac`;
- TOML loader met strict unknown-field checks;
- `inspect`, `render`, `render-nixos`, `run`, `up`;
- no-op detectie;
- rootfs-store Quadlet renderer;
- transient `systemctl --user` run;
- Nix package build;
- NixOS module met `configFiles`, recursive `configRoot` discovery en `parents.*`/`children.*` resolving;
- effective TOML generatie tijdens NixOS build;
- TOML `runtime.packages` -> `pkgs.<name>` in NixOS module;
- examples en docs.

Nog te bouwen:

- package refs beyond simple `pkgs.<name>` strings;
- session state;
- workspace copy/jj candidate flow;
- promote branch/PR flow;
- Home Manager module;
- persistent user Quadlet mode;
- idle/leave lifecycle.
