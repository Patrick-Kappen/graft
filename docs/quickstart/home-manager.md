# Home Manager user-container quickstart

This example adds one rootless, user-scoped Graft container to an existing
non-root Home Manager configuration. The reusable files for this path are under
`examples/quickstart/home-manager/`: `module.nix` and
`containers/graft-example.toml`. Copy them into that configuration, or use them
as a reference for an existing Home Manager module.

## Prerequisites

The host must already provide Linux, a non-root Home Manager account, systemd
user services, and a Podman package with Quadlet support for rootless containers.
Rootless overlay support such as `fuse-overlayfs` may also be required by the
host's Podman setup. Graft's user target does not reject UID 0; a root-owned user
manager runs Podman rootful and is outside this rootless quickstart.

Graft does not enable Podman, install `fuse-overlayfs`, enable user linger,
configure firewall/DNS rules, or mutate other host policy. The generated service
runs in the user's systemd manager through rootless Podman. If it must run
without an active login session, configure linger separately on the host.

## Add Graft to the flake

In the host flake's `inputs`:

```nix
inputs.graft.url = "github:Patrick-Kappen/graft";
```

Copy `examples/quickstart/home-manager/module.nix` and its `containers/`
directory into the Home Manager repository. Add both the exported Graft module
and the copied module to the Home Manager `modules` list:

```nix
outputs = { graft, home-manager, nixpkgs, ... }: {
  homeConfigurations."your-user" = home-manager.lib.homeManagerConfiguration {
    pkgs = nixpkgs.legacyPackages.x86_64-linux;
    modules = [
      ./home.nix
      graft.homeManagerModules.graft
      ./path/to/module.nix
    ];
  };
};
```

Adjust the package system for the target host. The exported module supplies the
Graft package by default. No `extraSpecialArgs` wiring is required by the copied
module. Keep its `containers/graft-example.toml` relative to `module.nix`, or
update `configRoot` to the copied location.

The example uses only the public `bash` package from the host's pinned
nixpkgs. Graft also adds its built-in `graft-pause` package.

## Activate and inspect

From the host repository:

```bash
git add path/to/module.nix path/to/containers/graft-example.toml
home-manager switch --flake .#your-user
systemctl --user daemon-reload
systemctl --user start graft-example.service
systemctl --user status graft-example.service
journalctl --user -u graft-example.service --no-pager
```

The service should log:

```text
graft-example-ready
```

The generated Quadlet file is named `graft-example.container`, and the
resulting user systemd service is `graft-example.service`. The TOML filename
stem and `name` intentionally match; see [issue #107](https://github.com/Patrick-Kappen/graft/issues/107)
for the future identity contract.

The relevant generated Quadlet output is:

```ini
[Container]
ContainerName=graft-example
Rootfs=/nix/store/...-graft-graft-example-env:O
Exec="bash" "-c" "echo graft-example-ready; exec /bin/graft-pause"
Volume=/nix/store:/nix/store:ro
Environment="GRAFT_EXAMPLE=home-manager-user"
```

To stop the workload:

```bash
systemctl --user stop graft-example.service
```

Stopping removes the runtime container, but leaves the generated Quadlet file.
The current `Rootfs=...:O` overlay is not a persistent promote mechanism.

## Remove the example

Remove `./path/to/module.nix` from the Home Manager `modules` list, then remove
the copied module and TOML declaratively:

```bash
git rm path/to/module.nix path/to/containers/graft-example.toml
home-manager switch --flake .#your-user
systemctl --user daemon-reload
```

After activation, `graft-example.service` is no longer generated. Removing only
the TOML while leaving `configRoot` pointed at an untracked empty directory can
make that directory invisible to a Git flake.
