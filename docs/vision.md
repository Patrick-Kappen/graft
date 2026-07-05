# Vision: graft as direnv for containers

`graft` should ultimately feel like **direnv for containers**.

Not:

```text
cd project -> host shell gets extra PATH/env
```

but:

```text
cd project -> the project container is started/used
leave/idle -> the container is stopped/stays idle per policy
changes -> pulled from a candidate workspace and staged for review
```

The tool provides the shell and orchestration. Users decide their own policy,
presets, mounts, network, security, and packages through TOML.

## Core value

The gap this project fills:

```text
Nix store-backed runtime closures
+ Podman/Quadlet lifecycle
+ container-only tools
+ a TOML graph for composition
+ fast, dynamic project containers
+ optional promotion to permanent config
```

Without requiring:

- a Dockerfile;
- an OCI image build;
- an image pull;
- host PATH pollution;
- hand-written Quadlet units;
- a NixOS rebuild for the fast/project path.

## Product definition

```text
graft = direnv for Podman/Quadlet containers, backed by Nix store closures
```

For a project:

```text
project repo
  graft.toml / .graft.toml / config.toml
  flake.nix / flake.lock optional
```

`graft` can then:

1. autodetect TOML;
2. resolve the parent/child graph;
3. realise runtime packages in `/nix/store`;
4. render a minimal rootfs-store container;
5. start Quadlet transient or persistent;
6. isolate a workspace via copy/jj candidate;
7. on leave/idle, collect changes for review;
8. later promote a working configuration to a repo/branch/PR.

## Key principle

Empty means nothing.

```toml
version = 1
name = "example"

[config]
# no-op
```

No implicit container, mount, network, security policy, or package.

## TOML is the source of truth

TOML ultimately sets everything:

- container name;
- runtime mode;
- packages;
- command;
- mounts;
- filesystem flags;
- network;
- security;
- resources;
- session/idle policy;
- parent/child relations;
- deploy/promote metadata.

NixOS/Home Manager should stay short and only point at a directory:

```nix
services.graft = {
  enable = true;
  configRoot = ./containers;
};
```

A TOML from `configRoot` becomes NixOS-managed only if it explicitly enables
deployment:

```toml
[deploy]
enable = true
target = "system"
```

For Home Manager / rootless user Quadlet:

```nix
programs.graft = {
  enable = true;
  configRoot = ./containers;
};
```

Home Manager only deploys TOML with:

```toml
[deploy]
enable = true
target = "user"
```

The content lives in TOML, not in Nix options.

## Nix store runtime, not host install

Packages do not need to be installed on the host `PATH`.

```text
package in /nix/store        yes
package on host PATH         no
package on container PATH    yes
```

Example:

```toml
[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils", "go", "gopls"]
command = ["bash", "-lc", "go test ./..."]
```

Nix realises the package closures in `/nix/store`. The container gets a runtime
env on `PATH`, but the host shell does not.

## No image build needed for the fast flow

For `rootfs-store`, no container image is built.

What is produced:

```text
/nix/store/...-runtime
/nix/store/...-minimal-rootfs
/generated Quadlet .container
```

If store paths already exist or come from a binary cache, this is fast. If not,
Nix builds/downloads only the missing closures.

Later, OCI/image mode can still exist for distribution or non-Nix hosts.

## Fast versus permanent

There are two main routes.

### Fast project flow

No NixOS rebuild.

```bash
graft up ./graft.toml
```

Or autodetect:

```bash
graft up
```

Flow:

```text
TOML in project
  -> resolve/build runtime closure
  -> temporary Quadlet in $XDG_RUNTIME_DIR/containers/systemd
  -> systemctl --user start
  -> container runs
```

### Permanent / promoted flow

For containers you use more often:

```text
effective/project config
  -> graft promote
  -> new TOML in infra/NixOS/Home Manager repo
  -> jj branch / PR / review / merge
  -> NixOS/HM managed Quadlet
```

