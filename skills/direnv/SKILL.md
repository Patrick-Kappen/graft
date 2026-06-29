---
name: direnv
description: Use when Pi needs local tools or a persistent Nix dev shell that should not be committed to the project repo.
---

# Pi Direnv

Use the `direnv` tool for local agent tooling.

Rules:

- Add local tools with `direnv action=add target="<pkg...>"`.
- Do not edit tracked project files for local-only tools.
- Normal bash commands run through the local Nix shell after the extension is loaded.
- Generated local files:

```text
.pi-direnv/
.envrc
.direnv/
```

These are machine-local and should stay out of Git/JJ.
