# Contributing to Graft

Graft is early-alpha software with strict boundaries between TOML intent, CLI
resolution, Nix materialisation, Quadlet output, systemd lifecycle, and Podman
runtime behavior. Focused bug fixes, tests, documentation corrections, and
design feedback are welcome.

## Before making changes

- Search the [open issues](https://github.com/Patrick-Kappen/graft/issues) and
  the [roadmap](docs/roadmap.md) for existing work.
- Open or discuss an issue before adding product behavior, changing an existing
  contract, or expanding security-sensitive capability.
- Keep one issue and one focused branch per change.
- Do not turn TOML into unrestricted Nix, Quadlet, systemd, or Podman
  passthrough.
- Do not add hidden state or move business logic into the Nix modules.

Security vulnerabilities must not be discussed in a public issue. Follow the
[security policy](SECURITY.md) instead.

## Development workflow

1. Read the issue and relevant architecture, reference, and non-goal docs.
2. Confirm the intended scope before implementation.
3. Add tests for successful behavior, failures, and meaningful edge cases.
4. Update every affected public example and manual page.
5. Run the relevant local checks.
6. Open a focused pull request that links the issue and lists validation run.
7. Address review comments without mixing unrelated cleanup into the change.

The canonical commands and renderer checklists live in the
[Development guide](docs/development.md). Use its Rust, Nix, documentation,
security, and workflow checks as applicable. In particular:

- Rust changes require tests and clippy with warnings denied.
- Nix module changes require both NixOS and Home Manager coverage where the
  behavior is shared.
- IFD-backed module checks must be built explicitly, not inferred from
  `nix flake check` alone.
- Runtime-sensitive Quadlet behavior needs generator/systemd verification and,
  where practical, a real system or user runtime check.
- Public documentation must not contain private hosts, paths, endpoints,
  repositories, credentials, or maintainer-only procedures.

## Pull requests

A pull request should explain:

- the problem and chosen scope;
- user-visible or architectural effects;
- tests and validation performed;
- known limitations or follow-up issues.

Keep generated artifacts, build outputs, credentials, unrestricted diagnostic
dumps, and local maintainer files out of commits.

By contributing, you agree that your contribution is licensed under the
repository's [Apache-2.0 license](LICENSE).
