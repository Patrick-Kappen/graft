# Changelog

## Unreleased
- fix(cli): systemctl uses --user for non-root, system scope for root — matches Quadlet deploy target
- feat(modules): expose configRoot as GRAFT_SYSTEM_CONTAINERS env var so CLI can find base TOMLs for merge
- docs: document container resolution flow (3 cases), write locations, base TOML lookup via env var
- Add CLI skeleton: `graft shell <name>` with trait-based ContainerRuntime, clap, anyhow, tracing; 4 unit tests passing
- Add complete TOML config schema (`schema.rs`) and `TomlConfig` provider reading `.graft/containers/<name>.toml`; 9 unit tests passing
