# graft config

> For the full field reference see [`reference.md`](reference.md) and the
> annotated [`examples/reference.toml`](../examples/reference.toml).

By default `graft` does nothing with Podman, Quadlet, containers, or profiles.

## No-op convention

Empty options mean: do nothing.

```toml
version = 1

[config]
```

Empty `parents` / `children` lists must not implicitly activate a container,
runtime, mount, profile, or Quadlet unit. Users fill in only the parts they
explicitly want to use.

Run `graft config init` to write the example no-op template to
`~/.config/graft/config.toml`, or `graft config show` to print it.

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

## Home and workspace isolation

graft can isolate the container's home directory and give extra writable paths
to the container without touching the host directly.

```toml
[config.home]
mode   = "session"                          # ephemeral | persistent | session
source = "~/.local/share/graft/my-app"      # seeded on start (session/persistent)
target = "/home/user"                        # mount point inside the container
review = "diff"                             # print diff before promote
promote = "prompt"                          # auto | prompt | never

# Shadow mounts: extra writable paths backed by per-session copies.
# Use `graft diff` to review changes, `graft promote` to push back to host.
[[config.home.shadow]]
container = "/workspace"
host      = "~/projects/my-app"
```

| Mode         | Behaviour |
| ------------ | --------- |
| `ephemeral`  | Temp dir, wiped after each run. Maximum isolation. |
| `persistent` | Host dir bind-mounted; survives across runs. |
| `session`    | Host dir copied to a temp dir on start; changes reviewed/promoted on stop. |

**Shadow mounts** (`[[config.home.shadow]]`) add extra writable paths isolated
per session. The agent writes to the container path; after the run:

```bash
graft diff my-app-1      # review what changed
graft promote my-app-1   # copy changes back to host path
graft reset my-app-1     # clear session data, start fresh next run
```

## Attach behaviour

```toml
[config.attach]
tmuxSession = "main"   # tmux session name (default: "main")
shell       = "sh"     # fallback when tmux is not in the container
startDelay  = "500ms"  # wait after `graft up` before attaching
```

tmux is optional. If it is not in the container, graft falls back to the
configured shell.

## Service options

Service-level systemd options can be set explicitly:

```toml
[config.service]
type = "notify"          # "oneshot" (default) or "notify"
restart = "on-failure"
restartSec = "10s"
timeoutStartSec = "2m"
timeoutStopSec = "30s"
remainAfterExit = false
restartIfChanged = false # set to false to keep container alive during nixos-rebuild
```

These render into the Quadlet-generated `[Service]` section. `restartIfChanged`
is a NixOS-specific option: setting it to `false` prevents `nixos-rebuild switch`
from restarting a running container when its unit file changes.

## More container/security fields

See [`security.md`](security.md) and [`reference.md`](reference.md) for
explicit TOML fields covering:

- filesystem volumes/mounts/devices;
- security options/capabilities/seccomp/labels;
- network DNS/hosts/published ports;
- resources and ulimits;
- Podman secrets;
- raw Quadlet passthrough.
