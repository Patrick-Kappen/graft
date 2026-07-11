# Container network intent

> **Status:** approved design for
> [#144](https://github.com/Patrick-Kappen/graft/issues/144). The typed modes
> described here are not implemented yet; implementation is tracked by
> [#165](https://github.com/Patrick-Kappen/graft/issues/165).

Graft models the network namespace a workload needs. It does not expose raw
Podman network arguments, arbitrary Quadlet `Network=` text, or host firewall
configuration.

The first implementation phase covers the implicit default network, no network,
and sharing another Graft workload's network namespace. Host networking and
broader DNS, address, alias, and generated-network controls remain a second
phase.

## Intent contract

### Implicit default

When `[config.network]` is absent, or contains only currently supported
`publish` entries, no network mode is resolved and no Quadlet `Network=` line is
rendered:

```toml
[config.network]
publish = ["127.0.0.1:8080:8080"]
```

This preserves Quadlet and Podman's target-specific default. Graft deliberately
does not offer `mode = "default"`: absence is the single representation for
accepting the runtime default.

The default provides ordinary container networking, but it is not an egress or
firewall policy. Rootful and rootless Podman may use different implementations
and address ranges.

### No IP network

```toml
[config.network]
mode = "none"
```

Resolved intent renders mechanically as:

```ini
[Container]
Network=none
```

This gives the container its loopback device without external IP connectivity.
It is not a claim that every communication path is absent: a mounted Unix
socket, device, or other host resource may still provide communication.

### Share another workload's network namespace

```toml
[config.network]
mode = "container"
container = "database"
```

`container` is a typed reference to another Graft workload's top-level `name`.
It is not a Podman runtime container name, TOML filename, systemd unit name, or
free-form suffix for `container:`.

The resolver translates the workload reference to the referenced Quadlet source
unit. If that unit is `database.container`, Graft renders:

```ini
[Container]
Network=database.container
```

Quadlet resolves the source-unit reference to the runtime container identity and
adds service dependencies itself. With Podman 5.8.2 the normalized result is:

```ini
[Unit]
Requires=database.service
After=database.service

[Service]
ExecStart=<podman> run ... --network container:<runtime-container-name> ...
```

Graft must not render `Network=container:<runtime-container-name>` directly.
That raw Podman form does not ask Quadlet to resolve a source unit and therefore
does not gain Quadlet's existence check, `Requires=`, or `After=` relationship.
No generic `[Unit]` passthrough is needed for this relationship.

Sharing a network namespace means both containers use the same interfaces, IP
addresses, routes, and port space. They can reach each other over loopback, and
they can conflict when binding the same address and port. It does not share
filesystem, process, user, mount, or cgroup namespaces.

### Host network

The reserved typed form is:

```toml
[config.network]
mode = "host"
```

It maps to `Network=host`, but is not part of the first implementation phase.
Host networking removes the separate network namespace, exposes host loopback
and interfaces to the workload, and makes container port publication
meaningless. It is dangerous intent and must satisfy the capability policy in
[#128](https://github.com/Patrick-Kappen/graft/issues/128) before support lands.
The mode does not configure or attest to host firewall policy.

## Mode matrix

| TOML intent | Quadlet output | Exposure boundary | Phase |
| --- | --- | --- | --- |
| mode absent | no `Network=` | Podman target-specific default | A |
| `mode = "none"` | `Network=none` | loopback only, excluding other mounted communication paths | A |
| `mode = "container"` plus reference | `Network=<source>.container` | exactly the referenced workload's network namespace | A |
| `mode = "host"` | `Network=host` | host network namespace; dangerous | B, after #128 |

Only these values belong to the approved contract. Values such as `bridge`,
`slirp4netns`, `pasta`, arbitrary network names, and literal
`container:<runtime-name>` remain unsupported until separately designed.
Generated `.network` resources are tracked by
[#147](https://github.com/Patrick-Kappen/graft/issues/147).

## Validation contract

Resolution must reject explicit intent that cannot have the requested effect.

| Condition | Result |
| --- | --- |
| `container` is set without `mode = "container"` | error |
| `mode = "container"` has no `container` reference | error |
| referenced workload does not exist | error |
| workload references itself | error |
| reference cycle exists | error with the cycle path |
| referenced workload is disabled | error |
| source and referenced workload have different deploy targets | error |
| referenced workload has effective `job` or `setup` lifecycle | error |
| `publish` is combined with `none`, `container`, or `host` | error |
| unsupported or raw mode text is configured | field-specific error |

A referenced workload must have the effective `long-running` lifecycle. A
finite job has no stable network namespace after completion, while a retained
setup unit keeps systemd state but no running container. Quadlet's generated
`Requires=` and `After=` relationship starts and orders a valid long-running
reference when the dependent service starts. Independent boot or login
activation remains separate typed intent in
[#191](https://github.com/Patrick-Kappen/graft/issues/191).

Published ports are valid only with the implicit default mode in Phase A. They
are invalid with `none`, redundant with `host`, and cannot belong to a workload
that joins another container's existing namespace. The namespace owner must
carry future exposure intent for a shared namespace.

DNS servers, DNS search domains and options, host entries, aliases, static
addresses, and generated network references are invalid with Phase A modes
until their Phase B combinations are explicitly implemented. The fail-closed
resolver work in [#106](https://github.com/Patrick-Kappen/graft/issues/106)
must reject those reserved fields rather than silently discard them.

## Reference resolution

Single-file parsing is sufficient for scalar validation, but it cannot prove
that another workload exists, is enabled, has a compatible lifecycle, or shares
the same target. Phase A therefore requires an explicit config index owned by
the Rust resolver.

For each configured TOML source, the index contains only the context needed for
references:

- explicit source path and source-unit stem;
- top-level workload name;
- effective deploy target and enable state;
- effective service lifecycle;
- typed network reference, when present.

The Nix module supplies the concrete, deterministically ordered TOML source set.
The CLI parses and validates the index, resolves workload names to source-unit
identities, and reports duplicates, missing references, target mismatches, and
cycles. Nix does not interpret dependency semantics. The resolver must not scan
ambient directories, read environment-provided roots, or infer hidden state.

This index is not configuration merging. Parent/child precedence, overlays, and
provenance remain owned by
[#159](https://github.com/Patrick-Kappen/graft/issues/159). Unit and container
identity are still subject to
[#107](https://github.com/Patrick-Kappen/graft/issues/107); until that issue
unifies them, the index provides the explicit mapping from public workload name
to actual Quadlet source-unit stem.

## State and failure behavior

For a dependent workload `worker` sharing `database` networking:

```text
start worker
  → systemd starts database.service through Quadlet Requires=
  → database.service reaches active according to its lifecycle/readiness
  → worker.service starts in database's network namespace
```

Failure behavior:

```text
database cannot activate       → worker is not started successfully
database is stopped             → systemd propagates deactivation to worker
database runtime missing        → worker's Podman start fails
missing typed workload reference → resolver fails before materialisation
inconsistent Quadlet source set  → generator fails; no silently degraded unit
```

Application-level readiness remains distinct from service ordering. The current
long-running contract uses Quadlet/conmon readiness; typed health and
application-notify behavior remains tracked by
[#146](https://github.com/Patrick-Kappen/graft/issues/146).

## Migration from the reserved string field

The parser currently reserves `config.network.mode` as an unchecked string, but
the resolver ignores it. A value such as:

```toml
[config.network]
mode = "container:database-runtime-name"
```

is not preserved as supported syntax. The implementation must provide an
actionable diagnostic directing users to:

```toml
[config.network]
mode = "container"
container = "database"
```

The diagnostic should identify the old value and show both replacement fields,
for example:

```text
config.network.mode = "container:database" is not supported; use
config.network.mode = "container" with config.network.container = "database"
```

The reference uses the Graft workload name. It must not depend on a separately
configured or generated Podman runtime name.

## Phase A implementation and tests

[#165](https://github.com/Patrick-Kappen/graft/issues/165) must implement Phase
A through typed parser, schema, resolver, resolved-JSON, and renderer values.
Tests must include:

- parser/schema coverage for absent, `none`, and typed container modes;
- field-specific errors for malformed modes and invalid combinations;
- config-index tests for missing, disabled, self, cross-target, duplicate, and
  cyclic references;
- lifecycle compatibility tests, including the implicit long-running default;
- mirrored NixOS and Home Manager fixtures;
- real system and user Quadlet generation proving the normalized `--network`
  argument;
- generated-unit assertions for `Requires=` and `After=`;
- `systemd-analyze verify` over the complete generated service set;
- runtime tests proving no external IP path for `none` and shared loopback for a
  container reference.

Runtime tests must avoid claiming stronger isolation than they observe. In
particular, `none` tests should not mount host communication sockets, and shared
namespace tests should prove both connectivity and port-space sharing.

Phase A completion unblocks #106 even while dangerous host networking and the
broader Phase B remain open.

## Phase B boundary

Phase B may add the approved host mode after #128 and design typed intent for:

- loopback versus public published-port binds;
- network aliases;
- DNS servers, search domains, and options;
- static IPv4 and IPv6 addresses;
- host entries;
- references to generated `.network` resources;
- rootful/rootless and low-port constraints;
- inspectable effective exposure and diagnostics.

It must not add unrestricted `PodmanArgs=`, `GlobalArgs=`, raw Quadlet maps, or
host firewall mutation. Egress enforcement beyond the `none` namespace requires
a separately defined authority and threat model.

## Upstream evidence

This design was checked against Podman/Quadlet 5.8.2:

- Quadlet documents `.container` network references and their generated service
  dependency:
  [Podman documentation](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/docs/source/markdown/podman-systemd.unit.5.md#L717-L739).
- The generator resolves `.container` source units, adds `Requires=` and
  `After=`, rejects missing units, and emits `container:<resource-name>`:
  [generator source](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/pkg/systemd/quadlet/quadlet.go#L1811-L1860).
- Upstream fixtures assert both the generated dependency and runtime network
  argument:
  [Quadlet test fixture](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/test/e2e/quadlet/network.reuse.container#L1-L7).

The future compatibility matrix remains owned by
[#129](https://github.com/Patrick-Kappen/graft/issues/129).
