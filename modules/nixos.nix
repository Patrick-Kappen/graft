{ config, lib, pkgs, ... }:

let
  cfg = config.services.graft;

  # Read all .toml files from configRoot
  tomlFiles = lib.optionalAttrs (cfg.configRoot != null)
    (lib.filterAttrs
      (name: type: type == "regular" && lib.hasSuffix ".toml" name)
      (builtins.readDir cfg.configRoot));

  # Parse each TOML file into a Nix attrset
  containers = lib.mapAttrs
    (name: _: builtins.fromTOML (builtins.readFile (cfg.configRoot + "/${name}")))
    tomlFiles;

  # Build a buildEnv per container from its package list
  containerEnvs = lib.mapAttrs (name: ctr:
    pkgs.buildEnv {
      name  = "graft-${lib.removeSuffix ".toml" name}-env";
      paths = map (p: pkgs.${p}) (ctr.config.runtime.packages or []);
    }
  ) containers;

  # Render a Quadlet .container file per container
  quadletFiles = lib.mapAttrs (name: ctr:
    let
      containerName = lib.removeSuffix ".toml" name;
      cmd     = lib.escapeShellArgs (ctr.config.runtime.command or []);
      restart = ctr.config.service.restart or "on-failure";
      env     = containerEnvs.${name};
    in ''
      [Container]
      ContainerName=${containerName}
      Rootfs=${env}:O
      Exec=${cmd}
      Volume=/nix/store:/nix/store:ro

      [Service]
      Restart=${restart}

      [Install]
      WantedBy=multi-user.target
    ''
  ) containers;

  # Only containers with deploy.enable = true and target = "system" go to systemd
  systemContainers = lib.filterAttrs (name: ctr:
    (ctr.deploy.enable or false) &&
    (ctr.deploy.target or "system") == "system"
  ) containers;

in {
  options.services.graft = {
    enable = lib.mkEnableOption "Graft — TOML-driven Podman Quadlet containers";

    configRoot = lib.mkOption {
      type        = lib.types.nullOr lib.types.path;
      default     = null;
      description = "Directory containing .toml container definitions.";
    };
  };

  config = lib.mkIf cfg.enable {
    # Ensure Podman is available (mkDefault so existing podman module takes precedence)
    virtualisation.podman.enable = lib.mkDefault true;

    # Place rendered Quadlet files for system containers in /etc/containers/systemd/
    environment.etc = lib.mapAttrs' (name: _:
      lib.nameValuePair
        "containers/systemd/${lib.removeSuffix ".toml" name}.container"
        { text = quadletFiles.${name}; }
    ) systemContainers;
  };
}
