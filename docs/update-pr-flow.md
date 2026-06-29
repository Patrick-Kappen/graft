# Base container and updates via PR

Historical note, now part of [`vision.md`](vision.md): fast containers can later
be promoted to TOML in a repo/branch/PR.

Goal: updates to a runtime/profile/agent never happen directly on the live
environment. An update runs in a temporary container/candidate and the result is
offered as a reviewable change, preferably as a PR.

## Base container

The base container is declarative and minimal:

```text
empty Podman rootfs
+ /nix/store read-only
+ flake .#minimal-runtime
+ explicit mounts per mode
```

The base runtime is in `flake.nix` as:

```text
minimal-runtime = bashInteractive + coreutils
```

A second runtime can sit next to it later, for example:

```text
pi-runtime = bash + coreutils + node + pi + npm + jj + rg
```

But the container boundary stays the same.

## Update flow

A Pi update or extension update happens in a candidate:

```text
current profile / lock
  ↓ copy
candidate profile
  ↓ temporary update container
pi update / npm install / pi install
  ↓
diff candidate vs current
  ↓
PR/review
  ↓
promote only after approval
```

## No direct mutation

Do not run:

```bash
pi update --extensions
```

on the real host or the real profile.

Instead run:

```bash
graft update-profile default -- pi update --extensions
```

or:

```bash
graft update-runtime
```

Those commands should always:

1. create a temporary/candidate directory;
2. run the update in a container;
3. show the result as a diff;
4. prepare a branch/PR;
5. activate only after review/promote.

## PR model

For reproducibility, capture the output of an update in the repo/dotfiles, for
example:

```text
profiles/default/settings.json
profiles/default/package-lock.json
profiles/default/npm-snapshot/
flake.lock
```

An update becomes a normal VCS change:

```text
update/pi-default-2026-06-29
```

containing only the explicitly promoted changes.

## First implementation steps

1. Keep `graft-plain-shell` as a minimal container smoke test.
2. Build a Nix package/app `graft` instead of loose scripts.
3. Add a `pi-runtime` next to `minimal-runtime`.
4. Add `graft update-runtime` for `flake.lock` updates.
5. Add `graft update-profile <name>` for Pi/npm/profile updates.
6. Make both flows produce candidate output only, never mutating the live
   environment directly.

## Important principle

The container may be mutable during the update. The live environment must not be.

```text
mutable in tmp/candidate
immutable/reviewed in repo
active only after promote
```
