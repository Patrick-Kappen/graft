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

No hidden policy — all security is explicit, composable TOML.

- [ ] **`examples/security/`** — copy/paste locked parent configs:
  - `locked.toml`, `readonly-rootfs.toml`, `no-network.toml`
  - `tmpfs-home.toml`, `proxy-egress.toml`, `rootless-keep-id.toml`, `agent-safe.toml`
- [ ] **Validation mode** — `validation.level = "strict"` with checks for:
  - dangerous mounts (writable `$HOME`, host `/`)
  - missing volume modes (`ro`/`rw`)
  - `privileged = true`, broad ports, disabled SELinux labels
  - network without egress policy
  - secrets with store paths
- [ ] **Quadlet `.network` rendering** — internal Podman networks for proxy sidecar pattern
- [ ] **Proxy sidecar examples** — `app → internal net → proxy → internet`, not just env vars
- [ ] **Secret docs + examples** — `podman secret create` + `[[config.secrets]]` TOML
- [ ] **Closure-only store access** — `storeAccess = "closure-only"` to limit container's view of `/nix/store`
- [ ] **Seccomp / AppArmor / SELinux examples** — profiles, not hidden defaults

---

## Phase 4 — Observability & supply-chain

Audit trails and automated update pipelines.

- [ ] **Effective config export** — `graft explain <instance>` shows resolved mounts/network/security
- [ ] **Proxy logs / egress audit** — which hostnames the container reached
- [ ] **SBOM / package list** — list runtime closure packages for an instance
- [ ] **Diff of effective config** — before/after a TOML change
- [ ] **Supply-chain update pipeline** — Renovate-like flow:
  ```text
  locked container
    + proxy-only egress
    + ephemeral HOME/XDG
    + candidate workspace
    + one update action
    + diff export
    + user review → promote
  ```
- [ ] **TUI** — interactive list of instances, logs, attach, promote

---

## Out of scope (for now)

- OCI image build or pull (no Dockerfile, no image registry required)
- Host PATH modifications
- Secrets stored in TOML or the Nix store
- Any hidden security defaults
