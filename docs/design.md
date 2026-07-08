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
for each container. The `.container` file is output.

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

The CLI reads a single TOML file and writes resolved JSON to stdout:

```text
graft <container.toml> > $out
```

Nix captures stdout through Import From Derivation:

```nix
resolvedJson = pkgs.runCommand "graft-resolve-${name}" {} ''
  ${graft}/bin/graft ${tomlFile} > $out
'';

resolved = builtins.fromJSON (builtins.readFile resolvedJson);
```

The CLI owns business logic:

- applying Graft defaults
- adding implicit dependencies
- selecting the keep-alive command
- validating supported runtime modes
- preserving explicit user choices
- translating TOML/Graft concepts into the JSON shape Nix needs

The CLI does not write JSON files into the repository.

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
no user command → command = ["/bin/graft-pause"]
user command    → command = user command

packages = ["graft-pause", ...user packages]
```

`graft-pause` exits cleanly on `SIGTERM` and `SIGINT`, so stopping a generated
service should not fall back to SIGKILL or leave a failed unit.

There is no default `bashInteractive`, no default `coreutils`, and no default
`sleep infinity`.

## Defaults and explicit choices

The CLI may only add defaults that belong to Graft semantics.

| Field | Rule |
|---|---|
| `version` | required; currently only `1` is supported |
| `name` | required; must be safe for container and unit output |
| `runtime.packages` | always `graft-pause` + user packages |
| `runtime.command` | user command, or `/bin/graft-pause` if missing |
| `deploy.target` | default `system`, unless user sets `user` |
| `runtime.mode` | currently only `rootfs-store` |
| supported container fields | no defaults; include only if user sets them |
| environment, publish, volumes | no defaults; preserve deterministic ordering rules |
| `service.restart` and timing | no defaults; include only if user sets them |
| `deploy.enable` | no default in JSON; modules render unless explicitly `false` |
| autostart / `[Install]` | no default; future support must be explicit |

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

They do not decide:

- which default command to use
- which implicit package is needed
- which restart policy applies
- whether `bash` or `coreutils` should be present
- how TOML concepts merge or validate

That is CLI logic.

## Rootfs and Quadlet

Graft uses a rootfs from the Nix store, not container images.

```ini
[Container]
ContainerName=node-dev
Rootfs=/nix/store/...-graft-node-dev-env:O
Exec=/bin/graft-pause
Volume=/nix/store:/nix/store:ro
```

Important details:

- `Image=` is not used for `rootfs-store` containers.
- `Rootfs=...:O` gives Podman a writable overlay above the read-only store rootfs.
- `/nix/store` is mounted read-only inside the container.
- If a package is not in the generated rootfs/store closure, it is not available
  inside the container.
- No downloads happen at container runtime.

System containers use rootful Podman and kernel overlayfs through `:O`. User
containers use rootless Podman and rootless overlay support such as
`fuse-overlayfs`.

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
The TOML schema is broader than what the MVP renders today, but the renderer now
covers the common fields listed in [Reference](reference.md).

Currently proven:

- CLI resolver
- NixOS IFD materialisation
- Home Manager IFD materialisation
- system/rootful Quadlet runtime
- user/rootless Quadlet runtime
- useful Quadlet rendering for container identity, working directory,
  environment, environment files, published ports, volumes, and service timing
- clean keep-alive shutdown

Future work is tracked in [Roadmap](roadmap.md). Deliberate exclusions are
tracked in [Non-goals and deferred scope](non-goals.md). Contributor workflow is
tracked in [Development](development.md).

## Future CLI control plane

The CLI is currently a deterministic build-time resolver. Later it should also
become the user-facing control plane for runtime workflows.

Agreed lifecycle command names:

```text
graft up
graft down
```

No `graft shell` command is planned.

Runtime operations must remain separate from pure TOML-to-JSON resolution so Nix
evaluation stays deterministic and side-effect free.

## Non-goals

The high-level constraints are:

- TOML should not become raw Quadlet.
- TOML should not become raw Nix.
- Nix modules should not contain business logic.
- Packages should not be installed ad-hoc inside containers.
- Containers should not auto-start unless explicitly configured.
- Promote/diff workflows must never promote binaries.
- Hidden module state should be avoided.

See [Non-goals and deferred scope](non-goals.md) for the current detailed list.
