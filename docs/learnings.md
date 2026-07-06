# Learnings

- Always add `.gitignore` before the first `cargo build` — `cli/target/` got committed and needed a cleanup commit to remove it
- Factor out pure parsing (`parse_toml`) from file I/O (`load_from_path`) so tests don't need temp files for the happy path
- Use `environment.variables` (NixOS) / `home.sessionVariables` (HM) to pass build-time config to the CLI — no wrapper scripts, no extra files
