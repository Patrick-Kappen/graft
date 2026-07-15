# Threat model and trust boundaries

> **Status:** this document defines the security assumptions and invariants of
> the current `rootfs-store` MVP, including the implemented
> [secure target defaults](secure-defaults.md). It does not claim complete
> production isolation.

Graft turns selected TOML files into resolved JSON, Nix-store root filesystems,
Quadlet source units, generated systemd services, and Podman containers. This
model identifies what that pipeline protects, what it trusts, and where an
operator must apply policy outside Graft. The separate
[Capability policy](capability-policy.md) classifies first-class typed intent,
dangerous explicit authority, and forbidden escape hatches.

The central rule is:

> A TOML file selected through a Graft module config root is trusted
> configuration with the authority of its effective target. Its location in an
> application repository does not make unreviewed TOML safe.

A system-target TOML is host-privileged configuration because it controls a
rootful container and can request host mounts or same-manager units. A
user-target TOML is configuration trusted with the current Home Manager
account's authority. Podman is rootless only when that account is non-root; a
root-owned user manager retains root authority, and Graft does not enforce the
UID. Rootless execution reduces the host privilege available to the runtime,
but it does not isolate a workload from everything accessible to the same host
account.

## Security objectives

Under the [trust assumptions](#trust-assumptions), Graft aims to ensure that:

1. only currently supported, typed intent reaches resolved JSON;
2. unsupported or dangerous intent fails closed rather than disappearing;
3. Nix modules materialise resolver decisions without widening them;
4. generated Quadlet and systemd text cannot gain extra directives through
   control characters or unescaped supported values;
5. workload references resolve only through the explicit source set and do not
   cross system/user targets silently;
6. startup remains absent unless typed startup or dependency intent requests
   it; and
7. generated inputs, concrete identities, and Graft-owned defaults represented
   in resolved output remain reviewable; intentionally preserved upstream
   defaults are identified as such.

These objectives constrain Graft's translation pipeline. They do not make a
malicious workload process, malicious package, compromised host, or unreviewed
TOML safe.

## Protected assets

| Asset | Security interest |
| --- | --- |
| Host root and kernel | Prevent an input from silently gaining rootful, host-namespace, device, capability, or host-command authority. |
| User account | Limit a rootless workload to reviewed access and avoid presenting rootless as isolation from the same account. |
| Host files and mounted state | Make host crossings explicit and avoid implicit deletion or promotion. |
| Nix store and selected package set | Preserve immutable build inputs and prevent TOML from becoming arbitrary Nix evaluation. |
| Resolved JSON and generated units | Preserve typed intent, concrete identity, deterministic output, and valid escaping across parsers. |
| Credentials and sensitive values | Keep unsupported secret flows from being mistaken for a protected transport. |
| Workload identity and relationships | Prevent ambiguous, cross-target, missing, disabled, self, duplicate, or cyclic Graft references. |
| Service availability | Avoid hidden activation and identify current denial-of-service gaps such as absent resource limits. |
| Future control-plane identity and authority | Prevent local or remote clients from confusing hosts, scopes, workloads, generations, or typed permissions. |
| Persistent data and foreign units | Keep materialisation and activation changes from implicitly deleting unrelated state. |

## Trust assumptions

### Trusted inputs and components

The current model trusts:

- the host administrator and the NixOS or Home Manager configuration that
  selects Graft's package and config roots;
- every TOML file in those selected roots, after normal code review, for the
  authority of its effective target;
- the selected nixpkgs package set and every package deliberately included in a
  workload rootfs;
- the Graft CLI, Nix evaluator and daemon, activation tooling, filesystem
  permissions, Quadlet generator, systemd manager, Podman runtime, OCI runtime,
  and host kernel;
- host account, linger, authentication, firewall, DNS, storage, backup, and
  update policy;
- any external systemd unit deliberately named by trusted TOML; and
- the host CDI registry, each referenced CDI spec, and the software that
  produces those specs.

A compromise of these trusted computing base components can bypass Graft's
controls. Graft pins and tests some build inputs, but it is not an independent
sandbox around Nix, systemd, Podman, or the kernel. The exact tested upstream
versions are recorded in
[Capability status](capabilities.md#tested-upstream-context).

### Untrusted data and actors

The model assumes that the following may be hostile:

- network clients sending data to a published workload;
- a process running inside a workload, including its child processes;
- application input, checked-out source, generated files, archives, and other
  repository content processed by that workload;
- writable overlay state and data in explicitly mounted writable paths; and
- malformed TOML submitted for validation before a trusted operator accepts it.

Graft validates malformed or unsupported TOML, but validation is not an
authorization boundary for an attacker who can change activated configuration.
An actor able to modify a selected config root or the host configuration has the
corresponding target authority. Do not automatically activate TOML from an
untrusted pull request.

A local process running as the same user as a Home Manager target is also
outside the rootless isolation claim: normal same-user access to configuration,
runtime APIs, mounted files, or the user manager is governed by host policy, not
by Graft.

## Pipeline trust boundaries

```text
trusted host configuration selects package + TOML roots
  ↓
TOML parser and Rust resolver
  ↓ validated, resolved JSON in the Nix store
Nix evaluation and rootfs construction
  ↓ mechanically rendered Quadlet source
NixOS system path or Home Manager user path
  ↓ Quadlet generator
system or user systemd service
  ↓ Podman + OCI runtime
container process sharing the host kernel
  ↕ explicit mounts, CDI resources, environment files,
     network, and unit relationships
host resources and other workloads
```

### Planned control-plane boundary

The future [control-plane architecture](control-plane.md) adds Nix-managed
system and per-account user workers outside the deterministic materialisation
pipeline. Local CLI and TUI clients connect over typed Unix-socket APIs; an
optional controller may later connect through an authenticated remote protocol.
The worker adapts approved operations to the materialisation manifest, systemd
D-Bus, journald, Podman, cgroups, and bounded storage accounting.

That boundary introduces additional threats which must be resolved before
implementation:

- hostile, spoofed, or compromised clients and sockets;
- confused-deputy requests crossing system, user, host, workload, or generation
  identity;
- treating Unix peer credentials as sufficient authorization rather than an
  authenticated input to host policy;
- stale or malicious manifests, search-path shadowing, and workload-name reuse;
- backend-controlled logs, paths, labels, metrics, and diagnostics reaching a
  terminal or remote client;
- unbounded log, event, metric, storage, or concurrent-operation requests;
- replay, downgrade, revocation, reconnect, and partial-delivery failures on the
  future remote protocol;
- controller compromise being mistaken for authority to bypass local Nix
  policy; and
- operational caches or history becoming a hidden desired-state database.

The planned split keeps a user worker within its owning account and a system
worker within system scope. A normal CLI/TUI may aggregate authorized views, but
system mutation requires explicit host authorization and ambiguous cross-scope
names fail closed. The worker does not accept raw shell, Nix, systemd, D-Bus,
Podman, Quadlet, or host-path passthrough. Controller loss cannot stop workloads
or block local clients, and worker restart reconstructs observations from
read-only manifests and authoritative backends rather than persisted intent.
Detailed controls and evidence remain acceptance criteria of [#240], [#242],
and [#245]; this section does not claim they are implemented today.

### 1. Config selection and parsing

The NixOS or Home Manager module enumerates regular `.toml` files only from
explicit `configRoot` and `configRoots` paths. It stages that concrete source
set under original filenames and invokes `graft --set` once. The CLI does not
scan ambient directories or discover configuration through environment state.

Unknown TOML fields fail deserialization. Parser-recognised reserved fields are
excluded from the supported schema and fail normal resolution even when their
explicit value is `false`, zero, empty, or otherwise apparently harmless.
`validation.level` cannot downgrade that behavior. `deploy.target` is required;
omission fails before materialisation so rootful system authority cannot result
from an implicit target choice.

### 2. Resolution and Nix materialisation

Resolved JSON is the only semantic input consumed by the Nix materialiser. The
resolver owns target selection, graph decisions, concrete unit identities, and
the defaults it represents in JSON. Nix applies the documented mechanical rule
that an absent `deploy.enable` means materialise, resolves the mandatory
`graft-pause` package from the host-selected Graft `cfg.package`, resolves other
package names from the host-provided `pkgs` set, constructs the rootfs, filters
containers by target, and renders fixed keys.

An absent `config.network.mode` intentionally preserves Podman's target-specific
default, and an absent `config.service.lifecycle` preserves Quadlet's
long-running behavior. Those upstream defaults are documented boundaries, not
effective values represented in resolved JSON.

Resolved JSON, generated Quadlet text, package paths, commands, and configured
environment values may enter world-readable Nix-store objects. They are not a
secret channel. TOML cannot contain arbitrary Nix expressions or select a new
package repository, but a trusted config author can choose code from the
host-provided package set and the host selects the Graft package that supplies
`graft-pause`.

### 3. Quadlet and manager materialisation

System-target source units are materialised for the system manager and rootful
Podman. User-target source units are materialised for the current Home Manager
account's user manager. Podman is rootless only when that account is non-root;
under UID 0 it remains rootful. Quadlet translates those source units into
generated services; systemd owns service lifecycle and Podman owns container
execution.

Graft emits only its fixed supported `[Unit]`, `[Container]`, `[Service]`, and
`[Install]` keys. It does not accept raw sections, host commands, or arbitrary
Podman arguments. This controls Graft's own output, not other Quadlet search
paths, systemd drop-ins, or administrator-managed units. Detection of shadowing
and foreign overrides remains in
[#171](https://github.com/Patrick-Kappen/graft/issues/171).

### 4. Runtime and host-resource crossings

Containers share the host kernel. A rootful system container or root-owned
user-target container is not a boundary against hostile code that requires
protection from host root. A user-target container under a non-root account
reduces runtime privilege through Podman's rootless model, but a kernel/runtime
vulnerability or an explicitly exposed same-user resource can still cross the
boundary. Use a VM when the workload must not share the host kernel.

The generated rootfs lower layer, `/nix/store` scaffold, and realised closure
member mounts are read-only. The `:O` mode provides runtime overlay state, which
is not a durable or reviewable persistence contract, while Graft's default
`config.filesystem.readOnly = true` blocks writes to container rootfs paths.
Upstream-managed and typed tmpfs mounts can still provide
selected writable paths. Effective process writes remain subject to mountpoint
modes and the dropped capability set. Typed mount targets cannot overlap
`/nix/store`, but an explicit bind can expose a selected store source at another
target and CDI specs can inject mounts. Graft therefore does not guarantee an
effectively read-only workload view. Binds, managed volumes, CDI references,
environment files, published ports, shared network namespaces, and external-unit
dependencies cross back into host or manager resources and must be reviewed as
such. Graft validates a CDI qualified name but does not inspect the host spec
that can inject device nodes, mounts, environment values, and OCI hooks.

The implemented [closure-scoped store contract](closure-scoped-store.md) fails
materialisation unless the derived source contains the exact realised rootfs
closure within fixed member and fragment limits. Unrelated host store paths are
not visible through Graft-owned mounts, and no complete-store fallback exists.
Closure scoping does not constrain explicit binds or trusted CDI edits.

## Current security invariants and evidence

The identifiers below are stable references for future design and feature
issues. “Evidence” names the tests or checks that currently exercise the
invariant; it does not extend the invariant beyond its stated scope.

| ID | Current invariant | Enforcement | Evidence |
| --- | --- | --- | --- |
| **GRAFT-TM-01** | Unknown and explicitly configured unsupported intent never reaches normal resolved JSON. | [`schema.rs`][schema-source] uses `deny_unknown_fields`; [`resolve.rs`][resolve-source] exhaustively classifies parser fields and fails closed. | Parser `unknown_field_returns_error`; resolver `configured_unsupported_fields_return_field_specific_errors`, `explicit_empty_unsupported_leaf_values_return_errors`, and `validation_level_cannot_disable_fail_closed_resolution`; generated-schema parity in [`tests/schema.rs`][schema-tests]. |
| **GRAFT-TM-02** | Graft TOML cannot supply raw Quadlet maps, arbitrary Podman arguments, or host systemd commands. | Unsupported `podmanArgs`, `globalArgs`, and `config.quadlet.*` fields fail; [`render-quadlet.nix`][renderer-source] owns a fixed key set. | Resolver unsupported-field matrix; negative reserved-field schema probe and security job in [`ci.yml`][ci-source]. |
| **GRAFT-TM-03** | Supported scalar and list values cannot inject an additional generated line through control characters; parser-specific output is escaped mechanically. | Resolver line-safety and identity validators; renderer quoting plus `%` and `$` escaping. Broad literal fields receive line safety, not invented semantic policy. | Resolver control-character and unsafe-name tests; system/user escape assertions and real generator plus `systemd-analyze verify` checks in [`flake.nix`][flake-source]. |
| **GRAFT-TM-04** | Graft workload references use only the explicit source set and cannot silently cross target, identity, enablement, or lifecycle constraints. | `ConfigSource`, `ConfigIndex`, and graph validation in [`resolve.rs`][resolve-source]; one explicit set invocation in [`materialise-containers.nix`][materialiser-source]. | Missing, disabled, self, cross-target, duplicate, identity-membership, and mixed-cycle resolver tests; Quadlet dependency and network checks in [`flake.nix`][flake-source]. |
| **GRAFT-TM-05** | A resolved workload is materialised only by the module matching its effective `system` or `user` target; `user` selects manager scope, not an enforced non-root UID. | Target filtering in [`materialise-containers.nix`][materialiser-source]; separate [`nixos.nix`][nixos-source] and [`home-manager.nix`][home-manager-source] destinations. | Module assertions prove opposite-target files are absent; [`activation.nix`][activation-test] proves rootful system execution and rootless user-manager execution for its non-root test accounts. |
| **GRAFT-TM-06** | Materialisation does not imply startup. Typed startup has only fixed system/user targets, and dependency activation remains explicit. | Resolver maps `startup` to `multi-user.target` or `default.target`; absent intent renders no `[Install]`. | Resolver startup tests; `quadlet-activation` generator checks; manager transitions and foreign-unit preservation in [`activation.nix`][activation-test]. |
| **GRAFT-TM-07** | Configured rootfs package names resolve only from host-selected sources: mandatory `graft-pause` from the configured Graft package and other names from `pkgs`. Package file collisions, reserved runtime `/etc` entries, and package `/etc` copy errors fail materialisation. `Rootfs=<store-path>:O` uses the read-only lower layer; the initial Graft-owned `/nix/store` view contains only a read-only scaffold and the realised rootfs runtime closure. Typed targets cannot overlap the store, but explicit binds can expose selected store paths elsewhere and trusted CDI edits remain outside this invariant. | Package mapping and collision-safe `buildEnv`, shared fail-fast `/etc` materialisation, `pkgs.closureInfo`, type-matched placeholders, closure equality and limits, and derived source construction in [`materialise-containers.nix`][materialiser-source]; fixed `Rootfs=:O` and ordered typed mounts in [`render-quadlet.nix`][renderer-source]. | Package collision, missing/valid/conflicting/dangling `/etc`, required runtime-entry, NixOS/Home Manager closure parity, generator, missing-source, rootful, and rootless checks in [`flake.nix`][flake-source] and [`closure.nix`][closure-test]. Overlay durability is explicitly excluded. |
| **GRAFT-TM-08** | Implemented non-default network namespaces are typed as `none` or a validated same-target Graft workload reference. | Network resolver and graph validation; source-unit rendering lets Quadlet own runtime identity and dependencies. | Resolver network matrix; `quadlet-network` generation; rootless no-network and shared-loopback checks in [`network.sh`][network-test]. |
| **GRAFT-TM-09** | Current declarative startup changes do not implicitly remove mounted state, workspace markers, or foreign units. | Modules replace managed source-unit declarations only; no Graft cleanup control plane exists. | Removal, reboot, restoration, and preservation scenarios in [`activation.nix`][activation-test]. |
| **GRAFT-TM-10** | External-unit dependency intent remains an exact, validated, visible same-manager unit name rather than host command text. | Strict concrete unit-name validation and fixed dependency axes in [`resolve.rs`][resolve-source]. | External-name, identity-collision, module parity, real Quadlet translation, and `systemd-analyze verify` tests. Unit existence and safety are host review responsibilities. |
| **GRAFT-TM-11** | Repository quality gates scan for known dependency advisories, configured dependency-policy violations, recognized secret patterns in the current tracked-file snapshot, and high-confidence workflow findings before merge. | Pinned Nix tools and commit-pinned GitHub Actions; gitleaks uses its configured signatures and zizmor runs at `high` minimum confidence. | `cargo-audit`, `cargo-deny`, the tracked-file gitleaks scan, zizmor, actionlint, named CI jobs, and coverage in [`ci.yml`][ci-source]. These checks do not scan removed secrets in Git history; advisory databases, patterns, rules, and confidence thresholds can produce false negatives. They reduce supply-chain risk without proving the snapshot or dependencies benign. |
| **GRAFT-TM-12** | Device intent accepted by Graft is limited to ordered, colon-free qualified CDI names; direct paths, duplicate references, target remapping, permissions, and arbitrary runtime arguments remain unavailable. The host CDI spec is trusted policy rather than validated Graft input. | CDI grammar and indexed field validation in [`resolve.rs`][resolve-source]; fixed `AddDevice=` rendering in [`render-quadlet.nix`][renderer-source]. | Resolver positive and negative CDI tests; generated-schema parity; `quadlet-cdi` NixOS/Home Manager generator verification and the controlled fake-spec runtime test wired through [`flake.nix`][flake-source]. |
| **GRAFT-TM-13** | Every workload resolves read-only rootfs, drop-all capabilities, and no-new-privileges defaults; target selection is explicit, and typed boolean opt-outs plus canonical capability additions are visible dangerous intent. | Hardening schema constraints and resolver validation in [`schema.rs`][schema-source] and [`resolve.rs`][resolve-source]; fixed `DropCapability=`, `NoNewPrivileges=`, and `ReadOnly=` rendering in [`render-quadlet.nix`][renderer-source]. | Resolver default, positive, ordering, false-value, malformed, mixed, and duplicate tests; schema parity; combined CDI/hardening system and user generator verification; controlled runtime checks for effective capabilities, no-new-privileges, and rootfs writes. |
| **GRAFT-TM-14** | Typed tmpfs accepts only an absolute normalised target plus bounded mode and size. It always renders fixed `rw,noexec,nosuid,nodev` flags. Protected targets, duplicates, and ancestor or descendant overlaps fail across tmpfs, binds, and managed volumes; only tmpfs may target the approved temporary trees. | Indexed option and path validation plus shared mount-collision validation in [`resolve.rs`][resolve-source]; fixed ordered `Tmpfs=` rendering in [`render-quadlet.nix`][renderer-source]. | Resolver option, protected-target, temporary-tree, same-kind, and cross-kind collision tests; generated-schema parity; NixOS and Home Manager assertions, real Quadlet generation, and rootful/rootless runtime evidence. |

## Threats, controls, and residual risk

### Configuration and generated-unit injection

Typed parsing, fail-closed resolution, line-safety validation, fixed rendering,
and real-generator checks reduce command, unit, and Quadlet injection through
supported fields. Raw `[Unit]`, `[Service]`, `[Install]`, Podman arguments, host
shell, and arbitrary Nix are not accepted input paths.

Residual risk remains in deliberately broad upstream syntaxes such as
published ports, environment-file paths, and systemd timing values, and in
typed host resources whose activation-time properties cannot be attested.
Graft validates their documented structure but does not prove the resulting
host policy safe.

### Runtime privilege and container escape

System targets use rootful Podman. Their TOML is host-privileged and must not be
accepted from an untrusted workload author. User targets use the current Home
Manager account's authority: Podman is rootless for a non-root account and
rootful under UID 0. Graft does not enforce that account UID, per-container
subordinate identities, seccomp policy, security labels, a mandatory non-root
container user, or
workdir-only writes. Drop-all capabilities, no-new-privileges, and a read-only
root filesystem are concrete Graft defaults; typed opt-outs and capability
additions remain explicit dangerous intent. Direct host device paths, remapping, and permissions
remain unavailable and fail closed. Qualified CDI references are current, but
their host-managed specs are trusted policy and can widen the container's effective
OCI resources. System targets and root-owned user targets consume those specs
through rootful Podman; non-root user targets consume them through rootless
Podman and remain limited by host and runtime authorization. See
[Container Device Interface references](cdi.md). The runtime's standard device
set remains upstream policy.

Other explicit `config.security.*` intent still fails closed. Omitting supported
hardening fields resolves Graft's concrete secure defaults. `ReadOnly=true` also preserves the tested
upstream read-write-tmpfs mount default, without guaranteeing process write
permissions, and does not constrain explicit tmpfs, volumes, or CDI-injected
mounts.
Capability classification is defined in the
[Capability policy](capability-policy.md); defaults and relaxations are
implemented through [#139] and [#163].

### Host files, mounts, paths, and state

A typed bind can expose host content with the target manager's authority. Its
source and target must be absolute, colon-free, and lexically normalised; binds
default read-only, protected virtual sources fail, and writable access requires
explicit `readOnly = false`. Shared validation rejects protected targets,
duplicates, and ancestor or descendant overlaps across binds, managed volumes,
and tmpfs, including every target equal to, above, or below `/nix/store`.
Managed volumes are separate typed runtime-owned storage, with literal named
resources remaining dangerous sharing authority. Graft still does not attest a
bind source's existence, ownership, permissions, type, or symlink traversal. An
allowed source can therefore expose a device, socket, or store path at another
target, and a writable bind lets a compromised workload alter host-owned data
within its runtime authority. Dedicated direct-device fields stay deferred
because pure resolution cannot attest their activation-time type. Qualified CDI
references do not attest the effective resources in the host spec; a spec may
add devices, mounts, environment values, or hooks with the selected target's
runtime authority.

Overlay writes are disposable runtime state. Explicitly mounted persistent data
needs separate permissions, backup, integrity, and retention policy. Graft does
not currently inspect, diff, promote, back up, or securely erase it.

### Credentials and sensitive output

Do not put secrets in TOML, `config.container.environment`, command arguments,
or generated text. Resolved JSON and generated source can be stored in readable
Nix-store paths, and process environments or runtime metadata are not a robust
secret boundary. Each `environmentFile` entry passes one host path value to
Quadlet, which resolves relative paths against the source-unit directory before
passing them to Podman. Graft neither provisions the file nor attests traversal,
symlinks, existence, permissions, ownership, lifecycle, or disclosure behavior.

Typed secret and credential materialisation is unavailable; `config.secrets`
fails closed. Design and implementation remain in [#143] and [#166]. Until then,
operators must provide any external credential mechanism and assess its Podman,
systemd, process, logging, and mount exposure independently.

### Network exposure and communication

Absence of a network mode preserves Podman's target-specific default; it is not
an egress or firewall policy. Published-port strings are explicit and line-safe,
but Graft does not configure the host firewall, constrain bind addresses beyond
the supplied string, or attest reachability. Shared network namespaces share
interfaces, routes, loopback, and port space.

`mode = "none"` removes external IP connectivity in the tested runtime, but it
does not block mounted Unix sockets, devices, inherited host resources, or
kernel attacks. Host networking remains unavailable dangerous intent. Broader
network and egress policy is tracked by [#193].

### Workload and systemd relationships

Graft validates workload graph identity, target, enablement, lifecycle, and
cycles inside the explicit source set. It cannot inspect application readiness.
An `externalUnit` can activate the exact named unit in the selected manager; for
a system target this may activate a host unit. Graft does not validate that
unit's implementation, authorization, drop-ins, or relationships outside the
Graft graph.

The public workload name, Quadlet filename stem, and `ContainerName=` are not
yet one identity. The resolver carries an explicit mapping and rejects
collisions in its known source set, but operators must keep names aligned until
[#107] defines the final contract.

Other Quadlet search paths or systemd drop-ins can shadow or alter generated
behavior. Existing generator checks validate Graft's own complete fixture set,
not arbitrary host-local overrides. Detection remains in [#171].

### Availability and resource exhaustion

Current workloads have no Graft-enforced CPU, memory, PID, shared-memory, or
ulimit defaults. A workload can consume resources available to its target,
create excessive output, loop, or repeatedly fail under an operator-selected
restart policy. Resource controls are tracked by [#145]. Host-level cgroups,
storage quotas, monitoring, and recovery remain operator policy.

Graft validates cycles among known Graft workload and shared-network edges.
External units can introduce additional systemd transactions or cycles that are
visible only to the generator or manager.

### Build and supply chain

The repository pins Nix inputs and GitHub Actions and runs dependency, workflow,
secret, test, generator, and documentation gates. A host still chooses its Graft
and nixpkgs revisions. Package code executes inside the workload; build scripts
execute under the host's Nix build policy. A compromised package, binary cache,
Nix daemon, upstream runtime, CI platform, or trusted release can violate this
model.

Graft performs no package-malware analysis and makes no reproducibility claim
beyond what the selected Nix inputs and builders provide. Runtime image pulls
are absent from `rootfs-store`, but that does not remove build-time supply-chain
risk. Rootfs construction fails on package file collisions, rejects package
content at Graft-owned runtime `/etc` entries, and propagates package `/etc`
copy failures. Shared package directories may merge, but package order does not
select a collision winner.

## Deployment-context assumptions

| Context | Required assumption | Current boundary |
| --- | --- | --- |
| Local development | The operator reviews selected TOML and package intent. Repository code and data processed inside the workload may be untrusted. | Prefer an explicit user target under a non-root account. Current Graft has no automatic workspace mount or interactive shell contract; explicit volumes carry their own host-file risk. The future baseline uses field-specific opt-outs rather than a development profile. |
| Unattended server | Host administrators own account, UID, linger, authentication, firewall, updates, logging, storage, and recovery policy. | Rootless under a non-root account is preferred; secure defaults are implemented, but per-container identities are not. Early-alpha Graft is not a strong production isolation claim. |
| Remote deployment | Any transport, credentials, host selection, approval, rollback, and remote Nix activation are trusted external tooling. | Graft has no remote deployment control plane yet; design and implementation remain in [#161] and [#174]. |
| Temporary agents | Hostile code would require strict identity, mount, network, secret, TTL, cleanup, concurrency, and resource contracts. | Those contracts are not implemented. Do not treat current containers as disposable-agent isolation; use a VM when a shared kernel is insufficient. See [#151], [#153], and [#169]. |

## Accepted residual risks and non-guarantees

For the current alpha, Graft explicitly does not guarantee:

- VM-equivalent isolation or protection from host-kernel/runtime compromise;
- safe execution of unreviewed selected TOML or untrusted system/rootful or
  root-owned user-target workloads;
- isolation from other processes running as the same rootless host account;
- per-container UID/GID isolation, host-source attestation, resource limits,
  secret transport, or egress control;
- safety, availability, or contents of host-managed CDI specs and resources;
- safety, existence, or behavior of explicitly named external systemd units;
- protection from host-local Quadlet shadowing or systemd drop-ins;
- confidentiality of TOML, resolved JSON, commands, environment values, or
  generated Nix-store text;
- persistence of overlay writes, backup of mounted data, or safe promotion;
- a remote-deployment or temporary-agent security boundary; or
- availability against resource exhaustion or a compromised trusted computing
  base.

These are boundaries, not invitations to add raw escape hatches. New support
must remain typed, reviewable, fail closed, and explicit about which invariant
it changes.

## Requirements for future security-sensitive work

A security-sensitive design or implementation must:

1. identify the affected `GRAFT-TM-*` invariants and trust boundaries;
2. state whether it narrows or deliberately expands authority;
3. classify intent under the [Capability policy](capability-policy.md) as
   first-class, dangerous, or forbidden;
4. expose effective defaults and relaxations in resolved/inspectable output;
5. cover system/rootful, non-root user/rootless, and root-owned user/rootful
   manager contexts separately;
6. add negative tests for accidental activation, injection, target crossing, and
   incompatible combinations; and
7. update this model when assumptions or accepted residual risks change.

Qualified CDI references are current through [#203], secure defaults and typed
relaxations through [#163], and the [filesystem policy](filesystem-policy.md)
through [#164]. Direct devices remain deferred pending host-aware attestation.
Identity and rootfs-integrity gaps are tracked
by [#107] and [#108]. Related isolation,
mount, secret, resource, shadowing, remote, and temporary-agent work is linked in
the risk sections above.

Suspected violations of these boundaries must follow the private
[security reporting policy][security-policy], not a public issue.

[#107]: https://github.com/Patrick-Kappen/graft/issues/107
[#108]: https://github.com/Patrick-Kappen/graft/issues/108
[#139]: https://github.com/Patrick-Kappen/graft/issues/139
[#143]: https://github.com/Patrick-Kappen/graft/issues/143
[#145]: https://github.com/Patrick-Kappen/graft/issues/145
[#151]: https://github.com/Patrick-Kappen/graft/issues/151
[#153]: https://github.com/Patrick-Kappen/graft/issues/153
[#161]: https://github.com/Patrick-Kappen/graft/issues/161
[#163]: https://github.com/Patrick-Kappen/graft/issues/163
[#164]: https://github.com/Patrick-Kappen/graft/issues/164
[#166]: https://github.com/Patrick-Kappen/graft/issues/166
[#169]: https://github.com/Patrick-Kappen/graft/issues/169
[#171]: https://github.com/Patrick-Kappen/graft/issues/171
[#174]: https://github.com/Patrick-Kappen/graft/issues/174
[#193]: https://github.com/Patrick-Kappen/graft/issues/193
[#203]: https://github.com/Patrick-Kappen/graft/issues/203
[#240]: https://github.com/Patrick-Kappen/graft/issues/240
[#242]: https://github.com/Patrick-Kappen/graft/issues/242
[#245]: https://github.com/Patrick-Kappen/graft/issues/245
[activation-test]: https://github.com/Patrick-Kappen/graft/blob/main/tests/nixos/activation.nix
[ci-source]: https://github.com/Patrick-Kappen/graft/blob/main/.github/workflows/ci.yml
[closure-test]: https://github.com/Patrick-Kappen/graft/blob/main/tests/nixos/closure.nix
[flake-source]: https://github.com/Patrick-Kappen/graft/blob/main/flake.nix
[home-manager-source]: https://github.com/Patrick-Kappen/graft/blob/main/modules/home-manager.nix
[materialiser-source]: https://github.com/Patrick-Kappen/graft/blob/main/modules/lib/materialise-containers.nix
[network-test]: https://github.com/Patrick-Kappen/graft/blob/main/tests/runtime/network.sh
[nixos-source]: https://github.com/Patrick-Kappen/graft/blob/main/modules/nixos.nix
[renderer-source]: https://github.com/Patrick-Kappen/graft/blob/main/modules/lib/render-quadlet.nix
[resolve-source]: https://github.com/Patrick-Kappen/graft/blob/main/crates/graft/src/resolve.rs
[schema-source]: https://github.com/Patrick-Kappen/graft/blob/main/crates/graft/src/config/schema.rs
[schema-tests]: https://github.com/Patrick-Kappen/graft/blob/main/crates/graft/tests/schema.rs
[security-policy]: https://github.com/Patrick-Kappen/graft/blob/main/SECURITY.md
