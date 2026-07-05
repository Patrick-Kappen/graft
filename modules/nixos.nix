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

  # Build a rootfs per container:
  # - real directories for /etc, /tmp, /var, /home, /root, /run, /proc, /sys, /dev
  # - symlinks for bin, lib, etc from the inner buildEnv (skipping /etc to avoid symlink)
  containerEnvs = lib.mapAttrs (name: ctr:
    let
      containerName = lib.removeSuffix ".toml" name;
      inner = pkgs.buildEnv {
        name  = "graft-${containerName}-inner";
        paths = map (p: pkgs.${p}) (ctr.config.runtime.packages or []);
        ignoreCollisions = true;
      };
    in pkgs.runCommand "graft-${containerName}-env" {} ''
      # Real system directories (so overlay can write to them)
      mkdir -p $out/{etc,tmp,var,home,root,run,proc,sys,dev}

      # Symlink everything from the inner env except directories we own
      for entry in ${inner}/*; do
        name=$(basename "$entry")
        case "$name" in
          etc|tmp|var|home|root|run|proc|sys|dev) continue ;;
        esac
        ln -s "$entry" "$out/$name"
      done

      # Copy /etc contents from packages (if any) into our real /etc
      if [ -e ${inner}/etc ]; then
        cp -rL ${inner}/etc/. $out/etc/ 2>/dev/null || true
      fi
    ''
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
