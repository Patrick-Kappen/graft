# Roadmap

> **Status: early beta — v0.1.0.**
> Core features work. The TOML schema and CLI surface may change before v1.0.
> See [vision.md](vision.md) for the full product direction.

This roadmap is grouped by phase. Items without a checkbox are not yet started.

---

## Phase 0 — Foundation ✅ (current, v0.1.x)

Everything in this phase works today.

- [x] Go CLI binary (`graft`)
- [x] Strict TOML loader (unknown fields rejected)
- [x] No-op detection (empty config does nothing)
- [x] `rootfs-store` Quadlet rendering
- [x] NixOS module — recursive `configRoot` discovery, graph resolution, `packageOps`
- [x] Home Manager module — same resolver, rootless user Quadlet
- [x] `parents.*` / `children.*` TOML graph resolution
- [x] Managed-path instance operations: `up`, `down`, `attach`, `list`, `logs`
- [x] Dev-path: `graft run <file.toml> --as <name>` (transient unit)
- [x] Plumbing: `inspect`, `render`, `render-nixos`, `render-nixos-units`
- [x] `--host` flag for remote operations
- [x] Home session isolation, shadow mounts, diff/promote/reset skeleton
- [x] Examples, docs, LICENSE, CONTRIBUTING, SECURITY

---

## Phase 1 — Developer experience

Make the day-to-day dev flow smooth before adding complexity.

- [ ] **Worktree auto-naming** — derive instance name from git/jj worktree for `graft run`
- [ ] **Package refs beyond `pkgs.<name>`** — flake refs, overlay pins, store paths
- [ ] **`graft config` improvements** — `diff`, `effective`, `explain` subcommands
- [ ] **Shell hook** — `eval "$(graft hook zsh|bash|fish)"` for enter/leave on `cd`
- [ ] **`graft enter` / `graft leave` / `graft status`** — manual session lifecycle
- [ ] **Autodetect TOML** — `graft run` without an argument tries `graft.toml`, `.graft.toml`, `config.toml`
- [ ] **Idle policy** — configurable timeout and leave-action in TOML:

  ```toml
  [session]
  mode = "ephemeral"     # ephemeral | persistent | hybrid
  idleTimeout = "30m"
  leaveAction = "review" # review | keep | discard | stop
  ```

---

## Phase 2 — Workspace isolation & promotion

Safe workspace mutations and a path from experiment to repo truth.

- [ ] **Candidate workspace copy** — real workspace is never directly writable in the container
- [ ] **jj workspace candidate mode** — `mode = "jj"` creates a jj workspace for the container
- [ ] **`graft review`** — export diff/patch after container leaves
- [ ] **`graft apply` / `graft discard`** — accept or throw away workspace changes
- [ ] **`graft promote`** — write result TOML to a branch/PR in the infra repo
- [ ] **Persistent user Quadlet mode** — autostart on login via lingering

  ```toml
  [workspace]
  mode = "jj"       # jj | copy | none
  target = "/workspace"
  review = "patch"  # patch | jj-change
  ```

---

## Phase 3 — Security hardening

No hidden policy — all security is explicit, composable TOML. Secure presets are
opt-in parents, not invisible defaults.

### 3a — Presets & validation

- [ ] **`examples/security/`** — copy/paste locked parent configs:
  - `locked.toml` — read-only rootfs, no network, drop all caps, no new privs
  - `readonly-rootfs.toml` — filesystem hardening only
  - `no-network.toml` — network isolation only
  - `tmpfs-home.toml` — ephemeral HOME/XDG for agents
  - `proxy-egress.toml` — egress via proxy sidecar only
  - `rootless-keep-id.toml` — rootless user namespace mapping
  - `agent-safe.toml` — combined preset for AI/automation agents:
    - proxy-only egress, ephemeral HOME/XDG, read-only rootfs,
      candidate workspace, PID + memory limits, no capabilities
