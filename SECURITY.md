# Security policy

`graft` is early-stage software. Please do not treat it as a hardened security boundary yet.

## Reporting vulnerabilities

Please report suspected vulnerabilities privately to the maintainer before opening a public issue.

Include:

- affected commit/version;
- configuration/TOML needed to reproduce;
- impact;
- suggested fix if known.

## Current security posture

The project aims for explicit, reviewable security policy. It does not apply hidden hardening defaults.

Known important caveats:

- rootfs-store currently mounts `/nix/store` read-only;
- CLI graph resolution is intentionally blocked for unresolved parents/children until the resolver is shared;
- rootless Podman is preferred for user containers;
- secret contents must not be placed in TOML or the Nix store.
