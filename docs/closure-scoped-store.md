# Closure-scoped Nix store exposure

> **Status:** approved design for [#207]. Implementation belongs to [#209].

Graft currently mounts the host's complete `/nix/store` read-only into every
`rootfs-store` workload. The rootfs uses only its selected package closure, but
the container can enumerate and execute unrelated host store paths. This design
replaces that complete-store bind with one read-only bind for every path in the
realised rootfs runtime closure.

## Security invariant

For a workload without an explicit mount or trusted CDI edit that changes the
store view, `/nix/store` is a read-only scaffold and every path visible
immediately below it is a member of the realised Graft rootfs runtime closure. A
host store path outside that closure is not visible through Graft-owned mounts,
and a process cannot create an additional direct child through writable-rootfs
state.

Closure scoping is mandatory rootfs mechanics for both manager targets. It is
not TOML intent, has no workload opt-out, and never falls back to mounting the
complete store. Failure to enumerate, retain, materialise, or render the exact
closure fails the Nix build or module activation input.

This invariant reduces package and configuration disclosure and prevents
accidental use of undeclared host-store paths. It does not:

- reduce host disk use or network access;
- prevent copying or downloading executables into writable locations;
- constrain explicit binds, managed volumes, tmpfs, or CDI-injected mounts;
- attest the safety of package contents;
- isolate processes from the kernel, container runtime, or manager; or
- stop a trusted CDI specification from replacing or adding mounts below
  `/nix/store`.

The typed filesystem policy continues to reject user-declared targets equal to,
above, or below `/nix/store`. CDI specifications remain host-managed trusted
policy and can invalidate the Graft-owned visibility invariant.

## Selected mechanism

### Deterministic closure metadata

The Nix materialiser uses `pkgs.closureInfo` over the final rootfs. Its
`store-paths` output is derived from Nix's exported reference graph and its
`total-nar-size` output provides the closure size. No closure query, package
installation, daemon call, or network fetch occurs during container startup.

The final list is sorted bytewise and deduplicated before rendering. Every entry
must be an absolute direct child of `/nix/store`. The store object is inspected
without following links and must itself be a directory or regular file.
Top-level symlinks and every malformed, missing, or unsupported entry fail the
build; Podman must never dereference a closure member into unrelated host
content.

`nix-store -qR` is rejected as the implementation mechanism. It would require a
Nix CLI/daemon query in a build, evaluation, activation, or startup phase and is
less mechanically tied to derivation inputs than `closureInfo`.

### Rootfs mountpoint materialisation

OCI runtimes need each bind target to exist before applying a read-only rootfs.
The materialiser therefore creates `/nix/store` and a type-matched empty
placeholder for every expected closure member while building the rootfs:

- a directory source receives an empty directory target;
- a regular-file source receives an empty regular-file target; and
- the final rootfs output receives its own directory target using `$out`.

The package environment closure is available before the final rootfs is built,
so those placeholders do not require evaluation-time reads. After building the
rootfs, `closureInfo` over the final output is authoritative. Implementation
must prove that the final closure equals the prepared package closure plus the
rootfs output; any unexpected reference fails closed rather than producing a
mount without a target.

This two-stage assertion avoids a circular dependency: the final rootfs hash is
not known until the rootfs is built, while its own store path is a valid member
of its runtime closure.

### Derived Quadlet source

The Quadlet source becomes a derivation instead of an evaluation-time string.
That derivation first mounts the prepared scaffold read-only and then reads
`closureInfo/store-paths` to emit one nested line per path:

```ini
Volume=<rootfs>/nix/store:/nix/store:ro,bind,nodev,nosuid
Volume=/nix/store/<path>:/nix/store/<path>:ro,bind,nodev,nosuid
```

The parent mount is required even when the rootfs is writable: without it, a
sufficiently privileged container process can add direct children to the empty
scaffold through overlay state and violate the visibility invariant. Nested
member targets already exist in the scaffold and remain mountable below its
read-only parent.

`noexec` is intentionally absent because selected package executables must run.
`bind` is non-recursive: Nix store paths are immutable filesystem objects and
Graft must not import unrelated host submounts. `nosuid` and `nodev` reduce
unneeded file privilege and device interpretation; `ro` prevents mutation.
Relabel options such as `z` and `Z` are forbidden because Graft must not mutate
labels on host Nix store objects.

NixOS installs the derived source through `environment.etc.<name>.source`; Home
Manager uses the equivalent `xdg.configFile.<name>.source`. Both paths therefore
share exactly the same materialisation and rendering mechanism. No additional
IFD boundary is introduced.

## Garbage collection and generations

`closureInfo` records full store paths in a referenced store output. The derived
Quadlet source also contains every full path, so Nix's reference scanner makes
the source depend on the mounted closure. The active NixOS or Home Manager
generation retains that source and therefore its transitive runtime closure.
Old retained generations retain their corresponding source and closure; once a
generation and all other roots are removed, normal Nix GC may collect it.

Container startup consumes only paths retained by the active declarative
generation. Graft does not create mutable GC roots and does not promise that a
removed generation remains runnable after collection. Transient-instance
lifetime remains separate work under [#156].

## Bounds and failure policy

Per-path mounts increase source-unit size, generated service size, command-line
length, mount count, and setup work. The first implementation uses conservative
hard limits:

| Quantity | Initial limit |
| --- | ---: |
| Closure members | 512 |
| Generated Quadlet source | 128 KiB |

Exceeding either limit fails with the workload name, measured value, configured
limit, and guidance to reduce `config.runtime.packages`. There is no complete-
store fallback. Raising a limit requires runtime evidence through the
compatibility work in [#129].

The pinned real Quadlet generator must also remain covered with a deliberately
large fixture. Its generated service must remain below 512 KiB and its
`ExecStart=` below 192 KiB. These are regression budgets, not promises that
arbitrary host `ARG_MAX`, systemd, OCI runtime, or mount limits are identical.

A missing source, missing or wrong-type target, closure mismatch, generator
rejection, or OCI mount failure is terminal and phase-specific. Diagnostics
must distinguish closure enumeration, rootfs placeholder materialisation,
Quadlet generation, and runtime mount failure.

## Prototype evidence

Prototypes used Nix 2.34.7, Podman/Quadlet 5.8.2, systemd 260.2, crun 1.27.1,
and rootfs overlay mode. The large closure included Firefox and was deliberately
larger than ordinary command-line workloads.

### Closure and generator measurements

| Closure | Members | NAR size | Quadlet source | Generated service | `ExecStart=` |
| --- | ---: | ---: | ---: | ---: | ---: |
| Small | 18 | 60.4 MiB | 2.8 KiB | 7.6 KiB | 2.8 KiB |
| Medium | 89 | 374.5 MiB | 13.8 KiB | 35.2 KiB | 13.6 KiB |
| Large | 372 | 1.54 GiB | 54.9 KiB | 138.0 KiB | 53.6 KiB |

The generator accepted directory and regular-file store outputs with
`ro,bind,nodev,nosuid`. An initially missing target placeholder failed at OCI
startup, confirming that placeholders are mandatory rather than cosmetic. The
size measurements cover the per-member prototype before adding the single
scaffold line; that fixed line does not materially change the recorded budgets.

### Runtime timings

Each case ran three create/start/remove cycles. Values below are observed ranges
in milliseconds, not performance guarantees.

| Target | Members | Create | Start and attach | Remove |
| --- | ---: | ---: | ---: | ---: |
| Rootless | 18 | 44–46 | 86–95 | 69–73 |
| Rootless | 89 | 41–46 | 88–96 | 62–76 |
| Rootless | 372 | 46–47 | 91–96 | 61–69 |
| Rootful VM | 18 | 80–99 | 185–206 | 158–175 |
| Rootful VM | 89 | 90–110 | 195–215 | 169–173 |
| Rootful VM | 372 | 93–195 | 217–375 | 169–182 |

All successful runs executed selected closure content, observed an unrelated
host-store path as absent, and observed `/nix/store` as non-writable. Rootless
and rootful creation rejected a nonexistent bind source; no test widened access
to the complete store.

A supplemental writable-rootfs prototype added `CAP_DAC_OVERRIDE` to exercise a
process able to modify the overlay. Rootful execution without the scaffold bind
successfully created an arbitrary `/nix/store` child. With the read-only scaffold
bind, the same process wrote elsewhere in the rootfs but received a read-only
filesystem error for the store child. Rootless execution with the scaffold also
rejected the store write. Implementation tests must preserve this distinction.

The measurements include cold-start noise and do not establish scalability
past 372 mounts. They justify the initial 512-member and source-size limits,
subject to the large-fixture regression budgets above.

## Compatibility boundaries

Rootful and rootless operation passed on the pinned NixOS test platform. The VM
had AppArmor support but did not exercise a custom enforcing profile. SELinux
was not enabled. Enforcing SELinux hosts are therefore not claimed compatible
until [#129] records a representative test; Graft will not use automatic store
relabeling as a workaround.

Different Podman, Quadlet, systemd, crun, kernel, mount, and host argument limits
remain compatibility dimensions. A target that rejects the generated mounts
fails closed. Closure scoping is not silently disabled for compatibility.

## Implementation decision

Implementation may proceed in [#209] with this scope:

1. build the package-environment closure metadata;
2. reject top-level symlink outputs, create type-matched closure targets in the
   rootfs, and assert the final closure shape;
3. derive one shared NixOS/Home Manager Quadlet source with a read-only scaffold
   mount followed by sorted per-path mounts and fixed options;
4. enforce member and source-size limits with actionable diagnostics;
5. add generator, rootful, rootless, regular-file, absent-path, GC-reference,
   malformed-input, and no-fallback tests; and
6. update the threat model and reference from complete-store to closure-scoped
   exposure.

Reporting closure member count and NAR size through a future unified inspection
command belongs to [#137]. It does not block implementation because the build
already has deterministic values and limit failures are actionable.

[#129]: https://github.com/Patrick-Kappen/graft/issues/129
[#137]: https://github.com/Patrick-Kappen/graft/issues/137
[#156]: https://github.com/Patrick-Kappen/graft/issues/156
[#207]: https://github.com/Patrick-Kappen/graft/issues/207
[#209]: https://github.com/Patrick-Kappen/graft/issues/209
