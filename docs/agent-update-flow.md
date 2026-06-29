# Agent / update flow

The intended update flow for agents, npm packages, and other tools that write to
the workspace is:

```text
real repo/workspace
  → copy candidate workspace
  → ephemeral container (isolated HOME/XDG dirs)
  → run agent / install packages / do one action
  → show diff from candidate workspace
  → optionally promote reviewed changes back to source of truth
  → optionally bake npm packages into the Nix store
```

The real workspace is never mounted writable. The container sees a writable copy;
the host stays untouched until you explicitly promote.

## Configuration

### `[config.workspace]`

```toml
[config.workspace]
mode    = "copy"        # only supported mode today
source  = "."           # host directory to copy (absolute or relative to CWD)
target  = "/workspace"  # mount point inside the container
review  = "diff"        # print a diff after the container exits
promote = "prompt"      # "off" (default) | "prompt" | "auto"

# Override which directories are skipped during the copy.
# Default skip list: .git  .jj  .go  .direnv  result  node_modules
# Omitting "node_modules" includes it in the copy (useful if deps are pre-installed).
# excludePatterns = [".git", ".jj", ".go", ".direnv", "result"]
```

`workspace.promote` controls what happens after the diff is shown:

| value | behaviour |
|---|---|
| `"off"` | show diff and exit (default) |
| `"prompt"` | show diff, ask `Apply? [y/N]`, apply if yes |
| `"auto"` | always apply without asking (useful in CI or autonomous agents) |

"Apply" copies all changed/new files from the workspace candidate back to the real
`source` directory. Files that were deleted inside the container are left untouched
on the host (safe default).

### `[config.home]`

```toml
# Ephemeral HOME — wiped after every run (default, maximum isolation):
[config.home]
ephemeral = true
target    = "/home/agent"

# Persistent HOME — survives across runs (for session state, auth tokens, etc.):
[config.home]
mode   = "persistent"
source = "~/.local/share/graft/sessions/my-agent"   # ~ is expanded
target = "/home/agent"
```

Both modes set `HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_DATA_HOME`, and
`XDG_STATE_HOME` inside the container.

## Full npm-agent lifecycle

### Step 1 — Explore

Run the agent in an isolated workspace. Install npm packages, make changes, explore.

```bash
graft up examples/agent-update.toml
```

Inside the container:

```bash
npm install @anthropic-ai/sdk          # writes package-lock.json + node_modules/
node agent.js                          # agent runs, may modify workspace files
```

After exit, graft shows the diff and asks whether to promote.

### Step 2 — Promote

With `promote = "prompt"`, after the diff you see:

```
Apply workspace changes to /home/zerodawn/my-agent? [y/N] y
graft: applying workspace changes to /home/zerodawn/my-agent
```

The `package-lock.json` (and any other changed files) are now back in your real
workspace. `node_modules` itself is skipped (too large, not needed for the next step).

### Step 3 — Bake into the Nix store

```bash
graft nix-bake ./my-agent
```

This reads `package.json` and `package-lock.json`, runs `prefetch-npm-deps` to
compute the hash of all npm dependencies, and emits a `buildNpmPackage` Nix snippet:

```nix
pkgs.buildNpmPackage {
  pname = "my-agent";
  version = "1.0.0";
  src = ./my-agent;
  npmDepsHash = "sha256-abc123...";
}
```

### Step 4 — Wire up in your flake

```nix
# flake.nix
{
  outputs = { self, nixpkgs, ... }: {
    packages.x86_64-linux.my-agent =
      nixpkgs.legacyPackages.x86_64-linux.callPackage ./my-agent.nix { };
  };
}
```

Then reference it in your graft TOML:

```toml
[config.runtime]
mode     = "rootfs-store"
packages = ["my-agent"]              # now from /nix/store — no npm install at runtime
command  = ["my-agent", "--run"]
```

After `nixos-rebuild switch` or `home-manager switch`, every run of this container
is fully reproducible: all deps come from the Nix store, nothing is installed at
runtime.

## Security boundary

```text
container writes candidate copy + ephemeral or persistent home
                                 ↕  (promote on request)
real workspace — only touched when you say yes
```

Network can be disabled (`network.mode = "none"`) for the main agent run once
packages are in the store. Re-enable it only for the discovery / install step.

## Still missing

- `jj` workspace candidate mode (jujutsu change-based flow);
- structured patch / hunk export;
- PR / branch promote command;
- per-action metadata;
- TUI review.
