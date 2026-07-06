# Learnings

- Always add `.gitignore` before the first `cargo build` — `cli/target/` got committed and needed a cleanup commit to remove it
- Factor out pure parsing (`parse_toml`) from file I/O (`load_from_path`) so tests don't need temp files for the happy path
- Use `environment.variables` (NixOS) / `home.sessionVariables` (HM) to pass build-time config to the CLI — no wrapper scripts, no extra files
- systemctl scope (system vs --user) is determined by UID, not TOML — mirrors how systemd itself works and keeps the CLI stateless
- Build scope and runtime scope are completely separate — do not mix TOML→config generation with container start/exec logic in the same CLI
- Verify design decisions against recent discussion before updating docs — older handoffs can contain stale defaults
- After adding a second Rust binary, use `cargo run --bin graft` because Cargo can no longer infer the binary
- Use `lib.getExe'` for multi-binary packages in Nix modules so evaluation does not depend on `meta.mainProgram`
- Mirror NixOS and Home Manager module refactors in the same pass to avoid stale TOML defaults in one module
