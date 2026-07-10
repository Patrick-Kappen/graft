# Home Manager user-container quickstart

This example adds one rootless, user-scoped Graft container to an existing Home
Manager configuration. The reusable files for this path are under
`examples/quickstart/home-manager/`: `module.nix` and
`containers/graft-example.toml`. Copy them into that configuration, or use them
as a reference for an existing Home Manager module.

## Prerequisites

The host must already provide Linux, systemd user services, and a Podman package
with Quadlet support for rootless containers. Rootless overlay support such as
`fuse-overlayfs` may also be required by the host's Podman setup.

Graft does not enable Podman, install `fuse-overlayfs`, enable user linger,
configure firewall/DNS rules, or mutate other host policy. The generated service
runs in the user's systemd manager through rootless Podman. If it must run
without an active login session, configure linger separately on the host.

## Add Graft to the flake

In the host flake's `inputs`:

```nix
inputs.graft.url = "github:Patrick-Kappen/graft";
```

Import `examples/quickstart/home-manager/module.nix` from the Home Manager
configuration. The exported `inputs.graft.homeManagerModules.graft` module
supplies the Graft package by default. Copy
`examples/quickstart/home-manager/containers/graft-example.toml` to the
`containers/` directory relative to that module, or update `configRoot` to the
copied location.

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
WorkingDir=/workspace
Environment="GRAFT_EXAMPLE=home-manager-user"
```

To stop the workload:

```bash
systemctl --user stop graft-example.service
```

Stopping removes the runtime container, but leaves the generated Quadlet file.
The current `Rootfs=...:O` overlay is not a persistent promote mechanism.
