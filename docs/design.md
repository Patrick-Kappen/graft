# Architecture and responsibilities

Graft keeps a strict one-way materialisation pipeline:

```text
TOML → Graft CLI → resolved JSON → Nix modules → Quadlet → systemd → Podman
```

Each layer owns one kind of decision. Later layers must not reconstruct user
intent or introduce product policy hidden from the resolved contract.

## TOML is user intent

TOML describes supported workload behavior, not its Podman, Quadlet, systemd, or
Nix implementation:

```toml
version = 1
name = "node-dev"

[deploy]
target = "user"

[config.runtime]
packages = ["nodejs"]
```

It deliberately omits rootfs assembly, store mounts, overlay mechanics,
keep-alive implementation, raw unit sections, and Nix expressions. New runtime
authority must first become reviewed typed intent under the
[Capability policy](capability-policy.md).

## Resolver owns semantics

The CLI parses TOML, validates the concrete source set, resolves typed
cross-workload references, applies Graft defaults, and emits deterministic JSON.
It does not write state into the repository.

Direct single-workload resolution can receive explicit context files:

```text
graft <container.toml> [--context <other.toml>...] > resolved.json
```

The Nix modules use batch resolution. They stage every configured source under
its original filename and invoke the resolver once:

```nix
resolvedSetJson = pkgs.runCommand "graft-resolve-set" { } ''
  mkdir context
  # Materialisation links each configured source to context/<filename>.
  ${graft}/bin/graft --set context/base.toml context/workload.toml > $out
'';

resolvedByFilename = builtins.fromJSON (builtins.readFile resolvedSetJson);
```

The explicit source set provides the identity and target context needed for
workload dependencies and shared-container networking. It is not the future
parent/child configuration-merge graph.

### Semantic defaults

| Intent | Resolver rule |
| --- | --- |
| `version` | required; exactly `1` |
| `name` | required and output-safe |
| `deploy.target` | required explicit `system` or `user` |
| runtime mode | only `rootfs-store`; omission selects it |
| packages | Graft's `graft-pause` plus ordered user packages |
| command | preserve explicit argv; otherwise `/bin/graft-pause` for implicit or long-running lifecycle; require argv for `job` and `setup` |
| filesystem root | concrete read-only default; explicit `false` is a relaxation |
| process security | drop all capabilities and enable no-new-privileges; typed relaxations remain explicit |
| lifecycle, startup, dependencies, and network | no hidden relationships or autostart |
| `deploy.enable` | preserve only when explicit; materialisers render when absent |

Unknown fields, unsupported values, and parser-recognised reserved leaves fail.
The resolver never silently discards explicit intent.

## Nix modules materialise mechanics

NixOS and Home Manager consume resolved JSON through Import From Derivation
(IFD). The shared materialisation path:

1. selects resolved workloads for its `system` or `user` target;
2. maps `graft-pause` to the configured Graft package and other package names
   through the target's pinned `pkgs`;
3. builds the Nix-store rootfs;
4. creates required runtime directories and mount targets;
5. renders deterministic Quadlet source units;
6. places those units in the selected manager's search path.

The modules may enforce mechanical assertions needed to produce valid output,
but they do not choose defaults, resolve dependency meaning, or infer authority.
Absent `deploy.enable` meaning “render” is the documented materialisation rule.

IFD-backed checks must be built explicitly. `nix flake check` alone may not build
those derivations, so CI and release validation also build the NixOS and Home
Manager module-evaluation checks directly.

## Quadlet and runtime ownership

For `rootfs-store`, Nix renders a `.container` source unit with a store-backed
`Rootfs=`, resolved command, fixed complete-store bind, typed mounts, and
supported policy. Quadlet turns that source into a systemd service; systemd owns
activation and lifecycle; Podman owns container creation and runtime behavior.

The module never invokes `systemctl enable`. An absent activation setting emits
no `[Install]` section. Explicit startup resolves to a fixed manager-specific
target before rendering. See [Generated Quadlet output](quadlet.md) for exact
keys, quoting, ordering, and locations.

The current `Rootfs=...:O` mode does not provide a persistent inspectable
upperdir. Reviewable diff/promote is future work; binaries and package-manager
output must never become promoted state.

## Determinism and caching

Resolved JSON and generated rootfs paths are Nix store artefacts:

```text
TOML unchanged        → same resolver derivation
TOML changed          → new resolved JSON
packages changed      → new rootfs
only runtime policy changed → Quadlet may change while rootfs remains cached
```

Resolution must remain deterministic and side-effect free. Future host-aware
commands such as diagnostics, status, logs, `graft up`, and `graft down` must
stay outside the build-time resolver path.

## Contract boundaries

The current implementation intentionally forbids raw Podman arguments, raw
Quadlet maps, arbitrary systemd sections, host commands, and Nix expressions in
TOML. Host resources such as bind sources, environment files, external units,
named volumes, and CDI specifications remain explicit trust crossings rather
than Nix-module policy.

The [Configuration reference](reference.md) defines accepted user input.
[Capability status](capabilities.md) records each current pipeline stage and all
reserved concepts. The [Threat model](threat-model.md) owns security assumptions
and invariant evidence. [Non-goals](non-goals.md) and the [Roadmap](roadmap.md)
keep deferred product direction out of this architecture contract.
