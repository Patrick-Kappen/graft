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

Copy `examples/quickstart/nixos/module.nix` and its `containers/` directory into
the host repository. Add both the exported Graft module and the copied module to
the host's `modules` list:

```nix
outputs = { graft, nixpkgs, ... }: {
  nixosConfigurations.your-host = nixpkgs.lib.nixosSystem {
    modules = [
      ./configuration.nix
      graft.nixosModules.graft
      ./path/to/module.nix
    ];
  };
};
```

The exported module supplies the Graft package by default. No `specialArgs`
wiring is required by the copied module. Keep its `containers/graft-example.toml`
relative to `module.nix`, or update `configRoot` to the copied location.

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
Environment="GRAFT_EXAMPLE=nixos-system"
```

To stop the workload:

```bash
sudo systemctl stop graft-example.service
```

Stopping removes the runtime container, but leaves the generated Quadlet file.
The current `Rootfs=...:O` overlay is not a persistent promote mechanism.

## Remove the example

Remove `./path/to/module.nix` from the host's `modules` list, then remove the
copied module and TOML declaratively:

```bash
git rm path/to/module.nix path/to/containers/graft-example.toml
sudo nixos-rebuild switch --flake .#your-host
sudo systemctl daemon-reload
```

After activation, `graft-example.service` is no longer generated. Removing only
the TOML while leaving `configRoot` pointed at an untracked empty directory can
make that directory invisible to a Git flake.
