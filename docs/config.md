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
