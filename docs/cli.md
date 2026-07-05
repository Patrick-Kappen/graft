# CLI

Run `graft --help` for a full usage summary; `graft --version` prints the
version.

## Concepts

### Blueprint vs instance

A TOML file is a **blueprint** — a reusable definition of a container. It has
no deploy target and its `name` field identifies it in the graph, not in systemd.

An **instance** is a running container with a unique name. Instance names carry
a suffix so multiple instances of the same blueprint can coexist:

```text
my-app.toml        ← blueprint (in git)
my-app-1           ← instance 1 (in systemd)
my-app-work        ← instance 2 (in systemd)
```

### Two paths

**Managed path (production):** TOML files are committed to git and processed
by the NixOS or Home Manager module at `nixos-rebuild` time. Quadlet units land
in `/etc/containers/systemd` or `~/.config/containers/systemd`. The CLI then
operates on those pre-deployed instances — it does not read TOML at runtime.

**Dev path (test before rebuild):** `graft run` reads a TOML directly, renders
a transient Quadlet unit, and starts it. You name the instance explicitly with
`--as`. Use this to validate a blueprint locally before committing and rebuilding.

### Remote hosts

All managed-path commands accept a `--host` flag for SSH remote operation:

```bash
graft --host myserver up my-app-1
graft --host myserver logs my-app-1
graft --host myserver attach my-app-1
```

---

## Managed path — operating instances

These commands assume the container unit was deployed by `nixos-rebuild` or
`home-manager switch`. They do not read TOML files.

### `graft up <instance>`

Start a deployed container:

```bash
graft up my-app-1
```

Equivalent to `systemctl --user start my-app-1.service`. If the unit does
not exist, graft exits with an error — deploy it first via `nixos-rebuild`.

If the container was started with `home.mode = "session"` or shadow mounts,
graft sets up the per-session directories before starting.

### `graft down <instance>`

Stop a running container:

```bash
graft down my-app-1
```

If the container uses `home.mode = "session"`, the configured review and
promote steps run before stopping (diff output, optional copy back to source).

### `graft attach <instance>`

Attach to an interactive session inside the running container:

```bash
graft attach my-app-1
```

graft tries the following in order:

1. Attach to an existing tmux session (name configured via `[config.attach]`).
2. Start a new tmux session if none exists.
3. Fall back to `exec`-ing the configured shell (`sh` by default) directly.

tmux is optional — if it is not in the container, the shell fallback is used.
The session name and shell are configured via `[config.attach]` in the TOML.

### `graft <instance>`

Start-or-attach in one command. The most common way to interact with a
named container:

```bash
graft my-app-1   # up if not running, then attach
```

### `graft list`

List all running graft-managed containers (labelled `managed-by=graft`):

```bash
graft list
```

### `graft logs <instance> [--denied]`

Show journald output for a container service:

```bash
graft logs my-app-1           # all logs
graft logs my-app-1 --denied  # egress proxy: blocked connections only
```

### `graft stop <instance>`

Stop a container and remove its runtime unit (for transient/dev instances):

```bash
graft stop my-app-1
```

---

## Session and workspace commands

These commands operate on session data (home session and shadow mounts) that
accumulates while a container is running. They only work locally (no `--host`).

### `graft diff <instance>`

Show a diff of all session data compared to the host source:

```bash
graft diff my-app-1
```

Shows:
- Home session diff (if `home.mode = "session"`)
- Shadow mount diffs (one per `[[home.shadow]]` entry)

Use this after the container has run to review what changed before promoting.

### `graft promote <instance> [--path <container-path>]`

Copy shadow mount changes back to the configured host path:

```bash
graft promote my-app-1                   # promote all shadow mounts
graft promote my-app-1 --path /workspace # promote only /workspace
```

Shadow mount changes are isolated in a per-session directory. `promote` copies
them back to the `host` path configured in `[[home.shadow]]`.

Home session promote is automatic (configured via `home.promote`). Use `graft
promote` explicitly only for shadow mounts.

### `graft reset <instance>`

Clear all session data for a container (home session + shadow mounts):

```bash
graft reset my-app-1
```

The next `graft up` starts with a fresh copy from the configured source paths.

---

## Dev path — testing a blueprint

Use `graft run` to validate a blueprint locally before committing and
running `nixos-rebuild`. This is the only path that reads a TOML file at
runtime.

### `graft run <file.toml> --as <instance-name>`

Render and start a transient container from a TOML blueprint:

```bash
graft run my-app.toml --as my-app-1
```

`--as` is required. It sets the instance name — the running container name
and systemd unit stem. The unit is written to
`$XDG_RUNTIME_DIR/containers/systemd` and is removed when the container stops.

Shadow mounts and home session mode work on the dev path too: session dirs are
created in a temporary work directory, and review/promote prompts run when the
container exits.

---

## Scope: user vs system

By default graft targets the **user** systemd scope (`systemctl --user`), which
matches the Home Manager module (`target = "user"`).

For NixOS system-target units (`target = "system"`), set:

```bash
export GRAFT_SYSTEMD_SCOPE=system
```

All managed-path commands (`up`, `down`, `stop`, `logs`, `diff`, `promote`,
`reset`) and the start-or-attach shortcut respect this variable. The dev path
(`graft run --as`) always uses user scope because transient Quadlet units are
written to `$XDG_RUNTIME_DIR`.

---

## Proxy

### `graft proxy serve`

Start the egress proxy engine. This is the command the proxy container itself
runs — set it in the blueprint's `config.runtime.command`. Do not call it
directly on the host.

---

## Plumbing / module support

These commands are used internally by the Nix modules and for debugging. They
read TOML and render Quadlet text but do not start anything.

```text
graft inspect <file.toml>                       Print resolved metadata as JSON
graft render <file.toml>                        Render Quadlet text to stdout
graft render-nixos <file.toml> <rootfs> <name>  Render with concrete store paths
graft render-nixos-units <file.toml> <name> <out-dir>
                                                Render all units to a directory
graft prepare-rootfs <dir>                      Create the minimal container rootfs
graft nix-bake <dir>                            Generate a buildNpmPackage Nix snippet
```

---

## Config management

```text
graft config path          Print the default config file path
graft config init [path]   Write the no-op example config
graft config show [path]   Print the current config file
```

See [config.md](config.md) for the configuration reference.
