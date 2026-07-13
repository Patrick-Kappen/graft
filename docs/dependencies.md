# Typed workload dependencies

> **Status:** implemented for Graft workloads and explicitly named external
> systemd units on both system and user targets.

Graft models common activation, ordering, and lifecycle relationships without
accepting raw `[Unit]` maps. Dependencies are typed workload intent, resolved by
the CLI, and rendered mechanically by the NixOS and Home Manager modules.

## Workload dependency

```toml
[[dependencies]]
target = { workload = "database" }
requirement = "required"
ordering = "after"
lifecycle = "part-of"
```

`workload` is another top-level Graft `name`, not a TOML filename, Quadlet
source-unit name, generated service name, or Podman runtime name. The resolver
uses the explicit source set to map it to the referenced `.container` source
unit.

The referenced workload must:

- exist exactly once in the explicit source set;
- be enabled;
- use the same effective `deploy.target`;
- not be the current workload; and
- not create a workload-reference cycle, including a cycle mixed with a shared
  network-namespace reference.

All three workload lifecycles may be requirement or ordering targets. A
`required` and `after` relationship to a `job` can therefore express a finite
prerequisite, while a `setup` may remain `active/exited` after successful
completion. `lifecycle = "bound"` rejects a `job` target because its successful
result is inactive and would immediately invalidate the binding.

## External-unit dependency

```toml
[[dependencies]]
target = { externalUnit = "postgresql.service" }
requirement = "required"
ordering = "after"
```

`externalUnit` is an explicit opt-in to a concrete unit in the workload's
selected system or user manager. It is not a raw directive or command. Graft
accepts the systemd unit suffixes `service`, `socket`, `device`, `mount`,
`automount`, `swap`, `target`, `path`, `timer`, `slice`, and `scope`.

External names must be ASCII, at most 255 characters, contain no whitespace,
control characters, paths, or specifiers, and name a concrete unit rather than
an uninstantiated `name@.service` template. Concrete instances such as
`worker@one.service` are valid.

Pure resolution cannot inspect the selected manager, so it cannot prove that an
external unit exists, is loadable, or has suitable runtime behavior. The exact
name remains visible in resolved JSON and generated source for review, and
systemd validates it when loading the generated service. A system-target
external dependency can activate a host unit; treat rootful TOML as privileged
host configuration.

Use `target = { workload = "..." }` for another Graft workload. Referring to a
Graft-generated service through `externalUnit` bypasses workload identity,
target, enablement, and cycle validation and is rejected when both target forms
resolve to the same service in one dependency list.

## Relationship axes

Each dependency configures one or more independent axes. At least one axis is
required; Graft adds no implicit relationship.

| Field | Value | Source `[Unit]` output | Meaning |
| --- | --- | --- | --- |
| `requirement` | `required` | `Requires=` | Request the target and couple activation failure/deactivation. Combine with `ordering = "after"` when the workload must not start if target activation fails. |
| `requirement` | `optional` | `Wants=` | Request the target without making target activation failure fail this workload. |
| `ordering` | `after` | `After=` | Start this workload after the target's start job completes. |
| `ordering` | `before` | `Before=` | Order this workload before the target when both are in the transaction. |
| `lifecycle` | `part-of` | `PartOf=` | Propagate target stop and restart operations to this workload; it does not activate the target. |
| `lifecycle` | `bound` | `BindsTo=` | Pull in the target and stop this workload if the target becomes inactive. It cannot be combined with `requirement`, because `BindsTo=` already activates the target. Combine with `ordering = "after"` for systemd's strongest active-state coupling. |

These relationships describe systemd unit state, not application-level
readiness. `After=` waits for the target's systemd start job; it does not probe a
port, health endpoint, or application protocol. Quadlet's default long-running
notify lifecycle reports container startup, not arbitrary application
readiness.

Compatible relations compose literally. For example,
`requirement = "required"` plus `ordering = "after"` renders both `Requires=`
and `After=`. Graft does not infer ordering from requirements, reverse
relationships, or add startup activation. A separate requirement beside
`lifecycle = "bound"` is rejected rather than presenting `BindsTo=` as optional
or rendering a redundant `Requires=`.

## Resolution and output

Given a Graft workload whose TOML source is `database.container`, the resolver
emits concrete source-unit identities:

```json
{
  "dependencies": {
    "requires": ["database.container"],
    "after": ["database.container"]
  }
}
```

The renderer writes:

```ini
[Unit]
Requires=database.container
After=database.container
```

Quadlet recognizes its own source-unit extensions in normal systemd dependency
directives, verifies that the referenced source exists, and translates the
result to:

```ini
[Unit]
Requires=database.service
After=database.service
```

External `.service` and other systemd unit names are already concrete and remain
unchanged. Resolved relation lists are sorted for deterministic JSON and output.

## Validation contract

Resolution rejects:

- a dependency entry without a relationship axis;
- duplicate entries for the same typed target;
- two target forms that resolve to the same service;
- unsafe workload or external-unit names;
- a separate requirement combined with `lifecycle = "bound"`;
- binding to a Graft `job`, whose successful unit state is inactive;
- missing, disabled, self, cross-target, or ambiguous workload references;
- duplicate workload or Quadlet source-unit identities; and
- any cycle among Graft workload references, including mixed namespace and
  generic dependency edges.

An empty `dependencies = []` list is equivalent to absence and renders no
`[Unit]` section. External-unit existence and relationships outside the
explicit Graft source set cannot be included in pure graph validation.

## Startup, lifecycle, and resource boundaries

Dependencies do not imply `deploy.activation = "startup"`. Starting a dependent
workload can activate its required, optional, or bound targets even when those
targets have no startup activation of their own. Conversely, manager startup
may request multiple workloads in parallel unless dependency ordering connects
them.

Typed resource references continue to own their automatic relationships. For
example, `config.network.mode = "container"` renders
`Network=<source>.container`; Quadlet adds the required `Requires=` and `After=`
relationships itself. Graft does not duplicate those lines through generic
dependencies. Future generated network, volume, and pod resources must follow
the same source-reference rule in their own contracts.

## Capability classification

- **First-class:** the six fixed mappings `Requires=`, `Wants=`, `After=`,
  `Before=`, `PartOf=`, and `BindsTo=` are available only through the typed
  target and relationship axes above.
- **Dangerous and unavailable:** `Conflicts=`, `Upholds=`, `OnFailure=`,
  `OnSuccess=`, and stop propagation to external units can activate, retain, or
  stop manager units outside the ordinary dependency direction. They require a
  separate classification under the [Capability policy](capability-policy.md).
- **Deferred:** `Requisite=` and reload propagation are not accepted without a
  concrete use case and contract.
- **Forbidden:** raw `[Unit]`, host commands, and arbitrary systemd maps are not
  Graft input paths.

## Upstream evidence

This contract is tested with Podman/Quadlet 5.8.2 and systemd 260:

- Quadlet enumerates the standard dependency keys whose source-unit references
  it translates:
  [Podman source](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/pkg/systemd/quadlet/quadlet.go#L223-L241).
- Quadlet translates recognized `.container`, `.network`, `.volume`, and other
  source-unit extensions to generated service names and rejects missing source
  units:
  [Podman source](https://github.com/containers/podman/blob/5b263b5f5b48004a87caac44e67349a8266d9ef4/pkg/systemd/quadlet/quadlet.go#L2298-L2331).
- systemd documents requirement, ordering, and lifecycle propagation as
  independent unit relationships:
  [`systemd.unit`](https://www.freedesktop.org/software/systemd/man/260/systemd.unit.html#%5BUnit%5D%20Section%20Options).
