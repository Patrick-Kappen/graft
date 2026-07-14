# Graft — Design

## Principle

Graft has strict layer boundaries:

```text
TOML → CLI → JSON stdout → Nix modules → Quadlet .container
```

- **TOML** is the user-facing Graft language. It captures user intent.
- **CLI** resolves that intent into a complete JSON build spec.
- **NixOS/Home Manager modules** materialise the resolved spec into rootfs paths
  and Quadlet `.container` files.
- **Quadlet/systemd** runs the resulting services.

Users do not write Quadlet boilerplate and do not write Nix module boilerplate
for each container. The `.container` file is output. The typed distinction
between long-running services, finite jobs, and retained setup jobs is defined
in [Workload lifecycle semantics](lifecycle.md). Explicit manager-start policy
is defined separately in [Workload startup activation](activation.md). The
approved typed namespace, shared-container reference, and network security
boundaries are defined in [Container network intent](networking.md). Qualified
host-managed resource references are defined in
[Container Device Interface references](cdi.md). Current process
and rootfs controls are defined in [Container hardening](hardening.md). The
approved typed storage, host-bind, and collision boundary is defined in
[Filesystem and mount policy](filesystem-policy.md). Direct host devices remain
unavailable pending a host-aware attestation design.
Security assumptions, target authority, and current invariant evidence are
defined in [Threat model and trust boundaries](threat-model.md). The boundary
between
first-class typed intent, dangerous explicit authority, and forbidden escape
hatches is defined in [Capability policy](capability-policy.md).

## TOML is user intent

A TOML file should describe what the user wants, not how Podman, Quadlet, or Nix
will implement it.

Example:

```toml
version = 1
name = "node-dev"

[config.runtime]
packages = ["nodejs"]
```

That intentionally does not include:

- rootfs setup
- `/nix/store` mounts
- overlay details
- default keep-alive commands
- default restart policy
- default autostart / `[Install]` section
- Quadlet boilerplate

Those details are resolved by Graft and materialised by Nix.

## CLI resolve logic

For direct use, the CLI can resolve one TOML file to JSON stdout. Repeated
context arguments provide explicit sources for a cross-workload reference:

```text
graft <container.toml> [--context <other.toml>...] > $out
```

The Nix modules use the batch form instead. They stage every configured source
under its original filename, invoke the CLI once, and consume the returned map
through Import From Derivation:

```nix
resolvedSetJson = pkgs.runCommand "graft-resolve-set" { } ''
  mkdir context
  # The materialiser links each explicit TOML source into context/<filename>.
  ${graft}/bin/graft --set context/base.toml context/workload.toml > $out
'';

resolvedByFilename = builtins.fromJSON (builtins.readFile resolvedSetJson);
```

The CLI owns business logic:

- applying defaults represented in resolved JSON
- resolving typed workload and external-unit dependencies
- selecting the keep-alive command
- validating supported runtime modes
- preserving explicit user choices
- translating TOML/Graft concepts into the JSON shape Nix needs

The CLI does not write JSON files into the repository.

Typed cross-workload references require explicit context that one file cannot
provide. Repeated `--context <toml>` arguments support direct single-workload
resolution. The modules stage sources under their original filenames and invoke
`graft --set <toml>...`, which parses and indexes the complete concrete source
set once before returning resolved JSON keyed by TOML filename. Nix remains a
mechanical caller and does not resolve dependency semantics. This reference
index is distinct from future parent/child configuration merging; see
[Container network intent](networking.md) and
[Typed workload dependencies](dependencies.md).

Checks that evaluate this IFD path should be built explicitly, for example with
`nix build .#checks.x86_64-linux.nixos-module-eval`. `nix flake check` may omit
IFD-backed checks, so CI and release gates must not rely on it alone.

## `graft-pause`

`graft-pause` is a minimal keep-alive binary shipped by the same Rust crate as
`graft`.

```text
/bin/graft
/bin/graft-pause
```

It is always added to the resolved package list and therefore always present in
the generated rootfs.

Rules:

```text
implicit or long-running lifecycle + no user command
  → command = ["/bin/graft-pause"]
job or setup lifecycle + no user command
  → resolution error
user command
  → command = user command

packages = ["graft-pause", ...user packages]
```

`graft-pause` exits cleanly on `SIGTERM` and `SIGINT`, so stopping a generated
service should not fall back to SIGKILL or leave a failed unit.

There is no default `bashInteractive`, no default `coreutils`, and no default
`sleep infinity`.

## Defaults and explicit choices

The CLI may only add defaults that belong to Graft semantics.

| Field | Rule |
| --- | --- |
| `version` | required; currently only `1` is supported |
| `name` | required; must be safe for container and unit output |
| `config.runtime.packages` | always `graft-pause` + user packages |
| `config.runtime.command` | user command; `/bin/graft-pause` if missing for implicit or long-running lifecycle; required for `job` and `setup` |
| `deploy.target` | default `system`, unless user sets `user` |
| `config.runtime.mode` | currently only `rootfs-store` |
| supported container fields | no defaults; include only if user sets them |
| environment, publish, volumes, CDI devices | no defaults; preserve deterministic ordering rules |
| read-only rootfs, capability policy, and no-new-privileges | concrete secure defaults; typed relaxations remain explicit |
| `dependencies` | no defaults; typed relations resolve to sorted concrete source-unit or external-unit identities |
| `config.network.mode` | absent preserves Quadlet's target-specific default; `none` and typed container references are supported |
| `config.service.lifecycle` | absent means Quadlet's long-running notify default; explicit intent resolves to typed service fields |
| `config.service.restart` and timing fields | no defaults; include only if user sets them |
| `deploy.enable` | no default in JSON; modules render unless explicitly `false` |
| `deploy.activation` | `startup` resolves a fixed system/user `[Install]` target; absent means no startup relationship |

