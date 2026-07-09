# Graft

<p align="center">
  <img src="assets/graft-banner.png" alt="Graft banner">
</p>

TOML-driven Podman Quadlet containers, built from the Nix store.

Graft turns small TOML files into rootfs-based Podman Quadlet services for NixOS
and Home Manager. You describe container intent; Graft resolves the runtime
details; Nix materialises the rootfs and Quadlet output; systemd runs the result
like any other service.

Use the GitHub README as the repository landing page, then use this manual for
deeper design, reference, and contributor details.

## Start here

- [Overview](overview.md) explains the current architecture and data flow.
- [Long-term vision](vision.md) records the endgame without changing the active
  implementation roadmap.
- [Design](design.md) documents the boundaries between TOML, CLI, Nix modules,
  and Quadlet output.
- [Quadlet output](quadlet.md) describes the generated `.container` files.
- [Roadmap](roadmap.md) describes the active implementation direction.
- [Non-goals and deferred scope](non-goals.md) lists deliberate exclusions.
- [Reference](reference.md) links to the annotated TOML reference and current
  module options.
- [Development](development.md) captures contributor workflow and renderer
  checklists.

## Current scope

The current MVP focuses on `rootfs-store` containers:

- TOML to resolved JSON stdout
- NixOS system/rootful Quadlet output
- Home Manager user/rootless Quadlet output
- manual start/stop through systemd
- packages and commands resolved from TOML
- useful Quadlet rendering for identity, working directory, environment,
  environment files, published ports, volumes, and service timing
- clean shutdown through `graft-pause`
