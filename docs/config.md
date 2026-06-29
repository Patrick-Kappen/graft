# podman-agent-container config

`podman-agent-container` doet standaard niets met Podman, Quadlet, containers of profielen.

Voor nu levert het package alleen een voorbeeldconfiguratie:

```text
share/podman-agent-container/config.example.toml
```

## Huidige template

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

De volledige template staat in `config.example.toml`.

## No-op conventie

Lege opties betekenen: niets doen.

```toml
version = 1

[config]
```

of lege `parents`/`children`-lijsten mogen geen container, runtime, mount, profiel of Quadlet unit impliciet activeren.

Gebruikers vullen later alleen de delen in die ze expliciet willen gebruiken.

## Service options

Service-level systemd options can be set explicitly:

```toml
[config.service]
type = "simple"
restart = "on-failure"
restartSec = "10s"
timeoutStartSec = "2m"
timeoutStopSec = "30s"
remainAfterExit = false
```

These render into the Quadlet-generated `[Service]` section. They do not yet enable/start units automatically.

## More container/security fields

See [`security.md`](security.md) for explicit TOML fields covering:

- filesystem volumes/mounts/devices;
- security options/capabilities/seccomp/labels;
- network DNS/hosts/published ports;
- resources and ulimits;
- Podman secrets;
- raw Quadlet passthrough.