A TOML file existing means Graft may render a `.container` file. That is not the
same as automatically starting the service.

## Nix modules are dumb materialisers

The NixOS and Home Manager modules read resolved JSON and do mechanical work:

1. filter containers for their target (`system` or `user`)
2. map package names to Nix packages
3. build a `pkgs.buildEnv`
4. wrap it with real runtime directories (`/etc`, `/tmp`, `/var`, `/run`, ...)
5. render the Quadlet `.container` file
6. place it in the matching Quadlet search path

Beyond mechanically rendering when `deploy.enable` is absent, they do not
decide:

- which default command to use
- which implicit package is needed
- which restart policy applies
- whether `bash` or `coreutils` should be present
- how TOML concepts merge or validate

That is CLI logic.

## Rootfs and Quadlet

The current `rootfs-store` mode uses a rootfs from the Nix store, not container
images. Non-rootfs artifact scope remains undecided in
[#150](https://github.com/Patrick-Kappen/graft/issues/150); no future syntax is
promised.

```ini
[Container]
ContainerName=node-dev
Rootfs=/nix/store/...-graft-node-dev-env:O
Exec="/bin/graft-pause"
Volume=/nix/store:/nix/store:ro
```

Important details:

- `Image=` is not used for `rootfs-store` containers.
- `Rootfs=...:O` gives Podman a writable overlay above the read-only store rootfs
  by default. Explicit `config.filesystem.readOnly = true` makes container
  rootfs paths read-only while preserving the tested upstream read-write-tmpfs
  mount default.
- The current mode configures no persistent, inspectable upperdir. Do not rely
  on overlay writes after the runtime container is removed; reviewable
  diff/promote is future work tracked by [#160](https://github.com/Patrick-Kappen/graft/issues/160).
- The renderer currently adds a fixed read-only `/nix/store` bind. Typed mount
  targets cannot overlap the store, although an explicit bind can expose a
  selected store source elsewhere and trusted CDI edits can inject mounts.
- Graft adds no package outside the generated rootfs/store closure; an explicit
  bind, managed resource, or CDI edit may expose other host content.
- Graft performs no package installation or image pull at container runtime;
  the workload itself remains able to use its configured network access.

System containers use rootful Podman and kernel overlayfs through `:O`.
User-target containers run in the current Home Manager account's user manager;
Podman and rootless overlay support such as `fuse-overlayfs` apply only for a
non-root account. A root-owned user manager remains rootful.

## Build and cache behaviour

Incrementality comes from Nix:

```text
TOML unchanged        → same derivation → CLI does not run again
TOML changed          → CLI runs → new resolved JSON
packages changed      → rootfs changes
command/restart only  → Quadlet changes; rootfs may stay cached
```

The resolved JSON is a Nix store artefact, not a committed file.

## Current boundary

The current implementation focuses on the rootfs-store materialisation path.
The generated TOML schema exposes only implemented intent; reserved parser
fields fail closed. [Reference](reference.md) documents field semantics, while
[Capability status](capabilities.md) records which pipeline stages each field
reaches and separates current, deferred, dangerous, and forbidden behavior.
[Threat model and trust boundaries](threat-model.md) defines what that pipeline
protects, trusts, and leaves to host policy.

Currently proven:

- CLI resolver
- NixOS IFD materialisation
- Home Manager IFD materialisation
- system/rootful Quadlet runtime
- non-root Home Manager user/rootless Quadlet runtime
- useful Quadlet rendering for container identity, working directory,
  environment, environment files, published ports, volumes, qualified CDI
  references, explicit hardening, and service timing
- fake-spec CDI runtime translation without physical device hardware, combined
  with effective capability, no-new-privileges, and rootfs-write assertions
- clean keep-alive shutdown

Typed long-running, finite-job, and retained setup-job behavior is defined in
[Workload lifecycle semantics](lifecycle.md). Explicit startup activation is
implemented through the contract in [Workload startup activation](activation.md).
Other future work is tracked in [Roadmap](roadmap.md). Deliberate exclusions are
tracked in
[Non-goals and deferred scope](non-goals.md). Contributor workflow is tracked in
[Development](development.md).

## Future CLI control plane

The CLI is currently a deterministic build-time resolver. Later it should also
become the user-facing control plane for runtime workflows.

Agreed lifecycle command names:

```text
graft up
graft down
```

No `graft shell` command is planned. That does not decide a later
interactive-workspace access contract; see [Long-term vision](vision.md).

Runtime operations must remain separate from pure TOML-to-JSON resolution so Nix
evaluation stays deterministic and side-effect free.

## Non-goals

The high-level constraints are:

- TOML should not become raw Quadlet.
- TOML should not become raw Nix.
- Capability expansion must follow the
  [Capability policy](capability-policy.md), not raw escape hatches.
- Nix modules should not contain business logic.
- Packages should not be installed ad-hoc inside containers.
- Containers should not auto-start unless explicitly configured.
- Promote/diff workflows must never promote binaries.
- Hidden module state should be avoided.

See [Non-goals and deferred scope](non-goals.md) for the current detailed list.
