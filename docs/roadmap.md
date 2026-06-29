# Roadmap & launch plan

Strategic roadmap for open-sourcing `graft` (formerly `podman-agent-container` /
`pac`). This document is about **sequencing, scope boundaries, and the launch
narrative** — not code details. See [`vision.md`](vision.md) for the higher goal
and [`security-roadmap.md`](security-roadmap.md) /
[`supply-chain-update-flow.md`](supply-chain-update-flow.md) for the deeper
sub-plans.

---

## The vision in one line

> **`graft` = direnv for containers, backed by the Nix store** — project-local,
> image-less, TOML-declarative Podman/Quadlet containers, with safe
> agent/update mutation and a bridge to permanent NixOS/HM config.

The deeper "why": **controlled, reviewable mutation without polluting the host
or the repo.** The container is the means; reviewable mutation is the goal.

---

## The full scope = one core, multiple frontends

The vision is a shared core plus chunks that are each a project in their own
right:

| # | Sub-product | Layer | Status |
|---|---|---|---|
| a | Image-less Nix runtime + Quadlet renderer (rootfs-store) | core | working (vertical slice) |
| b | direnv lifecycle (enter/leave/idle, project-local `graft.toml`) | frontend (CLI) | designed |
| c | Agent/update sandbox with candidate → diff → promote | frontend (CLI) | primitive works |
| d | Renovate-like supply-chain pipeline (SBOM/scan/N-x/PR) | pipeline (CI) | designed |
| e | Declarative image building (`Image=`/OCI, digest pins) | core | designed |
| f | Nix-native authoring next to TOML | frontend (authoring) | working (`containers.<name>`) |
| g | Management dashboard (multi-host, persistent containers) | frontend (UI) | new |

**Two personas, one core:**

- **Developer / agent-runner** — ephemeral, project-local, sandbox. CLI-first. (b, c)
- **Homelab / infra-operator** — persistent containers, multiple hosts, needs an
  overview. Declarative + dashboard. (g)

Both share the same resolution engine, TOML/Nix schema, and Quadlet/image
renderer. For the MVP, pick **one persona** as the main story.

**Key insight:** the market gap is clear enough; the real risk is **scope,
sequencing, and trust**, not differentiation. The docs are roadmap-complete —
the failure risk is *execution breadth* ("eternally pre-1.0").

---

## Four architectural anchors (lock down before scope grows)

These four decisions determine whether the whole stays coherent.

### 1. One resolution engine as the backbone

Graph/merge/`packageOps` resolution must be **one shared implementation** that
feeds all render paths (rootfs-CLI, rootfs-NixOS, image). Today the logic lives
only in `nix/lib/eval-entries.nix`; the Go CLI does not resolve. With image mode
added, 3-way drift looms.

For a tool whose pitch is *"what you review is what runs"*, drift is fatal to
trust. A single source of truth for resolution is therefore almost a launch
requirement.

### 2. Hard boundary: `graft` (binary) vs. pipeline (CI)

```text
graft (small, auditable)               pipeline (thin, on top)
-----------------------------          -----------------------------
load/resolve/validate TOML             N-x delay window
effective-config export                cache candidates (attic/cachix/...)
render Quadlet/image                   SBOM/vuln/malware/license scan
candidate run + patch export           collect release notes
closure listing / SBOM hook            generate/update PR
                                       grouping/scheduling
```

The supply-chain flow is CI orchestration, not a container tool. Baking it into
`graft` would swell both the binary and the attack surface — bad for a security
tool. `graft` provides **primitives**; the Renovate layer is a GitHub
Action/script.

### 3. Flake pins are the keystone

