# Container Device Interface references

Graft supports qualified [Container Device Interface (CDI)] resource names as
narrow, first-class workload intent. CDI lets a host-managed registry describe
how a named resource changes an OCI container without exposing generic device
arguments in Graft TOML.

[Container Device Interface (CDI)]: https://github.com/cncf-tags/container-device-interface

## Configuration

```toml
[[config.filesystem.devices]]
source = "nvidia.com/gpu=all"
```

Each entry contains only `source`. Graft preserves declaration order, rejects
duplicates, and renders exactly one line per reference:

```ini
AddDevice=nvidia.com/gpu=all
```

The resolved JSON keeps the same narrow shape:

```json
{
  "filesystem": {
    "devices": [
      { "source": "nvidia.com/gpu=all" }
    ]
  }
}
```

An absent or empty `devices` list produces no filesystem or `AddDevice=` intent.
CDI references do not imply startup.

## Accepted name subset

A source has exactly the form `vendor/class=device`:

- `vendor` and `class` contain at least two characters, begin with an ASCII
  letter, end with an ASCII letter or digit, and otherwise contain only ASCII
  letters, digits, `.`, `_`, or `-`;
- `device` begins and ends with an ASCII letter or digit and otherwise contains
  only ASCII letters, digits, `.`, `_`, or `-`;
- the value contains exactly one `/` and one `=`; and
- empty values, whitespace-only values, control characters, and duplicates are
  rejected.

The upstream CDI grammar permits `:` inside a device name. Graft's current
contract deliberately accepts only the colon-free subset. A colon-containing
reference therefore fails resolution even if a particular runtime accepts it.
One-character vendor or class components are also excluded to avoid a known
failure path in the CDI parser used by the tested Podman 5.8.2 environment.

Direct paths such as `/dev/dri`, optional-device prefixes, target remapping, and
permission modes are not CDI-only intent and remain unavailable. The generated
schema publishes only `config.filesystem.devices[].source`; parser-reserved
`target` and `permissions` values fail normal resolution with indexed field
paths. Arbitrary Podman arguments remain forbidden.

## Host registry trust boundary

Graft validates and renders the qualified name. It does not read, build, copy,
or attest the corresponding host CDI spec. Registry presence, spec lifecycle,
ownership, permissions, runtime authorization, and referenced resource
availability remain host responsibilities. A missing or unusable reference
fails when Podman creates the container, not during TOML resolution or Nix
evaluation.

A host CDI spec is trusted policy. Depending on its contents, selecting one
qualified name can inject device nodes, mounts, environment values, and OCI
hooks. Reviewing only the Graft TOML is therefore insufficient: operators must
also review the effective host spec and its producers.

The target determines available authority:

| Graft context | Runtime boundary |
| --- | --- |
| `target = "system"` | The system manager invokes rootful Podman. The selected spec can exercise the resource authority available to host root and the runtime. |
| `target = "user"` under a non-root account | The user manager invokes rootless Podman. Same-user resources may still be exposed; inaccessible devices, mounts, or hooks should fail according to host and runtime policy. |
| `target = "user"` under UID 0 | The user manager remains root-owned and Podman remains rootful. This is not the non-root boundary. |

Graft does not infer or change the target from a CDI reference and does not make
a rootless resource available by changing host permissions.

## Tested translation

The deterministic Nix check covers NixOS and Home Manager rendering, preserves
reference order, runs the real Quadlet generator, verifies the resulting
services, and checks the generated Podman `--device` arguments. An advisory
NixOS VM test installs a controlled fake CDI spec whose only edit is an
environment value; the finite test workload succeeds only when Podman consumes
that edit. No physical GPU is required.

The pinned test environment currently uses Podman/Quadlet 5.8.2 with CDI library
1.0.1. These are tested versions rather than a formal minimum-version promise;
see [Capability status](capabilities.md#tested-upstream-context).

Typed bind, managed-volume, tmpfs, and collision rules are implemented through
the [filesystem policy](filesystem-policy.md). Direct host-device paths remain
deferred until a host-aware attestation contract exists.
