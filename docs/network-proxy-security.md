# Network, proxy and maximum isolation design

This document records the next security direction: focus first on a maximally locked-down container, then controlled egress via proxy/sidecar. Workspace/session flows are separate and can wait.

## Goal

Create containers that are as closed as possible by default **when the user explicitly selects a locked parent**, and allow network only through explicit, reviewable TOML.

Important principle remains:

```text
graft does not inject hidden policy.
users compose explicit locked/proxy parents in TOML.
```

## Proxy env is not a boundary

Environment variables like:

```toml
[config.container.environment]
HTTP_PROXY = "http://proxy:8080"
HTTPS_PROXY = "http://proxy:8080"
```

are useful for tools such as npm, curl, pip, Pi.dev, etc. But they are **not** a security boundary. A process can ignore them and open direct sockets if the network allows it.

For real isolation, egress control has to be network-level:

```text
application container cannot reach internet directly
application container can only reach proxy sidecar/internal proxy
proxy controls/logs/filters outbound traffic
```

## Strong locked parent

A user-authored locked parent can look like this:

```toml
version = 1
name = "base/locked"

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
privileged = false
securityLabelDisable = false

[config.resources]
pidsLimit = 512
memory = "1g"
```

This gives:

- read-only rootfs;
- tmpfs-only writable scratch paths;
- no network;
- dropped Linux capabilities;
- no-new-privileges;
- rootless/user namespace behavior where possible;
- resource limits.

This is the baseline for “no egress” jobs.

## Store access caveat

Current `rootfs-store` mode mounts the full host Nix store read-only:

```ini
Volume=/nix/store:/nix/store:ro
```

This is fast and cache-friendly, but broad. The container can read all host store paths. That is usually fine for public package closures, but secrets must never be put in the Nix store.

Future option:

```toml
[config.runtime]
storeAccess = "closure-only"
```

Possible implementation directions:

- many read-only bind mounts for closure store paths;
- generated closure view under runtime dir;
- later OCI/image mode for portable/closed bundles.

## Proxy-only egress model

The desired secure egress shape is:

```text
app container
  -> internal podman network only
  -> cannot reach internet directly
  -> can reach proxy sidecar

proxy container
  -> same internal network
  -> outbound internet allowed
  -> optional allowlist/logging/cache
```

Conceptually:

```text
[app] ---> [proxy] ---> internet
  X -----------------> internet directly
```

This requires first-class support for more Quadlet unit types, especially `.network`.

## Needed Quadlet unit support

Currently we mostly render `.container` files.

Needed next:

- `.network` units;
- probably `.volume` units later;
- maybe `.pod` units if useful for sidecars;
- dependency wiring between app/proxy/network units.

Example future TOML direction:

```toml
[[resources.networks]]
name = "graft-egress"
internal = true
```

App:

```toml
[config.network]
mode = "graft-egress"

[config.container.environment]
HTTP_PROXY = "http://graft-proxy:8080"
HTTPS_PROXY = "http://graft-proxy:8080"
NO_PROXY = "localhost,127.0.0.1"
```

Proxy sidecar:

```toml
version = 1
name = "graft-proxy"

[deploy]
enable = true
target = "user"

[config.network]
mode = "graft-egress"

[config.proxy]
enable = true
allowHosts = ["registry.npmjs.org", "github.com"]
log = true
```

The exact `[config.proxy]` schema is not decided yet.

## Proxy implementations to evaluate

Possible sidecar choices:

- tinyproxy;
- squid;
- mitmproxy if interception/debug is needed, but this has trust/cert implications;
- custom minimal Go HTTP CONNECT proxy with allowlist/logging;
- npm-specific caching proxy for npm workflows.

Important: TLS interception should not be implicit. If ever supported, it must be explicit because it changes trust roots.

## NPM / Pi.dev / agent update flow with proxy

For package/addon update tasks:

```text
locked app container
  + temporary HOME/XDG
  + candidate workspace/copy later
  + no direct internet
  + proxy-only egress
  + one action/update
  + diff/promote later
```

NPM example future shape:

```toml
[parents]
add = ["base/locked", "network/proxy-egress", "runtime/node"]

[config.container.environment]
HTTP_PROXY = "http://graft-proxy:8080"
HTTPS_PROXY = "http://graft-proxy:8080"
NPM_CONFIG_PROXY = "http://graft-proxy:8080"
NPM_CONFIG_HTTPS_PROXY = "http://graft-proxy:8080"
```

But again: env vars are convenience, not enforcement. Enforcement comes from network topology/firewall/proxy-only design.

## Validation ideas

Future validation can be explicit and configurable:

```toml
[validation]
level = "strict" # strict | warn | off
```

Potential strict checks:

- forbid `privileged = true` unless explicitly allowed;
- forbid host `/` mounts;
- require volume modes (`ro`/`rw`) to be explicit;
- warn on writable host home mounts;
- warn if deploy-enabled container has network and no proxy policy;
- detect duplicate volume targets;
- reject secrets that point into `/nix/store`;
- warn if full `/nix/store` access is used in high-security profile.

But validation must not become hidden policy. It should be explicit, reviewable, and overridable.

## Concrete next implementation steps

1. Add Quadlet `.network` rendering.
2. Add examples:
   - `base/locked.toml`;
   - `network/no-network.toml`;
   - `network/proxy-egress.toml`;
   - `proxy/tinyproxy.toml` or equivalent;
   - `apps/npm-update.toml` using proxy env.
3. Add tests for generated `.network` units and container network references.
4. Decide proxy sidecar implementation.
5. Add validation mode and dangerous mount/network checks.
6. Investigate closure-only `/nix/store` access.

## Current status

Implemented already:

- explicit security fields;
- explicit filesystem volumes/mounts/devices;
- explicit network mode, published ports, DNS, add-host;
- explicit resources/ulimits;
- explicit secrets references;
- raw Quadlet passthrough;
- locked examples in docs;
- transient copy/home primitives for update flows.

Not implemented yet:

- network unit rendering;
- proxy sidecar orchestration;
- proxy allowlist/cache/logging;
- closure-only store access;
- strict validation mode;
- promote/apply workflow.