- [ ] **Validation mode** — `validation.level = "strict"` with checks for:
  - writable `$HOME` or host `/` mounts
  - missing volume modes (`ro`/`rw`)
  - `privileged = true` or broad published ports (`0.0.0.0:*`)
  - disabled SELinux / AppArmor labels
  - network enabled without egress policy
  - secret content pointing into `/nix/store`
  - `AddCapability` combined with `dropCapabilities = ["all"]`
  - `Network=host`
  - duplicate volume targets or container names
  - graph cycles (with file/path context in error)

### 3b — Secret management

Secrets must never enter TOML, the Nix store, or rendered Quadlet files.
All injection happens at runtime only.

- [ ] **systemd credentials integration** — use Quadlet `LoadCredential=` /
  `SetCredential=` so secrets live in-memory at `/run/credentials/<unit>/`
  and are never on disk:

  ```toml
  [[config.secrets]]
  name = "api-token"
  source = "credential:api-token"   # loaded via systemd credential
  target = "/run/secrets/api-token"
  mode = "0400"
  ```

- [ ] **sops / sops-nix integration** — NixOS module can reference sops-managed
  secrets; decrypted at activation time, injected via systemd credentials or
  tmpfs mount
- [ ] **agenix / age integration** — same pattern for agenix users; age-encrypted
  secrets decrypted at activation, never in the Nix store
- [ ] **pass / gopass helper** — fetch secret from pass/gopass at container start,
  inject via tmpfs:

  ```toml
  [[config.secrets]]
  name = "npm-token"
  source = "pass:services/npm/token"
  target = "/run/secrets/npm-token"
  type = "tmpfs-mount"
  mode = "0400"
  ```

- [ ] **`podman secret` docs + examples** — complete guide for
  `podman secret create` + `[[config.secrets]]` TOML
- [ ] **Secret scoping** — secrets are not inherited through the parent graph
  by default; children must explicitly opt in
- [ ] **Secret TTL / rotation hooks** — re-inject or restart container when a
  secret is rotated
- [ ] **Anti-leak validation** — detect if a known secret pattern appears in
  rendered TOML, Quadlet output, or `graft inspect` output
- [ ] **Secret access audit** — log which secrets were accessed and when
  (via systemd journal)
- [ ] **External secret store interface** — generic adapter for HashiCorp Vault,
  Doppler, Infisical, etc.:

  ```toml
  [[config.secrets]]
  name = "db-password"
  source = "vault:secret/data/db#password"
  target = "/run/secrets/db-password"
  ```

### 3c — Namespace & runtime hardening

- [ ] **`noNewPrivileges = true` as a documented recommended default** — validated
  preset and strict-mode requirement
- [ ] **PID namespace isolation** — `--pid=private` so the container cannot see
  host processes
- [ ] **IPC namespace isolation** — no shared memory with host or other containers
  by default
- [ ] **UTS namespace isolation** — prevent hostname spoofing
- [ ] **dbus isolation** — agent containers must not access the host dbus socket
- [ ] **Ephemeral XDG_RUNTIME_DIR** — agents get a separate tmpfs runtime dir,
  not the host's `/run/user/<uid>`
- [ ] **Masked paths** — mask dangerous kernel interfaces by default in strict mode:
  `/proc/kcore`, `/proc/sysrq-trigger`, `/sys/firmware`, `/sys/kernel/debug`
- [ ] **Read-only `/proc` and `/sys` subtrees** — reduce kernel attack surface
  for agent containers
- [ ] **Resource limits as security** — CPU, memory, and PID limits to prevent
  fork bombs and resource exhaustion:

  ```toml
  [config.resources]
  memoryLimit = "512m"
  cpuQuota = "50%"
  pidsLimit = 64
  ```

- [ ] **Capability audit** — `graft capabilities <instance>` shows granted,
  dropped, and actually-used capabilities
