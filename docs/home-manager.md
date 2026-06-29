# Home Manager module

The Home Manager route writes rootless/user Quadlet files from TOML.

```nix
{
  imports = [
    inputs.graft.homeManagerModules.default
  ];

  programs.graft = {
    enable = true;
    configRoot = ./containers;
  };
}
```

The module discovers `*.toml` recursively under `configRoot`, resolves the TOML graph, generates effective TOML in the Nix store, renders Quadlet, and writes user Quadlet files to:

```text
~/.config/containers/systemd/<name>.container
```

A discovered TOML file becomes Home Manager managed only when it is not no-op and explicitly targets user deployment:

```toml
[deploy]
enable = true
target = "user"
```

System-target TOML stays ignored by the Home Manager module:

```toml
[deploy]
enable = true
target = "system"
```

Use the NixOS module for system-target containers.

## Explicit files

Like the NixOS module, explicit files are still supported:

```nix
programs.graft = {
  enable = true;
  configFiles = [
    ./containers/dev-shell.toml
  ];
};
```

Explicit `configFiles` are active when they are not no-op.

## Nix-native authoring

Containers can also be authored directly in Nix instead of TOML files, via
`programs.graft.containers.<name>`:

```nix
programs.graft = {
  enable = true;
  containers.dev-shell.config.runtime = {
    mode = "rootfs-store";
    packages = [ "bashInteractive" "coreutils" ];
    command = [ "bash" "-l" ];
  };
};
```

The attribute name is the container name (unless the value sets its own `name`),
and the value mirrors the TOML schema (`version`, `name`, `parents`, `children`,
`deploy`, `validation`, `config`). It is serialized to TOML with
`pkgs.formats.toml` and flows through the same resolver and renderer as
file-based configs — no second engine. Nix-authored containers are always active
(like `configFiles`), so `[deploy] enable` is not required; `parents`/`children`
refs resolve against `configRoot`. See
[reference.md](reference.md#nix-native-authoring-containers).

## Graph and packages

The Home Manager module uses the same resolver as the NixOS module:

- `parents.add` / `parents.set` / `parents.remove`;
- `children.add` / `children.set` / `children.remove`;
- recursive attrset merge;
- list concat with de-duplication;
- `config.runtime.command` override;
- `config.runtime.packageOps` add/remove/replace;
- `config.runtime.packages` -> `pkgs.<name>` runtime closure.

## Activation

Home Manager writes the Quadlet files. Starting/enabling is still handled by user systemd/Podman Quadlet outside this first module slice, for example:

```bash
systemctl --user daemon-reload
systemctl --user start <name>.service
```

Autostart/session lifecycle policy will be added later.
