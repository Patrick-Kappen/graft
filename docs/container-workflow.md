# Container-first agent workflow

Historische notitie, nu gegeneraliseerd in [`vision.md`](vision.md): `pac` moet direnv voor containers worden. Pi is hier slechts één mogelijke agent/app in zo'n container.

Het doel is niet om Pi, npm en extensions volledig declaratief te maken, maar om alle mutatie op te sluiten en pas na review naar de echte workspace te promoten.

## Doel

```text
host blijft schoon
Pi draait in container
de agent werkt in een tijdelijke workspace-kopie
wij reviewen het resultaat
pas na akkoord wordt er gemerged naar de echte workspace
```

## Basisidee

Een normale Pi-run doet straks dit:

1. Maak een tijdelijke run-directory.
2. Maak daarin een letterlijke kopie of jj-workspace van de huidige repo.
3. Start Pi in een locked container.
4. Mount alleen de tijdelijke workspace writable.
5. Laat de agent werken.
6. Na exit/signaal: toon diff/status.
7. Merge/apply alleen na expliciete goedkeuring.

Conceptueel:

```text
echte workspace
  ↓ snapshot / tijdelijke jj workspace
/tmp/pi-run-xxxx/workspace
  ↓ container mount als /workspace
Pi agent wijzigt alleen /workspace
  ↓ review
apply/merge naar echte workspace na akkoord
```

## Container boundaries

De container moet standaard zo beperkt mogelijk zijn:

- rootless Podman/Docker
- read-only base filesystem
- alleen expliciete mounts
- geen host `$HOME` mount
- geen automatische `~/.ssh`, `~/.config`, `~/.npm` of `~/.pi`
- tijdelijke writable `/home/pi`
- netwerk optioneel per modus

Voorbeeldrichting:

```bash
podman run --rm -it \
  --read-only \
  --cap-drop=ALL \
  --security-opt=no-new-privileges \
  --tmpfs /tmp \
  --tmpfs /run \
  -v "$TMP_HOME:/home/pi:rw" \
  -v "$TMP_WORKSPACE:/workspace:rw" \
  -w /workspace \
  pi-agent:locked \
  pi
```

## Profielen en updates

Pi/npm/extensions mogen alleen persistent muteren in een expliciete update-flow.

Normale run:

```text
locked image + locked profiel + tijdelijke home/workspace
```

Update run:

```text
locked image + kopie van profiel + npm/pi update
  ↓
review profielwijziging
  ↓
promote naar nieuw profiel pas na akkoord
```

Gewenste CLI-vorm:

```bash
podman-agent-container run default
podman-agent-container diff
podman-agent-container apply
podman-agent-container discard

podman-agent-container update-profile default -- pi update --extensions
podman-agent-container install-profile default npm:@some/extension
```

## Merge naar echte workspace

De agent mag nooit direct in de echte workspace schrijven. Na afloop maakt de wrapper een reviewbare wijziging.

Met jj is de voorkeursrichting:

- maak een aparte tijdelijke jj workspace/change
- laat de agent daarin werken
- toon `jj status`/diff vanuit de tijdelijke workspace
- merge/squash/apply naar de echte workspace alleen na akkoord

Als jj niet beschikbaar is, kan de wrapper terugvallen op een directory snapshot + patch/diff.

## Reproduceerbaarheid

De reproduceerbare unit wordt:

```text
container image digest
+ Pi profiel snapshot
+ echte repo revision
+ reviewed agent patch/change
```

Dus npm hoeft niet vertrouwd of volledig begrepen te worden. Alles wat npm doet gebeurt binnen de container/profiel-update-flow en wordt pas onderdeel van de vaste omgeving na review.

## Niet-doelen

Voorlopig niet proberen om:

- `~/.pi/agent` read-only uit Nix te maken
- npm install/update in een Nix build te draaien
- Pi extensions te verbieden om te schrijven
- de agent direct in de echte checkout te laten werken

De juiste richting is: mutatie toestaan, maar alleen binnen een sandbox en met expliciete promotie.