The entire supply-chain half (N-x, PR with closure diff, "which digest is
running") only works if packages hang off flake refs/locks instead of
`pkgs.<name>` strings. Without deterministic pins, "what will run" is not
provable. Flake overrides therefore belong **before** the pipeline work.

### 4. Machine-readable output everywhere (read model)

Every `graft` command must be able to emit its result as JSON: effective config,
status, Quadlet/image diff, closure listing, SBOM. Reason: both the pipeline (d)
and the dashboard (g) then become **consumers** of the same data, not a
reimplementation. This is cheap to build now and saves a re-architecture later.
Bake it in early, even though the UI comes much later.

---

## Phased launch plan

### Phase 0 — Launch hardening (blocks the open-source release)

Goal: the tool is small, honestly positioned, and has no hole in itself.

- [x] **Close the `nixString` injection** + package-name allowlist (config →
      code-exec is a credibility issue for a security tool, not an ordinary bug).
- [x] **One resolution engine** (anchor #1) — CLI and NixOS render identically,
      or the CLI fails loudly on parents/children/packageOps. Current status: the
      CLI fails loudly; a shared resolver remains strategic work.
- [x] Newline/control-char validation in `Validate()` (Quadlet directive
      injection).
- [x] Make `IsNoop` robust (no manual field list).
- [x] Cleanup: stray `jj` file, dead `deploy.autostart`, unused
      `renderedQuadlet` output.
- [x] Table stakes: LICENSE, CONTRIBUTING, SECURITY.md, CI (`nix flake check` +
      `go test` + lint), semver, flake usable as `inputs.graft.url`.
- [x] Name decision: chose **`graft`** (graft/override metaphor = the inheritance
      model of the TOML graph), org-scoped `github.com/zerodawn1990/graft`. A bare
      `graft` GitHub org/domain is not needed; the namesake `orbitinghail/graft`
      is cross-ecosystem (a Rust storage engine), no practical confusion. Always
      brand as "graft — declarative containers from the Nix store" + topics
      `nix`/`podman`/`quadlet`/`containers` for SEO. A coined word = optional
      later track. See [`name-change.md`](name-change.md).

### Phase 1 — MVP / "wow" (the first public release)

Goal: **one** slice that impresses on its own. Pick one as the main story:

- **Option A — image-less project container:** `cd project` → isolated
  container, no Dockerfile, no image build, no NixOS rebuild.
- **Option B — agent sandbox:** sandbox an agent/update, see the diff, nothing
  touches your real workspace.

Both are largely present already. Ship alongside:

- [ ] Killer README: one-liner + before/after + asciinema/GIF (demo > docs for a
      UX tool).
- [ ] Honest comparison table (oci-containers / quadlet-nix / arion /
      nixos-containers / devenv) — builds trust.
- [ ] FAQ answering "Why TOML and not Nix?" and "Why not X?" explicitly.
- [ ] `examples/security/*` opt-in hardened parents (`locked`, `no-network`,
      `tmpfs-home`, `agent-safe`, ...).
- [ ] JSON output (`--json`) for inspect/render/status — the read-model
      foundation (anchor #4).

### Phase 2 — v0.x (broaden on adopter pull)

- [ ] Session lifecycle: `enter`/`leave`/`status`/`review`/`apply`/`discard` +
      shell hook (`graft hook`).
- [ ] Finish the promote flow: patch export, `apply`/`discard`, `promote` →
      branch/PR.
- [ ] `validation.level = strict` with checks for dangerous
      mounts/network/devices/secrets.
- [ ] Flake overrides/pins (anchor #3) — precondition for the supply-chain half.
- [ ] `closure-only` store access (closes the `/nix/store` exposure).
- [ ] Quadlet `.network` + proxy-sidecar egress examples.
- [x] Nix-native authoring next to TOML (frontend over the same engine,
      anchor #1) — `services.graft.containers.<name>` / `programs.graft.containers.<name>`.

### Phase 3 — v1 (the big vision, only if adoption pulls for it)

- [ ] Declarative image building (image mode via the same TOML graph +
      resolution engine, digest pins).
- [ ] Supply-chain pipeline as a thin layer on `graft` primitives (N-x delay,
      SBOM, scan, candidate cache, auto-PR).
- [ ] Cleanup/lifecycle policy for transient vs. managed resources +
      labels/headers.
- [ ] Optionally a TUI for effective-config/diff review.
- [ ] Management dashboard (multi-host, desired-vs-actual, GitOps PRs) —
      north star (g).

---

## Nix-native authoring (f) — implemented

TOML stays the source of truth for the fast/project route, but the modules also
accept attrsets via `services.graft.containers.<name>` /
`programs.graft.containers.<name>`, so Nix purists don't have to switch
languages. Important: this is an **authoring frontend**, not a second engine —
the attrset is serialized to TOML with `pkgs.formats.toml` and goes through the
same resolution + renderer as file-based configs (anchor #1). It resolves the
biggest cultural pushback ("why a second config language?") without giving up
the TOML advantages (project-local, PR/agent friendly, no eval needed).

See [reference.md](reference.md#nix-native-authoring-containers) for the option
shape and semantics. Still open: allowing `parents`/`children` refs to target
other `containers.<name>` entries (today graph refs resolve against
`configRoot`).

## Management dashboard (g) — Portainer for the GitOps/Nix world

A real gap: for Docker this is mature (Portainer, Dockge, Lazydocker). For
Podman/Quadlet it's thin — Podman Desktop and Cockpit-podman are single-host /
dev-oriented, and for **Quadlet + Nix-store containers across multiple hosts**
nothing mature exists. It fits the "persistent containers, more hosts" persona
exactly.

The coherent take that makes this *your* tool instead of a Portainer clone:

- **GitOps, no live mutation.** "Change" actions in the UI produce
  **branches/PRs**, not direct host writes. Consistent with the core principle
  "git is the truth, mutation via review".
- **Desired vs. actual.** Show desired state (from git / nix-eval) next to actual
  state (podman ps / systemctl / journald) plus the diff. That is the killer view
  and what differentiates it from Portainer (which mutates live state).
- **Read model.** The UI consumes the JSON from anchor #4; no privileged daemon
  that changes hosts outside the git/PR flow.

Honest warning: this is the biggest scope and **attack-surface multiplier**. A
multi-host management UI is an attractive target — extra awkward for a security
tool. So: **not in the MVP.** It is a north star (Phase 3+) that only steers your
choices now (JSON output, multi-host model), not your launch.

## Non-goals (deliberately out of the launch)

- Do **not** launch the supply-chain pipeline first — Renovate/dependabot/Nix-CI
  already exist and SBOM/scanning is a bottomless pit. Keep it on the roadmap so
  it doesn't hold the launch hostage.
- No implicit security policy (stays a core principle).
- No mandatory Dockerfile/OCI build for the fast route.

---

## Positioning vs. existing solutions

| Existing | What it does | Why it doesn't fill this gap |
|---|---|---|
| `virtualisation.oci-containers` | Declarative containers in NixOS | Image-based, system-declarative, needs a rebuild |
| `quadlet-nix` | Quadlet units via Nix | You write Nix, image-based, no project-ephemeral flow |
| `arion` | compose-like, Nix-native, host-store possible | Image/Nix-oriented, no direnv UX |
| `compose2nix` | compose → nix/quadlet | Conversion tool, no runtime/lifecycle |
| `nixos-containers` / `extra-container` | nspawn, shares host store | nspawn instead of Podman, Nix-defined, full system containers |
| `devenv`/`direnv` | Project env on `cd` | Host PATH, **not isolated** |

Each individual piece overlaps with something; the **combination** (image-less +
TOML + no rebuild + direnv UX + agent-safe + promote-to-declarative) does not
exist as a single tool.

---

## Open decisions

- ~~Public name/brand~~ — **decided: `graft`** (org-scoped `zerodawn1990/graft`);
  bare org/domain not needed; SEO tagline mandatory; a coined word = optional
  later track.
- MVP main story: option A (project container) or B (agent sandbox)?
- Image mode: render to `dockerTools.streamLayeredImage` + Quadlet `Image=` so
  the same TOML graph/resolution is reused?
- Binary cache for the pipeline: attic / cachix / nix-serve / own infra?
- How pins are represented in TOML vs. `flake.lock`.
- MVP persona: developer/agent (CLI) or homelab/infra-operator (declarative +
  dashboard)?
- Dashboard: how does it talk to hosts — read-only agent per host, SSH, or purely
  from git + nix-eval (desired) vs. podman/systemd (actual)?
- Nix-native: how to guarantee parity with the TOML schema (generate one schema
  definition)?
