# CLI

De korte commandnaam is:

```bash
pac
```

De lange naam blijft beschikbaar:

```bash
podman-agent-container
```

## Up

Start een TOML-config direct via de transient Quadlet flow:

```bash
pac up ./config.toml
```

Zonder argument probeert `pac up` in de huidige directory automatisch een config te vinden:

```text
pac.toml
podman-agent-container.toml
.pac.toml
config.toml
```

Voor nu is `up` een korte alias rond de bestaande transient run-flow. Later wordt dit de snelle project-flow die de TOML graph resolved, runtime closure realiseert en de container start.