So fast experimentation stays transient. Making something permanent happens
through reviewable repo changes.

## Parent/child and overrides

The goal is composable containers.

Base container:

```toml
# base/server.toml
version = 1
name = "base/server"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils", "appA", "appB", "appC"]
```

Project override:

```toml
# projects/my-app.toml
version = 1
name = "my-app"

[parents]
add = ["base/server"]

[config.runtime.packageOps]
remove = ["appX"]
add = ["appZ"]

[[config.runtime.packageOps.replace]]
name = "appY"
with = "appY_pinned"
```

The project does not have to copy the whole base container. It can reference the
parent and declare only the differences.

## Package operations and pins

Current package operation model:

```toml
[config.runtime.packageOps]
remove = ["appX"]
add = ["appZ"]

[[config.runtime.packageOps.replace]]
name = "appY"
with = "appY_pinned"
```

Goal:

```text
base packages
  - appX
  replace appY
  + appZ
```

Version pinning can use flake locks/refs. Store paths only need to be realised;
they do not need to live in host profiles.

## Candidate workspace

For safe project mutations or isolated workloads:

```text
real workspace
  -> candidate copy / jj workspace
  -> container gets the candidate writable
  -> container works there
  -> leave/idle exports the diff/change
  -> review/apply/discard
```

Rule:

```text
the real workspace is never automatically writable in the container
```

Possible workspace modes:

```toml
[workspace]
mode = "jj"       # jj | copy | none
target = "/workspace"
review = "patch"  # patch | jj-change
```

## Session lifecycle

Later `graft` should manage sessions.

Manual first:

```bash
graft enter
graft leave
graft status
graft review
graft apply
graft discard
```

Then a shell hook:

```bash
eval "$(graft hook zsh)"
```

Behaviour:

```text
enter directory with graft.toml -> start/reuse container
leave directory                 -> stop/keep/review according to policy
idle timeout                    -> stop/keep/review according to policy
```

TOML direction:

```toml
[session]
mode = "ephemeral"     # ephemeral | persistent | hybrid
idleTimeout = "30m"
leaveAction = "review" # review | keep | discard | stop
```

Persistent mode can keep containers running idle for longer.

## Security model

The project ships no hidden policy. Docs may offer suggestions.

A user can build their own locked parent:

```toml
[config.filesystem]
readOnly = true
readOnlyTmpfs = true
tmpfs = ["/tmp", "/run", "/home/user"]

[config.network]
mode = "none"

[config.security]
dropCapabilities = ["all"]
noNewPrivileges = true
userns = "keep-id"
```

For speed, the first version often mounts all of `/nix/store` read-only. A
`closure-only` store access mode can be investigated later.

```toml
[config.runtime]
storeAccess = "full-readonly" # later: closure-only
```

## Directories and autodetect

`graft up` without an argument tries, in the current directory:

```text
graft.toml
.graft.toml
config.toml
```

The NixOS module can already resolve `parents.*` and `children.*` from
`configRoot`.

## Current implementation status

Present now:

- Go CLI;
- the `graft` binary;
- TOML loader with strict unknown-field checks;
- `inspect`, `render`, `render-nixos`, `run`, `up`;
- no-op detection;
- rootfs-store Quadlet renderer;
- transient `systemctl --user` run;
- Nix package build;
- NixOS module with `configFiles`, recursive `configRoot` discovery, and
  `parents.*`/`children.*` resolving;
- Home Manager module with the same resolver for rootless/user Quadlet;
- effective TOML generation during the NixOS/HM build;
- TOML `runtime.packages` -> `pkgs.<name>` in the NixOS/HM modules;
- examples and docs.

Still to build:

- package refs beyond simple `pkgs.<name>` strings;
- session state;
- workspace copy/jj candidate flow;
- promote branch/PR flow;
- persistent user Quadlet mode;
- idle/leave lifecycle.
