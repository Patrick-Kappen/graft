# Basiscontainer en update via PR

Historische notitie, nu onderdeel van [`vision.md`](vision.md): snelle containers kunnen later worden gepromoot naar TOML in een repo/branch/PR.

Doel: updates aan een runtime/profiel/agent gebeuren nooit direct op de echte omgeving. Een update draait in een tijdelijke container/candidate en het resultaat wordt als reviewbare wijziging aangeboden, bij voorkeur als PR.

## Basiscontainer

De basiscontainer is declaratief en minimaal:

```text
lege Podman rootfs
+ /nix/store read-only
+ flake .#minimal-runtime
+ expliciete mounts per mode
```

De basisruntime staat in `flake.nix` als:

```text
minimal-runtime = bashInteractive + coreutils
```

Later komt daar een tweede runtime naast voor Pi zelf, bijvoorbeeld:

```text
pi-runtime = bash + coreutils + node + pi + npm + jj + rg
```

Maar de container boundary blijft hetzelfde.

## Update-flow

Een Pi update of extension update gebeurt in een candidate:

```text
huidig profiel / lock
  ↓ kopie
candidate profiel
  ↓ tijdelijke update-container
pi update / npm install / pi install
  ↓
diff candidate vs huidig
  ↓
PR/review
  ↓
promote pas na akkoord
```

## Geen directe mutatie

Niet doen:

```bash
pi update --extensions
```

op de echte host of het echte profiel.

Wel doen:

```bash
podman-agent-container update-profile default -- pi update --extensions
```

of:

```bash
podman-agent-container update-runtime
```

Die commando's moeten altijd:

1. een tijdelijke/candidate directory maken;
2. de update in een container draaien;
3. het resultaat als diff tonen;
4. een branch/PR voorbereiden;
5. pas na review/promote actief maken.

## PR-model

Voor reproduceerbaarheid leggen we de output van een update vast in de repo/dotfiles, bijvoorbeeld:

```text
profiles/default/settings.json
profiles/default/package-lock.json
profiles/default/npm-snapshot/
flake.lock
```

Een update maakt een normale VCS-wijziging:

```text
update/pi-default-2026-06-29
```

Daarin staan alleen de expliciet gepromote changes.

## Eerste implementatiestappen

1. Houd `podman-agent-container-plain-shell` als minimale container smoke test.
2. Maak een Nix package/app `podman-agent-container` in plaats van losse scripts.
3. Voeg een `pi-runtime` toe naast `minimal-runtime`.
4. Maak `podman-agent-container update-runtime` voor `flake.lock` updates.
5. Maak `podman-agent-container update-profile <name>` voor Pi/npm/profile updates.
6. Laat beide flows alleen candidate-output produceren, nooit direct de actieve omgeving wijzigen.

## Belangrijk principe

De container mag mutable zijn tijdens de update. De echte omgeving niet.

```text
mutable in tmp/candidate
immutable/reviewed in repo
active pas na promote
```
