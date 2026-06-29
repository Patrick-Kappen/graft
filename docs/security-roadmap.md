# Security roadmap

This document collects security work still needed to make `graft` a convenient way to build secure Podman/Nix containers. Nothing here should become hidden policy: secure behavior should be explicit, composable TOML.

## Principles

- No implicit security policy.
- Provide strong example parents/presets users can opt into.
- Validate dangerous combinations when users opt into strict validation.
- Keep fast Nix store-backed containers, but document the tradeoffs.
- Treat proxy env vars as convenience only; network topology must enforce egress.
- Real workspace and real home should not be writable for agent/update flows.
- Secret contents must never enter TOML or the Nix store.

## Security preset examples

Add copy/paste TOML parents under examples, not built-in hidden defaults:

```text
examples/security/
  locked.toml
  readonly-rootfs.toml
  no-network.toml
  tmpfs-home.toml
  proxy-egress.toml
  rootless-keep-id.toml
  agent-safe.toml
```

Example usage:

```toml
[parents]
add = ["security/locked", "security/no-network"]
```

## Validation mode

Future schema:

```toml
[validation]
level = "strict" # strict | warn | off
```

Potential strict checks:

- reject duplicate volume targets;
- reject host `/` mount unless explicitly allowed;
- reject or warn on writable host `$HOME` mounts;
- require explicit volume mode (`ro` or `rw`);
- warn/reject `privileged = true`;
- warn/reject `securityLabelDisable = true` in strict mode;
- warn if network is enabled without a proxy/egress policy;
- reject secret source/target patterns that point into `/nix/store`;
- warn on full `/nix/store` access in high-security profiles;
- warn on devices unless explicitly allowed;
- reject `AddCapability` when `dropCapabilities = ["all"]` unless explicitly intended;
- warn on `Network=host`;
- warn on broad published ports like `0.0.0.0:*`;
- reject duplicate container names earlier with better context;
- detect cycles with file/path context;
- validate raw Quadlet passthrough keys if strict mode is enabled.

## Network and proxy egress

Proxy variables are not enforcement. For real proxy-only egress:

```text
app container -> internal network -> proxy sidecar -> internet
app container -X-> internet direct
```

Needed:

- Quadlet `.network` rendering;
- internal Podman network examples;
- proxy sidecar examples;
- app containers attached only to internal network;
- proxy allowlist/logging/cache design;
- tests for generated network units and references.

Potential proxy implementations:

- tinyproxy;
- squid;
- custom minimal Go CONNECT proxy;
- npm-specific caching proxy;
- mitmproxy only as an explicit debugging mode with trust-root implications documented.

## Nix store access

Current mode:

```ini
Volume=/nix/store:/nix/store:ro
```

Pros:

- very fast;
- cache-friendly;
- no image build;
- simple rootfs-store runtime.

Cons:

- container can read all host store paths;
- bad if secrets were accidentally put in store.

Future:

```toml
[config.runtime]
storeAccess = "full-readonly" # current
storeAccess = "closure-only"  # future
```

Closure-only options to investigate:

- bind-mount every closure path read-only;
- generate a runtime closure view under `$XDG_RUNTIME_DIR`;
- use overlay/bind tricks;
- build OCI image for portable closed bundles later.

## Rootless/user hardening

For Home Manager/rootless containers, document and test:

- rootless Podman assumptions;
- `userns = "keep-id"`;
- explicit container user/group;
- temporary HOME for agents;
- no real host `$HOME` mount;
- lingering implications for autostart later.

## Seccomp/AppArmor/SELinux

Renderer fields exist, but examples are needed:

```toml
[config.security]
seccompProfile = "./seccomp-agent.json"
securityOpt = ["apparmor=graft-agent"]
securityLabelDisable = false
```

Need:

- example seccomp profile;
- AppArmor profile docs if available on host;
- SELinux label docs for systems using SELinux;
- validation warnings when labels are disabled.

## Secrets

Current renderer supports Quadlet `Secret=` references. Missing work:

- docs for `podman secret create`;
- examples for mounting secrets at `/run/secrets/...`;
- validation that secret content is not in TOML;
- later helpers for sops/agenix/pass, but runtime-only: do not copy secret bytes into store.

Example desired docs:

```bash
printf '%s' "$TOKEN" | podman secret create npm-token -
```

```toml
[[config.secrets]]
name = "npm-token"
target = "/run/secrets/npm-token"
type = "mount"
mode = "0400"
```

## Devices and capabilities

Devices and capabilities are high-risk.

Need:

- examples only for narrow cases (`/dev/fuse`, GPU, etc.);
- strict validation warnings;
- docs explaining implications;
- no implicit devices.

## Filesystem hardening

Need examples and checks for:

- read-only rootfs;
- tmpfs-only writable paths;
- no host root mount;
- no writable host home;
- explicit volume mode;
- optional masked/read-only paths if Quadlet/Podman supports them cleanly;
- ephemeral HOME/XDG for agents/update tools.

## Supply-chain/update flow

See also [`supply-chain-update-flow.md`](supply-chain-update-flow.md) for the broader Renovate-like cache/scan/review pipeline idea.

For NPM/Pi.dev/addons/agents:

```text
locked container
  + no direct egress or proxy-only egress
  + ephemeral HOME/XDG
  + candidate workspace/copy/jj workspace
  + one update/action
  + diff/export
  + user review
  + promote to repo truth
```

Still missing:

- configurable copy excludes;
- jj workspace candidate mode;
- patch export;
- apply/discard commands;
- promote command that writes TOML/update result to branch;
- TUI later.

## Observability/audit

Useful security features later:

- `graft render`/TUI showing effective Quadlet;
- effective config export;
- list mounts/network/security in human-readable form;
- proxy logs;
- update action metadata;
- generated SBOM/package list for runtime closure;
- diff of effective config before/after changes.

## Suggested implementation order

1. Add `examples/security/*` locked parents.
2. Add `validation.level` with strict checks for dangerous mounts/network/devices/secrets.
3. Add Quadlet `.network` rendering.
4. Add proxy sidecar example and tests.
5. Add secret docs/examples.
6. Add closure-only store access investigation/prototype.
7. Add jj/candidate workspace and promote workflow.
8. Add TUI/explain once core security/runtime model is solid.
