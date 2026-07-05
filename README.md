# graft

**Declarative Podman/Quadlet containers backed by the Nix store — no Dockerfile, no image registry.**

`graft` is to containers what direnv is to shell environments: describe what you want in a small TOML file and the tool makes it happen. Packages are resolved as Nix store closures, a Quadlet unit is written, and systemd manages the lifecycle. Nothing to build, nothing to push, nothing to pull.

The container's root filesystem is a minimal rootfs with `/nix/store` mounted read-only. Commands run from absolute Nix store paths — content-addressed, binary-cached, reproducible.

---

## Why

| The alternatives | The problem |
|---|---|
| Dockerfile + registry | You own the image, the registry, the update pipeline |
| `nix-shell` / `nix develop` | Great isolation of dependencies, but still runs on the host — no process or filesystem isolation |
| NixOS containers | System-level, not practical for per-project or per-agent use |

**graft fills the gap:**

- **Pinned by Nix** — exact store paths, no surprise updates, auditable.
- **Fully isolated** — nothing leaks to or from the host PATH.
- **systemd-managed** — start, stop, status, journal like any other service.
- **TOML composition** — containers inherit from parents and override package sets.
- **Review before commit** — the agent writes to a shadow copy; you decide what goes back.

---

## How it works

```toml
# agent.toml
version = 1
name    = "agent"

[config.runtime]
mode     = "rootfs-store"
packages = ["claude-code"]
command  = ["claude"]

[config.home]
mode    = "ephemeral"
session = true

[[config.home.shadow]]
source = "~/projects/myapp"
target = "/workspace"
```

```bash
graft start agent.toml   # resolve packages, write Quadlet unit, start service
graft attach agent        # open a tmux session inside the container
graft diff    agent       # show what the agent changed in /workspace
graft promote agent       # copy those changes back to ~/projects/myapp
graft stop    agent
```

The core loop is: **start → work → review → promote**.

Shadow mounts let the container read and write a copy of a host directory. Nothing touches the original until you explicitly promote. Remote sessions work the same way — add `--host <hostname>` to `diff`, `promote`, or `reset` and graft SSHs there and runs the command.

---

## Quick start

You need [Nix](https://nixos.org/) (with flakes enabled) and Podman on Linux.

```bash
# Run directly from the flake — no install needed
nix run github:Patrick-Kappen/graft -- --help

# Or build and put it on your PATH
nix build github:Patrick-Kappen/graft
./result/bin/graft --help
```

Write a `graft.toml` in your project, then:

```bash
graft up      # autodetects graft.toml, .graft.toml, or config.toml
```

For permanent always-on containers, add the NixOS or Home Manager module. Containers are declared once, activated on `nixos-rebuild switch`, and restarted automatically when configuration changes.

```nix
# NixOS
services.graft = { enable = true; configRoot = ./containers; };

# Home Manager
programs.graft = { enable = true; configRoot = ./containers; };
```

---

## Documentation

| | |
|---|---|
| [Getting started](docs/getting-started.md) | Full walkthrough from zero to a running container |
| [CLI reference](docs/cli.md) | Every command, flag, and environment variable |
| [Configuration reference](docs/reference.md) | Every TOML field with defaults and examples |
| [Config notes](docs/config.md) | Home, workspace, shadow mounts, sessions, attach |
| [NixOS module](docs/nixos-module.md) | System-scoped managed containers |
| [Home Manager module](docs/home-manager.md) | Rootless user containers |
| [Design](docs/design.md) | Architecture and key decisions |
| [Security model](docs/security.md) | Threat model and isolation guarantees |
| [Security roadmap](docs/security-roadmap.md) | Planned hardening |

---

## Contributing & security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE).
