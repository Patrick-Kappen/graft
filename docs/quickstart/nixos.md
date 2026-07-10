# NixOS system-container quickstart

This example adds one rootful, system-scoped Graft container to an existing
NixOS flake. The reusable files for this path are under
`examples/quickstart/nixos/`: `module.nix` and
`containers/graft-example.toml`. Copy them into that flake, or use them as a
reference for an existing host module.

## Prerequisites

The host must already have NixOS, systemd, and a Podman package with Quadlet
support. The example enables Podman with:

```nix
virtualisation.podman.enable = true;
```

Graft does not configure firewall rules, DNS, users, linger, or other host
policy. The generated service runs in the system systemd manager as rootful
Podman.

## Add Graft to the flake

In the host flake's `inputs`:

```nix
inputs.graft.url = "github:Patrick-Kappen/graft";
```

Import `examples/quickstart/nixos/module.nix` from the NixOS host module. The
exported `inputs.graft.nixosModules.graft` module supplies the Graft package by
default. Copy `examples/quickstart/nixos/containers/graft-example.toml` to the
`containers/` directory relative to that module, or update `configRoot` to the
copied location.

The example uses only the public `bash` package from the host's pinned
nixpkgs. Graft also adds its built-in `graft-pause` package.

## Activate and inspect

From the host repository:

```bash
git add path/to/module.nix path/to/containers/graft-example.toml
sudo nixos-rebuild switch --flake .#your-host
sudo systemctl daemon-reload
sudo systemctl start graft-example.service
sudo systemctl status graft-example.service
sudo journalctl -u graft-example.service --no-pager
```

The service should log:

```text
graft-example-ready
```

The generated Quadlet file is named `graft-example.container`, and the
resulting systemd service is `graft-example.service`. The TOML filename stem and
`name` intentionally match; see [issue #107](https://github.com/Patrick-Kappen/graft/issues/107)
for the future identity contract.

The relevant generated Quadlet output is:

```ini
[Container]
ContainerName=graft-example
Rootfs=/nix/store/...-graft-graft-example-env:O
Exec="bash" "-c" "echo graft-example-ready; exec /bin/graft-pause"
Volume=/nix/store:/nix/store:ro
WorkingDir=/workspace
Environment="GRAFT_EXAMPLE=nixos-system"
```

To stop the workload:

```bash
sudo systemctl stop graft-example.service
```

Stopping removes the runtime container, but leaves the generated Quadlet file.
The current `Rootfs=...:O` overlay is not a persistent promote mechanism.
