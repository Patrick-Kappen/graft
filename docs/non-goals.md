# Non-goals and deferred scope

Graft keeps a visible list of deliberate non-goals so small renderer issues do not
silently become product decisions.

A non-goal is not necessarily rejected forever. It means the current phase should
not implement it without a separate issue or design pass.

## Current v0.2 scope

The v0.2 renderer work focuses on safe, useful Quadlet output from resolved TOML.
It intentionally does not try to cover the full TOML schema or every Podman,
Quadlet, and systemd feature.

Deferred for v0.2:

- no port syntax parser for `PublishPort=` values
- no filesystem path existence checks for volumes
- no volume mode allowlist beyond line-safety validation
- no Quadlet `.volume` or `.network` unit generation
- no automatic firewall, DNS, or network alias management
- no systemd timespan parser for service timing values
- no implemented `[Install]` or autostart rendering yet; the typed startup
  contract is designed in [Workload startup activation](activation.md) and
  implementation remains tracked by #132
- no `restartIfChanged` rendering
- no raw systemd service type or `RemainAfterExit=` passthrough; lifecycle stays
  typed Graft intent
- no supplemental groups, UID/GID mapping, or user namespace policy from the
  group renderer
- no secrets materialisation or host environment passthrough
- no generated environment files

## Architecture boundaries

These boundaries are intentional:

- TOML is user intent, not raw Quadlet.
- TOML is not a second NixOS module language.
- The CLI owns defaults, validation, dependency resolution, and semantic
  decisions.
- NixOS and Home Manager modules are dumb materialisers.
- Nix evaluation must stay deterministic and side-effect free.
- Hidden state between modules or commands should be avoided.
- Packages are declared in TOML and realised by Nix; they are not installed
  ad-hoc inside containers.

## Runtime and product workflow deferred scope

The current renderer work does not yet implement the larger Graft product flow.
Deferred topics include:

- Git-aware copied workspace workflows
- context-aware template variables
- named instances and dynamic hostname strategy
- promote and diff workflows
- CLI runtime control beyond the agreed future command direction
- local TOML linting and host-aware doctor diagnostics
- host login policy in TOML, such as enabling systemd user linger from a
  container definition
- dedicated security hardening defaults such as `userns=auto`, limited UIDs,
  workdir-only writes, resource limits, and secrets support
- OCI and other artifact backends beyond the current `rootfs-store` path
- repository-defined workload graphs that span local, remote, and temporary
  placements
- interactive workspace access, IDE attachment, TUI, and optional web control
  surfaces

These are deferred rather than permanently rejected; see
[Long-term vision](vision.md). The agreed future lifecycle command names remain:

```text
graft up
graft down
```

No `graft shell` command is planned. That command decision does not define the
later interactive-workspace access contract; see [Long-term vision](vision.md).

## Literal passthrough policy

Some upstream syntaxes are broad and already validated by Podman, Quadlet, or
systemd. For those fields, Graft should prefer line-safe passthrough until a
separate policy issue exists.

Current examples:

- `PublishPort=` values
- `Volume=` strings assembled from TOML parts
- systemd service timing values such as `RestartSec=` and `TimeoutStartSec=`

Line-safe passthrough means:

- reject empty or whitespace-only values where the field is present
- reject control characters
- render mechanically
- do not add a parser, allowlist, or policy by accident

## When to update this page

Update this page when implementation or review finds a repeated phrase like:

- "out of scope"
- "not yet"
- "deferred"
- "no parser yet"
- "no policy yet"
- "separate design issue"

If a non-goal later becomes planned work, create or link the issue and move the
specific item out of this page when the implementation lands.

Related tracking issues:

- [#11: Design named instances and dynamic hostname strategy](https://github.com/Patrick-Kappen/graft/issues/11)
- [#12: Design context-aware template variables for repo branch worktree and agent](https://github.com/Patrick-Kappen/graft/issues/12)
- [#13: Backlog: reduce Nix module rendering complexity](https://github.com/Patrick-Kappen/graft/issues/13)
- [#27: Design Git-aware copied workspace workflow](https://github.com/Patrick-Kappen/graft/issues/27)
- [#150: Decide Graft scope for image, build, kube, and artifact Quadlet units](https://github.com/Patrick-Kappen/graft/issues/150)
- [#159: Design deterministic parent, child, and multi-source config merging](https://github.com/Patrick-Kappen/graft/issues/159)
- [#161: Design multi-host build, deployment, and remote lifecycle control](https://github.com/Patrick-Kappen/graft/issues/161)
- [#100: Add graft lint for TOML diagnostics](https://github.com/Patrick-Kappen/graft/issues/100)
- [#101: Add graft doctor for local environment diagnostics](https://github.com/Patrick-Kappen/graft/issues/101)
