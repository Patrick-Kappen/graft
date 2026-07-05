# Security and isolation notes

For the broader hardening checklist, see [`security-roadmap.md`](security-roadmap.md).

`graft` does not enable hidden security policy. Security is explicit TOML.

## Locked-down parent example

```toml
version = 1
name = "base/locked"

[config.filesystem]
readOnly = true
readOnlyTmpfs = true
tmpfs = ["/tmp", "/run"]

[config.network]
mode = "none"

[config.security]
dropCapabilities = ["all"]
noNewPrivileges = true
userns = "keep-id"
```

Use it from an app:

```toml
[parents]
add = ["base/locked"]
```

## Supported security-related fields

```toml
[config.security]
dropCapabilities = ["all"]
addCapabilities = []
noNewPrivileges = true
privileged = false
seccompProfile = "/path/to/seccomp.json"
securityLabelDisable = true
securityOpt = ["apparmor=unconfined"]
userns = "keep-id"
```

Filesystem isolation:

```toml
[config.filesystem]
readOnly = true
readOnlyTmpfs = true
tmpfs = ["/tmp", "/run"]
mounts = ["type=bind,src=/cache,dst=/cache,ro=true"]

[[config.filesystem.volumes]]
source = "/host/path"
target = "/container/path"
mode = "ro"

[[config.filesystem.devices]]
source = "/dev/fuse"
target = "/dev/fuse"
permissions = "rwm"
```

Network:

```toml
[config.network]
mode = "none"
publish = ["127.0.0.1:8080:8080"]
dns = ["1.1.1.1"]
addHost = ["host.containers.internal:host-gateway"]
```

Resources:

```toml
[config.resources]
memory = "1g"
memorySwap = "2g"
cpus = "2"
cpuQuota = "50%"
pidsLimit = 512
ulimits = ["nofile=1024:2048"]
```

Secrets are declared as Podman/Quadlet secret references. Secret contents must not be placed in Nix store TOML.

```toml
[[config.secrets]]
name = "api-token"
target = "/run/secrets/api-token"
type = "mount"
mode = "0400"
```

## Raw Quadlet passthrough

For options not modeled yet, use explicit passthrough:

```toml
[config.quadlet.container]
Label = ["com.example.test=1"]

[config.quadlet.service]
Environment = ["FROM_SERVICE=1"]

[config.quadlet.install]
WantedBy = ["default.target"]
```

Passthrough is intentionally explicit and should be reviewed carefully.

## Ephemeral agent/update isolation

For agents and update tools that mutate random files, use explicit transient isolation:

```toml
[config.workspace]
mode = "copy"
source = "."
target = "/workspace"
review = "diff"

[config.home]
ephemeral = true
target = "/home/user"
```

This makes `graft up` mount a writable copy of the workspace and a temporary
HOME/XDG tree. The real workspace and real home are not mounted writable by
this flow. See [`config.md`](config.md) for home and workspace isolation options.

## Store access

The current rootfs-store mode mounts `/nix/store` read-only for speed:

```text
Volume=/nix/store:/nix/store:ro
```

This is fast and cache-friendly, but the container can read all host store paths. Do not put secrets in the Nix store. A future `closure-only` store access mode can reduce this exposure.