- [ ] **Seccomp profile helpers** — common named profiles (`server`, `network-client`,
  `file-processor`) and a tool to suggest minimal syscall set from the package
  closure:

  ```toml
  [config.security]
  seccompProfile = "graft:network-client"  # built-in named profile
  # or
  seccompProfile = "./seccomp-agent.json"  # custom profile
  ```

- [ ] **AppArmor / SELinux examples** — profile docs for systems that use them;
  validation warnings when labels are disabled in strict mode
- [ ] **Rootless hardening guide** — complete docs for rootless Podman security:
  `keep-id`, `newuidmap`/`newgidmap`, `subuid`/`subgid`, user namespace mapping

### 3d — Network security

- [ ] **Quadlet `.network` rendering** — generate Podman network units from TOML
- [ ] **Proxy sidecar pattern** — `app → internal net → proxy → internet`,
  not just environment variables:

  ```toml
  [config.network]
  mode = "internal"           # no direct internet
  proxy = "http-proxy-1"      # name of a sibling graft container
  ```

- [ ] **DNS filtering** — restrict which hostnames the container can resolve;
  split-horizon DNS for internal services
- [ ] **Port exposure auditing** — warn on `0.0.0.0` binds or broad port ranges
  in strict mode
- [ ] **Container-to-container isolation** — graft-managed containers must not
  reach each other on a shared Podman network unless explicitly configured
- [ ] **Egress allowlist** — declare permitted outbound hosts/ports in TOML;
  proxy enforces it:

  ```toml
  [config.network.egress]
  allow = ["registry.npmjs.org:443", "api.github.com:443"]
  ```

- [ ] **Proxy logs / egress audit** — `graft logs --denied <instance>` shows
  blocked egress attempts (already in CLI surface, needs proxy backend)

### 3e — Supply chain integrity

- [ ] **Nix store hash verification** — verify store path hashes before container
  start; fail fast on corruption
- [ ] **Binary cache trust docs** — required signatures, trusted substituters,
  how to lock down `nix.settings.trusted-substituters`
- [ ] **Reproducible build verification** — compare store hashes across machines
  and cache hits; surface mismatches in `graft inspect`
- [ ] **Closure-only store access** — `storeAccess = "closure-only"` limits the
  container's `/nix/store` view to only the runtime closure paths:

  ```toml
  [config.runtime]
  storeAccess = "full-readonly"  # current default
  # storeAccess = "closure-only" # future: only runtime closure visible
  ```

- [ ] **SLSA provenance** — generate provenance metadata for graft-rendered
  containers (useful for audit and compliance)
- [ ] **Image signing** — for future OCI mode: verify signatures via
  sigstore/cosign before use

---

## Phase 4 — Observability & supply-chain automation

Audit trails and automated update pipelines.

- [ ] **Effective config export** — `graft explain <instance>` shows resolved
  mounts, network, security, and secrets (without secret values)
- [ ] **SBOM / package list** — list runtime closure packages for an instance
- [ ] **Diff of effective config** — before/after a TOML change
- [ ] **Audit log** — systemd journal structured events for container lifecycle:
  start, stop, secret access, network connections, workspace changes
- [ ] **Supply-chain update pipeline** — Renovate-like flow:

  ```text
  locked container (agent-safe preset)
    + proxy-only egress with allowlist
    + ephemeral HOME/XDG
    + candidate workspace (jj or copy)
    + one update action (npm update, go get, etc.)
    + diff/patch export
    + user review → apply or discard
    → promote to infra repo branch/PR
  ```

- [ ] **TUI** — interactive list of instances with logs, attach, promote, and
  effective config view

---

## Out of scope (for now)

- OCI image build or pull (no Dockerfile, no image registry required)
- Host PATH modifications
- Secrets stored in TOML or the Nix store
- Any hidden security defaults
- mitmproxy as a default debugging tool (trust-root implications must be
  explicitly acknowledged by the user)
