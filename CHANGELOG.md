# Changelog

## Unreleased
- feat(cli): resolve TOML configs to JSON stdout with graft-pause defaults
- docs: clarify TOML→CLI→JSON→NixOS flow and remove stale defaults
- refactor(cli): remove all runtime code — CLI is build-time TOML→config generator only
- fix(cli): systemctl uses --user for non-root, system scope for root — matches Quadlet deploy target
- feat(modules): expose configRoot as GRAFT_SYSTEM_CONTAINERS env var so CLI can find base TOMLs for merge
- docs: document container resolution flow (3 cases), write locations, base TOML lookup via env var
- Add CLI skeleton: `graft shell <name>` with trait-based ContainerRuntime, clap, anyhow, tracing; 4 unit tests passing
- Add complete TOML config schema (`schema.rs`) and `TomlConfig` provider reading `.graft/containers/<name>.toml`; 9 unit tests passing
