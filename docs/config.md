# graft config

By default `graft` does nothing with Podman, Quadlet, containers, or profiles.

For now the package only ships an example configuration:

```text
share/graft/config.example.toml
```

## Current template

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

The full template is in `config.example.toml`.

## No-op convention

Empty options mean: do nothing.

```toml
version = 1

[config]
```

Empty `parents` / `children` lists must not implicitly activate a container,
runtime, mount, profile, or Quadlet unit.

Users fill in only the parts they explicitly want to use.

## Network units

A config can render additional Quadlet `.network` units alongside its
`.container` unit:

```toml
[config.network]
mode = "graft-internal.network"

[[config.networks]]
name = "graft-internal"
driver = "bridge"
internal = true
ipv6 = false
subnet = "10.89.0.0/24"
gateway = "10.89.0.1"
options = ["mtu=1500"]

[config.networks.labels]
managed-by = "graft"
```

The generated units are:

```text
graft-internal.network
<container-name>.container
```

Use `config.network.mode = "<name>.network"` to attach the container to the
generated network.

## Service options

Service-level systemd options can be set explicitly:

```toml
[config.service]
type = "notify" # "oneshot" (default) or "notify"; Quadlet rejects other values
restart = "on-failure"
restartSec = "10s"
timeoutStartSec = "2m"
timeoutStopSec = "30s"
remainAfterExit = false
```

These render into the Quadlet-generated `[Service]` section. They do not yet
enable/start units automatically.

## More container/security fields

See [`security.md`](security.md) for explicit TOML fields covering:

- filesystem volumes/mounts/devices;
- security options/capabilities/seccomp/labels;
- network DNS/hosts/published ports;
- resources and ulimits;
- Podman secrets;
- raw Quadlet passthrough.
